use std::{
    collections::HashSet,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::{
    Bulletin, Client, ForecastArea, Level, LiveQuery, LiveUpdate, Observation, Provider, Report,
    Result, ServerClock, Stamp, Tsunami, depth, entries, epicenter, magnitude, parse_time,
    socket::{connect_live, http_clock, read_live, read_timed_out},
    station_location, text,
};

const SOCKET: &str = "wss://api.p2pquake.net/v2/ws";
const HISTORY: &str = "https://api.p2pquake.net/v2/history";

fn scale(value: &Value) -> Option<Level> {
    let value = value.as_f64()?;
    let codes = [10., 20., 30., 40., 45., 50., 55., 60., 70.];
    if value == 0. {
        Some(Level::Other("0".into()))
    } else {
        codes
            .iter()
            .position(|&code| code == value)
            .map(|index| Level::Known(index as u8))
    }
}

fn read_quake(packet: &Value) -> Option<Report> {
    let event = &packet["earthquake"];
    let hypocenter = &event["hypocenter"];
    let name = text(&hypocenter["name"]);
    let origin = parse_time(text(&event["time"]));
    let issued =
        parse_time(text(&packet["issue"]["time"])).or_else(|| parse_time(text(&packet["time"])));
    let mut regions = Vec::new();
    let mut stations = Vec::new();
    for point in entries(&packet["points"]) {
        let name = text(&point["addr"]);
        let Some(level) = scale(&point["scale"]) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let region = point["isArea"].as_bool() == Some(true);
        let prefecture = text(&point["pref"]);
        let observation = Observation {
            name: name.into(),
            level,
            location: if region { None } else { station_location(name) },
            area_code: String::new(),
            prefecture: (!prefecture.is_empty()).then(|| prefecture.to_owned()),
        };
        if region {
            regions.push(observation);
        } else {
            stations.push(observation);
        }
    }
    for rows in [&mut regions, &mut stations] {
        rows.sort_by_key(|row| std::cmp::Reverse(row.level.index()));
    }
    let raw_id = text(&packet["id"]);
    if raw_id.is_empty() || (origin.is_none() && issued.is_none()) {
        return None;
    }
    let id = if name.is_empty() || origin.is_none() {
        format!("p2p:{raw_id}")
    } else {
        format!("p2p:{}:{name}", text(&event["time"]))
    };
    let tsunami_notice = match text(&event["domesticTsunami"]) {
        "None" => Some("心配なし"),
        "NonEffective" => Some("海面変動予想"),
        "Watch" => Some("注意報あり"),
        "Warning" => Some("警報等あり"),
        "Checking" => Some("調査中"),
        _ => None,
    };
    Some(Report {
        id,
        bulletin: Bulletin {
            provider: Provider::P2p,
            label: "地震情報".into(),
            name: name.into(),
            origin,
            issued,
            tsunami_notice,
            epicenter: epicenter(&hypocenter["longitude"], &hypocenter["latitude"]),
            magnitude: magnitude(&hypocenter["magnitude"]).unwrap_or_default(),
            depth: depth(&hypocenter["depth"]),
            maximum: scale(&event["maxScale"]),
            regions,
            stations,
            comment: text(&packet["comments"]["freeFormComment"]).into(),
            ..Bulletin::default()
        },
    })
}

fn tsunami(packet: &Value) -> Result<Tsunami> {
    let issue = parse_time(text(&packet["issue"]["time"])).ok_or("津波発表時刻が不正です")?;
    let cancelled = packet["cancelled"]
        .as_bool()
        .ok_or("津波発表区分が不正です")?;
    let areas = entries(&packet["areas"])
        .filter_map(|area| {
            let name = text(&area["name"]);
            if name.is_empty() {
                return None;
            }
            let level = match text(&area["grade"]) {
                "MajorWarning" => "大津波警報",
                "Warning" => "津波警報",
                "Watch" => "津波注意報",
                _ => "区分不明",
            };
            let mut line = format!("{level}  {name}");
            let height = text(&area["maxHeight"]["description"]);
            let first = &area["firstHeight"];
            let condition = text(&first["condition"]);
            if !height.is_empty() {
                line.push_str(&format!("  {height}"));
            }
            if !condition.is_empty() {
                line.push_str(&format!("  第1波 {condition}"));
            } else if let Some(at) = parse_time(text(&first["arrivalTime"])) {
                line.push_str(&format!("  第1波 {} JST", at.format("%m/%d %H:%M")));
            } else if area["immediate"].as_bool() == Some(true) {
                line.push_str("  第1波 直ちに来襲と予測");
            }
            Some(line)
        })
        .collect();
    Ok(Tsunami {
        provider: Provider::P2p,
        title: if cancelled {
            "津波警報・注意報の解除報"
        } else {
            "津波の発表内容"
        },
        alert: false,
        valid_until: None,
        issued: Some(issue),
        event_ids: Vec::new(),
        event_notice: None,
        areas,
    })
}

fn eew(packet: &Value) -> Result<Option<Bulletin>> {
    if packet["test"].as_bool() == Some(true) {
        return Ok(None);
    }
    let issue = &packet["issue"];
    let id = text(&issue["eventId"]);
    if id.is_empty() {
        return Err("EEW の eventId がありません".into());
    }
    let issued = parse_time(text(&issue["time"])).ok_or("EEW の発表時刻が不正です")?;
    let cancelled = packet["cancelled"]
        .as_bool()
        .ok_or("EEW の発表区分が不正です")?;
    let serial = issue["serial"]
        .as_u64()
        .or_else(|| text(&issue["serial"]).parse().ok());
    let mut label = if cancelled {
        "EEW 取消"
    } else {
        "EEW 警報"
    }
    .to_owned();
    if let Some(serial) = serial {
        label.push_str(&format!("  第{serial}報"));
    }
    let mut bulletin = Bulletin {
        provider: Provider::P2p,
        label,
        issued: Some(issued),
        eew: true,
        eew_serial: serial,
        eew_event_id: Some(id.into()),
        eew_cancelled: cancelled,
        warning: !cancelled,
        ..Bulletin::default()
    };
    if cancelled {
        return Ok(Some(bulletin));
    }
    let quake = &packet["earthquake"];
    let hypocenter = &quake["hypocenter"];
    bulletin.name = text(&hypocenter["name"]).into();
    bulletin.origin = parse_time(text(&quake["originTime"]));
    bulletin.epicenter = epicenter(&hypocenter["longitude"], &hypocenter["latitude"]);
    if text(&quake["condition"]) == "仮定震源要素" {
        bulletin.comment = "仮定震源要素".into();
    } else {
        bulletin.magnitude = magnitude(&hypocenter["magnitude"]).unwrap_or_default();
        bulletin.depth = depth(&hypocenter["depth"]);
    }
    let mut seen = HashSet::new();
    for area in entries(&packet["areas"]) {
        let name = text(&area["name"]);
        if name.is_empty() || !seen.insert(name) {
            continue;
        }
        bulletin.warning_areas.push(name.into());
        let minimum = scale(&area["scaleFrom"]);
        let maximum = scale(&area["scaleTo"]);
        if minimum.is_some() || maximum.is_some() {
            bulletin.forecast_areas.push(ForecastArea {
                name: name.into(),
                minimum,
                maximum,
            });
        }
    }
    if bulletin.forecast_areas.len() == bulletin.warning_areas.len()
        && bulletin
            .forecast_areas
            .iter()
            .all(|area| area.maximum.is_some())
    {
        bulletin.maximum = bulletin
            .forecast_areas
            .iter()
            .filter_map(|area| area.maximum.as_ref())
            .max_by_key(|level| level.index())
            .cloned();
    }
    Ok(Some(bulletin))
}

fn decode(packet: &Value) -> Option<LiveUpdate> {
    match packet["code"].as_u64()? {
        551 => read_quake(packet).map(|report| LiveUpdate::Quakes(Arc::from([report]))),
        552 => Some(match tsunami(packet) {
            Ok(report) => LiveUpdate::Tsunami(report),
            Err(error) => LiveUpdate::TsunamiInvalid(error.to_string()),
        }),
        556 => match eew(packet) {
            Ok(report) => report.map(|report| LiveUpdate::Eew(Box::new(report))),
            Err(error) => Some(LiveUpdate::EewInvalid(error.to_string())),
        },
        _ => None,
    }
}

fn history(client: &Client, query: &str) -> Result<(Vec<Value>, Option<ServerClock>)> {
    let mut response = client.request(&format!("{HISTORY}?{query}"), None)?;
    let clock = http_clock(response.headers(), Instant::now());
    Ok((response.body_mut().read_json()?, clock))
}

pub(super) fn last_quakes(client: &Client) -> Result<Arc<[Report]>> {
    let (packets, _) = history(client, "codes=551&limit=50")?;
    let mut cache = Cache::default();
    for report in packets.iter().filter_map(read_quake) {
        cache.add_quake(report);
    }
    Ok(cache.quakes())
}

pub(super) fn last_eew(client: &Client) -> Result<Option<Bulletin>> {
    let (packets, _) = history(client, "codes=556&limit=1")?;
    match packets.first() {
        Some(packet) => eew(packet),
        None => Ok(None),
    }
}

#[derive(Default)]
struct Cache {
    reports: Vec<Report>,
    tsunami_issue: Option<Stamp>,
}

impl Cache {
    fn add_quake(&mut self, report: Report) -> bool {
        if let Some(old) = self.reports.iter_mut().find(|old| old.id == report.id) {
            if old.bulletin.issued >= report.bulletin.issued {
                return false;
            }
            *old = report;
        } else {
            self.reports.push(report);
        }
        true
    }

    fn quakes(&mut self) -> Arc<[Report]> {
        self.reports.sort_by_key(|report| {
            std::cmp::Reverse(report.bulletin.origin.or(report.bulletin.issued))
        });
        self.reports.truncate(50);
        self.reports.clone().into()
    }

    fn accept(&mut self, update: LiveUpdate) -> Option<LiveUpdate> {
        match update {
            LiveUpdate::Tsunami(report) => {
                if report.issued <= self.tsunami_issue {
                    return None;
                }
                self.tsunami_issue = report.issued;
                Some(LiveUpdate::Tsunami(report))
            }
            LiveUpdate::Quakes(reports) => self
                .add_quake(reports.first()?.clone())
                .then(|| LiveUpdate::Quakes(self.quakes())),
            other => Some(other),
        }
    }
}

struct Snapshot {
    quakes: Result<(Vec<Value>, Option<ServerClock>)>,
    tsunami: Result<(Vec<Value>, Option<ServerClock>)>,
}

pub(super) fn connect(
    client: &Client,
    requests: &mpsc::Receiver<LiveQuery>,
    notify: &mut dyn FnMut(LiveUpdate) -> bool,
) -> Result<()> {
    let (mut socket, clock) = connect_live(SOCKET)?;
    if !notify(LiveUpdate::Connection(Provider::P2p, None)) {
        return Ok(());
    }
    let mut has_clock = clock.is_some();
    if let Some(clock) = clock
        && !notify(LiveUpdate::Clock(Provider::P2p, clock))
    {
        return Ok(());
    }
    let mut cache = Cache::default();
    let mut pending: Option<mpsc::Receiver<Snapshot>> = None;
    let mut refresh = true;
    let mut refresh_at = Instant::now();
    let mut last_message = refresh_at;
    let mut ping_at = refresh_at + Duration::from_secs(30);
    loop {
        loop {
            match requests.try_recv() {
                Ok(_) => refresh = true,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        if let Some(download) = &pending {
            match download.try_recv() {
                Ok(snapshot) => {
                    pending = None;
                    let clock = snapshot
                        .quakes
                        .as_ref()
                        .ok()
                        .and_then(|(_, clock)| *clock)
                        .or_else(|| snapshot.tsunami.as_ref().ok().and_then(|(_, clock)| *clock));
                    if !has_clock && let Some(clock) = clock {
                        has_clock = true;
                        if !notify(LiveUpdate::Clock(Provider::P2p, clock)) {
                            return Ok(());
                        }
                    }
                    if snapshot.quakes.is_err() || snapshot.tsunami.is_err() {
                        refresh = true;
                        refresh_at = Instant::now() + Duration::from_secs(30);
                    }
                    match snapshot.quakes {
                        Ok((packets, _)) => {
                            for report in packets.iter().filter_map(read_quake) {
                                cache.add_quake(report);
                            }
                            if !notify(LiveUpdate::Quakes(cache.quakes())) {
                                return Ok(());
                            }
                        }
                        Err(error) if !notify(LiveUpdate::HistoryInvalid(error.to_string())) => {
                            return Ok(());
                        }
                        Err(_) => {}
                    }
                    let tsunami = match snapshot.tsunami {
                        Ok((packets, _)) => packets.first().and_then(decode),
                        Err(error) => Some(LiveUpdate::TsunamiInvalid(format!(
                            "P2P 津波履歴取得失敗 · {error}"
                        ))),
                    };
                    if let Some(update) = tsunami.and_then(|update| cache.accept(update))
                        && !notify(update)
                    {
                        return Ok(());
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if pending.is_none() && refresh && Instant::now() >= refresh_at {
            refresh = false;
            refresh_at = Instant::now() + Duration::from_secs(5);
            let (send, receive) = mpsc::channel();
            let client = client.clone();
            thread::spawn(move || {
                let _ = send.send(Snapshot {
                    quakes: history(&client, "codes=551&limit=50"),
                    tsunami: history(&client, "codes=552&limit=1"),
                });
            });
            pending = Some(receive);
        }
        if Instant::now() >= ping_at {
            socket.send(tungstenite::Message::Ping(Vec::new().into()))?;
            ping_at = Instant::now() + Duration::from_secs(30);
        }
        match read_live(&mut socket) {
            Ok(packet) => {
                last_message = Instant::now();
                if let Some(update) = packet
                    .as_ref()
                    .and_then(decode)
                    .and_then(|update| cache.accept(update))
                    && !notify(update)
                {
                    return Ok(());
                }
            }
            Err(error)
                if read_timed_out(&error) && last_message.elapsed() < Duration::from_secs(85) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tsunami_delivery_has_no_current_validity() {
        let packet = json!({
            "code": 552,
            "cancelled": false,
            "issue": {"time": "2026/11/22 19:20:30"},
            "areas": [{"name": "岩手県", "grade": "Warning", "maxHeight": {"description": "3m"}}]
        });
        let Some(LiveUpdate::Tsunami(report)) = decode(&packet) else {
            panic!("expected P2P tsunami report");
        };
        assert_eq!(report.provider, Provider::P2p);
        assert!(!report.alert && report.valid_until.is_none());
        assert_eq!(report.areas, ["津波警報  岩手県  3m"]);
        assert!(matches!(
            decode(&json!({"code":552})),
            Some(LiveUpdate::TsunamiInvalid(_))
        ));
    }

    #[test]
    fn warning_preserves_open_ended_intensity_and_cancellation() {
        let mut packet = json!({
            "code": 556, "cancelled": false, "test": false,
            "issue": {"eventId":"20261122192030", "time":"2026/11/22 19:20:33", "serial":2},
            "areas": [{"name":"岩手県沿岸南部", "scaleFrom":45, "scaleTo":99}]
        });
        let report = eew(&packet).unwrap().unwrap();
        assert!(report.warning);
        assert_eq!(report.eew_serial, Some(2));
        assert_eq!(report.warning_areas, ["岩手県沿岸南部"]);
        assert_eq!(
            report.forecast_areas[0].minimum.as_ref().unwrap().index(),
            Some(4)
        );
        assert!(report.forecast_areas[0].maximum.is_none() && report.maximum.is_none());
        packet["cancelled"] = true.into();
        assert!(eew(&packet).unwrap().unwrap().eew_cancelled);
        packet["test"] = true.into();
        assert!(eew(&packet).unwrap().is_none());
    }
}
