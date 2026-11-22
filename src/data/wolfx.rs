use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc,
    time::{Duration, Instant},
};

use chrono::DateTime;
use serde::Deserialize;
use serde_json::Value;

use super::{
    Bulletin, ForecastArea, Level, LiveQuery, LiveUpdate, Provider, Result, ServerClock, Tsunami,
    depth, entries, epicenter, magnitude, parse_time,
    socket::{connect_live, read_live, read_timed_out},
    text,
    wolfx_details::read_reports,
};

const LIVE_SOCKET: &str = "wss://ws-api.wolfx.jp/all_eew?exclude=sc_eew,fj_eew,cq_eew,cenc_eew,cenc_eqlist,jma_special_eq";

impl LiveQuery {
    const ALL: [Self; 3] = [Self::Eew, Self::Quakes, Self::Tsunami];

    fn source(self) -> &'static str {
        match self {
            Self::Eew => "jma_eew",
            Self::Quakes => "jma_eqlist",
            Self::Tsunami => "jma_tsunami",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Eew => "query_jmaeew",
            Self::Quakes => "query_jmaeqlist",
            Self::Tsunami => "query_jmatsunami",
        }
    }
}

pub(super) fn live_update(packet: &Value, requested: Option<LiveQuery>) -> Option<LiveUpdate> {
    let kind = match text(&packet["type"]) {
        "" => requested?.source(),
        kind => kind,
    };
    match kind {
        "jma_eew" => match read_eew(packet) {
            Ok(report) => report.map(|report| LiveUpdate::Eew(Box::new(report))),
            Err(error) => Some(LiveUpdate::EewInvalid(error.to_string())),
        },
        "jma_eqlist" if packet.is_object() => Some(LiveUpdate::Quakes(read_reports(packet).into())),
        "jma_tsunami" => Some(match read_tsunami(packet) {
            Ok(report) => LiveUpdate::Tsunami(report),
            Err(error) => LiveUpdate::TsunamiInvalid(error.to_string()),
        }),
        _ => None,
    }
}

pub(super) fn live_snapshot(query: LiveQuery) -> Result<LiveUpdate> {
    let (mut socket, _) = connect_live(&format!("wss://ws-api.wolfx.jp/{}", query.source()))?;
    socket.send(tungstenite::Message::Text(query.command().into()))?;
    let deadline = Instant::now() + Duration::from_secs(7);
    while Instant::now() < deadline {
        match read_live(&mut socket) {
            Ok(Some(packet)) => {
                if let Some(update) = live_update(&packet, Some(query)) {
                    return Ok(update);
                }
            }
            Ok(None) => {}
            Err(error) if read_timed_out(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err("Wolfx WebSocket の応答がありません".into())
}

fn server_clock(packet: &Value, received: Instant) -> Option<ServerClock> {
    let value = &packet["timestamp"];
    let milliseconds = value.as_i64().or_else(|| text(value).parse().ok())?;
    let utc = DateTime::from_timestamp_millis(milliseconds)?;
    Some(ServerClock { utc, received })
}

pub(super) fn live_connection(
    requests: &mpsc::Receiver<LiveQuery>,
    notify: &mut dyn FnMut(LiveUpdate) -> bool,
) -> Result<()> {
    let (mut socket, _) = connect_live(LIVE_SOCKET)?;
    let ready_by = Instant::now() + Duration::from_secs(7);
    let mut ready = false;
    let mut initial = Vec::new();
    let mut pending = VecDeque::from(LiveQuery::ALL);
    let mut next_query = Instant::now();
    let mut last_message = next_query;
    let mut last_quakes = String::new();
    loop {
        if !ready && Instant::now() >= ready_by {
            return Err("サーバー時刻の応答がありません".into());
        }
        loop {
            match requests.try_recv() {
                Ok(query) if !pending.contains(&query) => pending.push_back(query),
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if Instant::now() >= next_query
            && let Some(query) = pending.pop_front()
        {
            socket.send(tungstenite::Message::Text(query.command().into()))?;
            // Wolfx may discard manual queries sent in a burst.
            next_query = Instant::now() + Duration::from_millis(1100);
        }

        match read_live(&mut socket) {
            Ok(packet) => {
                last_message = Instant::now();
                if let Some(packet) = packet {
                    let kind = text(&packet["type"]);
                    if matches!(kind, "heartbeat" | "pong") {
                        if let Some(clock) = server_clock(&packet, last_message) {
                            if !ready && !notify(LiveUpdate::Connection(Provider::Wolfx, None)) {
                                return Ok(());
                            }
                            ready = true;
                            if !notify(LiveUpdate::Clock(Provider::Wolfx, clock))
                                || !initial.drain(..).all(&mut *notify)
                            {
                                return Ok(());
                            }
                        }
                        continue;
                    }
                    let inferred_query = if kind.is_empty() {
                        if packet.get("No1").is_some() {
                            Some(LiveQuery::Quakes)
                        } else if packet.get("trusted").is_some() {
                            Some(LiveQuery::Tsunami)
                        } else if packet.get("EventID").is_some() {
                            Some(LiveQuery::Eew)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(update) = live_update(&packet, inferred_query) {
                        if matches!(&update, LiveUpdate::Quakes(_)) {
                            let hash = text(&packet["md5"]);
                            if !hash.is_empty() && hash == last_quakes {
                                continue;
                            }
                            last_quakes = hash.to_owned();
                        }
                        if ready {
                            if !notify(update) {
                                return Ok(());
                            }
                        } else if initial.len() < 64 {
                            initial.push(update);
                        }
                    }
                }
            }
            Err(error)
                if read_timed_out(&error) && last_message.elapsed() < Duration::from_secs(85) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn forecast_level(value: &str) -> Option<Level> {
    let value = value.trim();
    if matches!(value, "不明" | "未設定" | "-" | "-1") || value.chars().all(|c| c == '/') {
        return None;
    }
    Level::parse(value)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WolfxEew {
    #[serde(rename = "EventID")]
    event_id: Option<String>,
    announced_time: Option<String>,
    origin_time: Option<String>,
    hypocenter: Option<String>,
    code_type: Option<String>,
    max_intensity: Option<String>,
    issue: Option<WolfxIssue>,
    #[serde(default)]
    serial: Value,
    #[serde(default)]
    latitude: Value,
    #[serde(default)]
    longitude: Value,
    #[serde(default)]
    depth: Value,
    #[serde(default)]
    magnitude: Value,
    #[serde(default)]
    magunitude: Value,
    warn_area: Option<Vec<WolfxEewArea>>,
    #[serde(default, rename = "isWarn")]
    warning: Option<bool>,
    #[serde(default, rename = "isTraining")]
    training: Option<bool>,
    #[serde(default, rename = "isAssumption")]
    assumed: Option<bool>,
    #[serde(default, rename = "isCancel")]
    cancelled: Option<bool>,
    #[serde(default, rename = "isFinal")]
    final_report: Option<bool>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WolfxIssue {
    status: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WolfxEewArea {
    chiiki: Option<String>,
    #[serde(rename = "Type")]
    kind: Option<String>,
    shindo1: Option<String>,
    shindo2: Option<String>,
}

pub(super) fn read_eew(packet: &Value) -> Result<Option<Bulletin>> {
    let packet = WolfxEew::deserialize(packet)?;
    if packet.training == Some(true)
        || matches!(
            packet.issue.and_then(|issue| issue.status).as_deref(),
            Some("訓練" | "試験")
        )
    {
        return Ok(None);
    }
    let event_id = packet
        .event_id
        .filter(|id| !id.is_empty())
        .ok_or("EventID がありません")?;
    let issued = packet
        .announced_time
        .as_deref()
        .and_then(parse_time)
        .ok_or("AnnouncedTime が不正です")?;
    let cancelled = packet.cancelled == Some(true);
    let assumed = packet.assumed == Some(true);
    let kind = packet.code_type.as_deref().unwrap_or_default();
    let intensity_only = kind.starts_with("最大予測震度のみ");
    let warning = !cancelled && packet.warning == Some(true);
    let serial = packet
        .serial
        .as_u64()
        .or_else(|| text(&packet.serial).parse().ok())
        .filter(|&n| n > 0);
    let final_report = !cancelled && packet.final_report == Some(true);
    let mut label = if cancelled {
        "EEW 取消"
    } else if warning {
        "EEW 警報"
    } else {
        "EEW 予報"
    }
    .to_owned();
    if let Some(serial) = serial {
        label.push_str(&format!("  第{serial}報"));
    }
    if final_report {
        label.push_str("（最終報）");
    }
    let mut warning_areas = Vec::new();
    let mut forecast_areas = Vec::new();
    if !cancelled {
        let mut warnings_seen = HashSet::new();
        let mut forecasts_seen = HashSet::new();
        for area in packet.warn_area.as_deref().into_iter().flatten() {
            let name = area.chiiki.as_deref().unwrap_or_default().trim();
            if name.is_empty() {
                continue;
            }
            if warning
                && matches!(area.kind.as_deref().unwrap_or_default(), "" | "警報")
                && warnings_seen.insert(name)
            {
                warning_areas.push(name.into());
            }
            let maximum = area.shindo1.as_deref().and_then(forecast_level);
            let minimum = area.shindo2.as_deref().and_then(forecast_level);
            if (minimum.is_some() || maximum.is_some()) && forecasts_seen.insert(name) {
                forecast_areas.push(ForecastArea {
                    name: name.into(),
                    minimum,
                    maximum,
                });
            }
        }
    }
    let mut bulletin = Bulletin {
        label,
        name: packet.hypocenter.unwrap_or_default(),
        eew: true,
        eew_cancelled: cancelled,
        eew_serial: serial,
        eew_final: final_report,
        eew_event_id: Some(event_id),
        warning,
        warning_areas,
        forecast_areas,
        origin: (!intensity_only)
            .then(|| packet.origin_time.as_deref().and_then(parse_time))
            .flatten(),
        issued: Some(issued),
        comment: if assumed && !cancelled {
            format!("{kind}\n仮定震源要素")
        } else {
            kind.into()
        },
        ..Bulletin::default()
    };
    if !cancelled {
        bulletin.maximum = packet.max_intensity.as_deref().and_then(Level::parse);
        if !intensity_only {
            bulletin.epicenter = epicenter(&packet.longitude, &packet.latitude);
            if !assumed {
                bulletin.depth = depth(&packet.depth).filter(|&km| km != 0);
                bulletin.magnitude = magnitude(&packet.magnitude)
                    .or_else(|| magnitude(&packet.magunitude))
                    .filter(|v| v != "0.0")
                    .unwrap_or_default();
            }
        }
    }
    Ok(Some(bulletin))
}

pub(super) fn read_tsunami(packet: &Value) -> Result<Tsunami> {
    let (title, alert) = match text(&packet["status"]) {
        "active" => ("津波警報・注意報 発表中", true),
        "forecast_only" => ("津波予報 発表中", false),
        "cleared" => ("津波警報・注意報 解除", false),
        "expired" => ("発表の有効期限切れ", false),
        "none_reported" => ("津波なしの発表", false),
        "unknown" => ("津波の状況は不明です", false),
        "uninitialized" => ("津波情報をまだ受信していません", false),
        _ => return Err("Wolfx 津波情報の状態が不正です".into()),
    };
    let valid_until = (packet["trusted"].as_bool() == Some(true))
        .then(|| parse_time(text(&packet["validUntil"])))
        .flatten();
    let areas = entries(&packet["areas"])
        .filter_map(|area| {
            let name = text(&area["name"]);
            if name.is_empty() {
                return None;
            }
            let level = match text(&area["level"]) {
                "major_warning" => "大津波警報",
                "warning" => "津波警報",
                "advisory" => "津波注意報",
                "forecast" => "津波予報",
                "cleared" => "解除",
                "none" => "津波なし",
                _ => "区分不明",
            };
            let mut area_line = format!("{level}  {name}");
            let wave = &area["maxWave"];
            let height = text(&wave["text"]);
            if !height.is_empty() {
                area_line.push_str(&format!("  {height}"));
            } else if let Some(height) = wave["height"].as_f64() {
                let condition = text(&wave["condition"]);
                area_line.push_str(&format!("  {height} m {condition}"));
            }
            let first = &area["firstWave"];
            let first_text = text(&first["text"]);
            if !first_text.is_empty() {
                area_line.push_str(&format!("  第1波 {first_text}"));
            } else if let Some(arrival) = parse_time(text(&first["time"])) {
                area_line.push_str(&format!("  第1波 {} JST", arrival.format("%m/%d %H:%M")));
            }
            Some(area_line)
        })
        .collect();
    let event_notice = match text(&packet["status"]) {
        "active" => Some("警報等あり"),
        "forecast_only" => Some("予報あり"),
        "cleared" => Some("解除"),
        "none_reported" => Some("なし"),
        _ => None,
    };
    Ok(Tsunami {
        provider: Provider::Wolfx,
        title,
        event_notice,
        event_ids: entries(&packet["report"]["eventIds"])
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        alert,
        valid_until,
        issued: parse_time(text(&packet["report"]["time"])),
        areas,
    })
}
