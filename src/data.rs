use std::{
    collections::{HashMap, HashSet, VecDeque},
    error::Error,
    net::{TcpStream, ToSocketAddrs},
    sync::{Arc, Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use serde_json::Value;

const EARTHQUAKE_DETAIL: &str = "https://api.wolfx.jp/jma_earthquake_detail.json";
const LIVE_SOCKET: &str = "wss://ws-api.wolfx.jp/all_eew?exclude=sc_eew,fj_eew,cq_eew,cenc_eew,cenc_eqlist,jma_special_eq";
pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub lon: f64,
    pub lat: f64,
}

impl Location {
    pub fn new(lon: f64, lat: f64) -> Option<Self> {
        ((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat))
            .then_some(Self { lon, lat })
    }
}

pub const INTENSITIES: [&str; 9] = ["1", "2", "3", "4", "5弱", "5強", "6弱", "6強", "7"];

#[derive(Clone, Debug)]
pub enum Level {
    Known(u8),
    Other(String),
}

impl Level {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let value = value.replace(['－', '−'], "-").replace('＋', "+");
        let index = match value.as_str() {
            "1" => 0,
            "2" => 1,
            "3" => 2,
            "4" => 3,
            "5-" | "5弱" => 4,
            "5+" | "5強" => 5,
            "6-" | "6弱" => 6,
            "6+" | "6強" => 7,
            "7" => 8,
            "!" | "over" | "震度５弱以上未入電" => {
                return Some(Self::Other("5弱以上・未入電".into()));
            }
            _ => return Some(Self::Other(value)),
        };
        Some(Self::Known(index))
    }

    pub fn index(&self) -> Option<u8> {
        match self {
            Self::Known(index) => Some(*index),
            Self::Other(_) => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Known(index) => INTENSITIES[*index as usize],
            Self::Other(label) => label,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub name: String,
    pub level: Level,
    pub location: Option<Location>,
    pub area_code: String,
    pub prefecture: Option<String>,
}

pub type Stamp = DateTime<FixedOffset>;

fn parse_time(value: &str) -> Option<Stamp> {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.with_timezone(&FixedOffset::east_opt(9 * 3600)?));
    }
    let parsed = ["%Y/%m/%d %H:%M:%S%.f", "%Y/%m/%d %H:%M"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok())?;
    parsed
        .and_local_timezone(FixedOffset::east_opt(9 * 3600)?)
        .single()
}

#[derive(Clone, Debug)]
pub struct ForecastArea {
    pub name: String,
    pub minimum: Option<Level>,
    pub maximum: Option<Level>,
}

#[derive(Clone, Debug, Default)]
pub struct Bulletin {
    pub label: String,
    pub name: String,
    pub origin: Option<Stamp>,
    pub issued: Option<Stamp>,
    pub epicenter: Option<Location>,
    pub magnitude: String,
    pub depth: Option<u32>,
    pub maximum: Option<Level>,
    pub warning: bool,
    pub eew: bool,
    pub eew_cancelled: bool,
    pub eew_serial: Option<u64>,
    pub eew_final: bool,
    pub eew_event_id: Option<String>,
    pub eew_notice: Option<bool>,
    pub tsunami_notice: Option<&'static str>,
    pub warning_areas: Vec<String>,
    pub forecast_areas: Vec<ForecastArea>,
    pub stations: Vec<Observation>,
    pub regions: Vec<Observation>,
    pub comment: String,
}

#[derive(Clone)]
pub struct Report {
    pub id: String,
    pub bulletin: Bulletin,
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default().trim()
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| text(value).parse().ok())
}

fn epicenter(lon: &Value, lat: &Value) -> Option<Location> {
    let (lon, lat) = (number(lon)?, number(lat)?);
    if (lon, lat) == (0.0, 0.0) {
        return None;
    }
    Location::new(lon, lat)
}

fn magnitude(value: &Value) -> Option<String> {
    let value = number(value)?;
    (value.is_finite() && value >= 0.0).then(|| format!("{value:.1}"))
}

fn depth(value: &Value) -> Option<u32> {
    let km = value.as_f64().or_else(|| {
        let raw = text(value);
        raw.strip_suffix("km").unwrap_or(raw).trim().parse().ok()
    })?;
    (0.0..=1000.0).contains(&km).then(|| km.round() as u32)
}

fn entries(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn indexed_entry<'a>(array: &'a Value, index: &Value) -> Option<&'a Value> {
    array.get(usize::try_from(index.as_u64()?).ok()?)
}

fn station_location(name: &str) -> Option<Location> {
    static STATIONS: OnceLock<HashMap<&'static str, [f64; 2]>> = OnceLock::new();
    let locations = STATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../stations.json")).expect("station locations")
    });
    let &[lon, lat] = locations.get(name.trim_end_matches(['＊', '*']).trim_end())?;
    Location::new(lon, lat)
}

fn report_time(report: &Value) -> Option<Stamp> {
    parse_time(text(&report["time"])).or_else(|| parse_time(text(&report["issuedAt"])))
}

fn eew_notice(message: &str) -> Option<bool> {
    if message.contains("緊急地震速報を発表しています")
        || message.contains("緊急地震速報を発表しました")
    {
        Some(true)
    } else if message.contains("緊急地震速報を発表していません") {
        Some(false)
    } else {
        None
    }
}

fn tsunami_notice(message: &str) -> Option<&'static str> {
    if message.contains("津波の心配はありません") {
        Some("心配なし")
    } else if message.contains("若干の海面変動") {
        Some("海面変動予想")
    } else if message.contains("津波警報等を発表中")
        || message.contains("津波警報等（大津波警報・津波警報あるいは津波注意報）を発表中")
    {
        Some("警報等あり")
    } else {
        None
    }
}

fn report_notices(report: &Value) -> impl Iterator<Item = &str> {
    ["forecast", "warning", "note", "text"]
        .into_iter()
        .map(move |key| text(&report["comments"][key]))
        .chain(std::iter::once(text(&report["text"])))
}

fn event_report(event: &Value) -> Option<Report> {
    let id = text(&event["eventId"]);
    if id.is_empty() || text(&event["status"]) == "cancelled" {
        return None;
    }
    let mut products = HashMap::<&str, &Value>::new();
    let rank = |item: &Value| {
        (
            report_time(item),
            parse_time(text(&item["issuedAt"])),
            match text(&item["status"]) {
                "cancelled" => 2,
                "corrected" => 1,
                _ => 0,
            },
        )
    };
    for report in entries(&event["reports"]) {
        let product = text(&report["product"]);
        if !product.is_empty()
            && products
                .get(product)
                .is_none_or(|prior| rank(prior) <= rank(report))
        {
            products.insert(product, report);
        }
    }
    products.retain(|_, item| matches!(text(&item["status"]), "issued" | "corrected"));
    let active_reports = || {
        entries(&event["reports"]).filter(|report| {
            matches!(text(&report["status"]), "issued" | "corrected")
                && products.contains_key(text(&report["product"]))
        })
    };
    // Take an entire observation from the preferred product. A different
    // product's later observations must not be mixed into the same map.
    let observation_report = ["VXSE53", "VXSE51", "VXSE62", "VXSE61"]
        .iter()
        .find_map(|&product| {
            active_reports()
                .filter(|r| text(&r["product"]) == product && r["observation"].is_object())
                .max_by_key(|report| rank(report))
        });
    let hypocenter_report = active_reports()
        .filter(|report| report["hypocenter"].is_u64())
        .max_by_key(|report| rank(report));
    let summary = &event["summary"];
    let source = observation_report.or(hypocenter_report).or_else(|| {
        (text(&event["status"]) == "preliminary" && summary.is_object()).then_some(summary)
    })?;
    let hypocenter_source = if source["hypocenter"].is_u64() {
        source
    } else {
        hypocenter_report.unwrap_or(source)
    };
    let hypocenter = hypocenter_source["hypocenter"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| event["hypocenters"].get(index))
        .unwrap_or(&Value::Null);
    let observed = &source["observation"];
    let areas = &observed["areas"];
    let mut regions: Vec<_> = entries(areas)
        .filter_map(|area| {
            Some(Observation {
                name: text(&area["name"]).into(),
                level: Level::parse(text(&area["int"]))?,
                location: None,
                area_code: text(&area["code"]).into(),
                prefecture: None,
            })
        })
        .collect();
    let mut seen = HashSet::new();
    let station_sets = entries(areas)
        .map(|area| (Some(area), &area["stations"]))
        .chain(
            entries(&observed["cities"])
                .map(|city| (indexed_entry(areas, &city["area"]), &city["stations"])),
        );
    let mut stations = Vec::new();
    for (area, points) in station_sets {
        let prefecture = area
            .and_then(|area| indexed_entry(&observed["prefs"], &area["pref"]))
            .map(|pref| text(&pref["name"]).trim())
            .filter(|name| !name.is_empty());
        for station in entries(points) {
            let name = text(&station["name"]);
            let code = text(&station["code"]);
            let Some(level) = Level::parse(text(&station["int"])) else {
                continue;
            };
            if !name.is_empty() && seen.insert(if code.is_empty() { name } else { code }) {
                stations.push(Observation {
                    name: name.into(),
                    level,
                    location: epicenter(&station["longitude"], &station["latitude"])
                        .or_else(|| station_location(name)),
                    area_code: String::new(),
                    prefecture: prefecture.map(str::to_owned),
                });
            }
        }
    }
    for rows in [&mut regions, &mut stations] {
        rows.sort_by_key(|row| std::cmp::Reverse(row.level.index()));
    }
    let comments = &source["comments"];
    let mut comment: Vec<_> = ["forecast", "warning", "note", "text"]
        .into_iter()
        .map(|key| text(&comments[key]))
        .filter(|value| !value.is_empty())
        .collect();
    if !text(&source["text"]).is_empty() {
        comment.push(text(&source["text"]));
    }
    if text(&event["status"]) == "preliminary" {
        comment.push("Wolfx の暫定概要です。");
    }
    let mut comment = comment.join("\n");
    let period = text(&observed["maxLongPeriod"]);
    if !period.is_empty() {
        if !comment.is_empty() {
            comment.push('\n');
        }
        comment.push_str(&format!("最大長周期地震動階級 {period}"));
    }
    let eew_notice = active_reports()
        .filter_map(|report| {
            report_notices(report)
                .find_map(eew_notice)
                .map(|notice| (rank(report), notice))
        })
        .max_by_key(|&(rank, _)| rank)
        .map(|(_, notice)| notice);
    let tsunami_notice = active_reports()
        .filter_map(|report| {
            report_notices(report)
                .find_map(tsunami_notice)
                .map(|notice| (rank(report), notice))
        })
        .max_by_key(|&(rank, _)| rank)
        .map(|(_, notice)| notice);
    let bulletin = Bulletin {
        eew_notice,
        tsunami_notice,
        label: if regions.is_empty() && stations.is_empty() {
            "地震情報"
        } else {
            "観測震度"
        }
        .into(),
        name: text(&hypocenter["name"]).into(),
        origin: parse_time(text(&hypocenter_source["originTime"]))
            .or_else(|| parse_time(text(&source["originTime"]))),
        issued: report_time(source),
        epicenter: epicenter(&hypocenter["longitude"], &hypocenter["latitude"]),
        magnitude: magnitude(&hypocenter_source["magnitude"]).unwrap_or_default(),
        depth: depth(&hypocenter["depth"]),
        maximum: Level::parse(text(&observed["maxIntensity"]))
            .or_else(|| Level::parse(text(&source["maxIntensity"]))),
        regions,
        stations,
        comment,
        ..Bulletin::default()
    };
    Some(Report {
        id: id.into(),
        bulletin,
    })
}

fn read_reports(packet: &Value) -> Vec<Report> {
    let Some(entries) = packet.as_object() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries
        .iter()
        .filter_map(|(key, value)| Some((key.strip_prefix("No")?.parse::<u8>().ok()?, value)))
        .filter(|(number, _)| (1..=50).contains(number))
        .collect();
    entries.sort_by_key(|(number, _)| *number);
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter_map(|(_, item)| {
            let origin =
                parse_time(text(&item["time_full"])).or_else(|| parse_time(text(&item["time"])))?;
            let name = text(&item["location"]);
            let id = match text(&item["EventID"]) {
                "" => format!("{origin}/{name}"),
                id => id.to_owned(),
            };
            if !seen.insert(id.clone()) {
                return None;
            }
            let info = text(&item["info"]);
            let comment = if info.is_empty() {
                "Wolfx 地震一覧では最大震度を表示します。".into()
            } else {
                format!("{info}\n\nWolfx 地震一覧では最大震度を表示します。")
            };
            Some(Report {
                id,
                bulletin: Bulletin {
                    label: "地震情報".into(),
                    name: name.into(),
                    origin: Some(origin),
                    epicenter: epicenter(&item["longitude"], &item["latitude"]),
                    depth: depth(&item["depth"]).filter(|&km| km != 0),
                    magnitude: magnitude(&item["magnitude"])
                        .filter(|v| v != "0.0")
                        .unwrap_or_default(),
                    maximum: Level::parse(text(&item["shindo"])),
                    eew_notice: eew_notice(info),
                    tsunami_notice: tsunami_notice(info),
                    comment,
                    ..Bulletin::default()
                },
            })
        })
        .collect()
}

impl Bulletin {
    pub fn fresh(&self) -> bool {
        self.issued.as_ref().is_some_and(|stamp| {
            (-10..=120).contains(&(Utc::now().timestamp() - stamp.timestamp()))
        })
    }

    pub fn has_observations(&self) -> bool {
        !self.stations.is_empty() || !self.regions.is_empty()
    }
}

fn forecast_level(value: &Value) -> Option<Level> {
    let value = text(value).trim();
    if matches!(value, "不明" | "未設定" | "-" | "-1") || value.chars().all(|c| c == '/') {
        return None;
    }
    Level::parse(value)
}

fn read_eew(packet: &Value) -> Option<Bulletin> {
    let flag = |key: &str| packet[key].as_bool().unwrap_or(false);
    if text(&packet["EventID"]).is_empty()
        || flag("isTraining")
        || matches!(text(&packet["Issue"]["Status"]), "訓練" | "試験")
    {
        return None;
    }
    let issued = parse_time(text(&packet["AnnouncedTime"]))?;
    let cancelled = flag("isCancel");
    let assumed = flag("isAssumption");
    let kind = text(&packet["CodeType"]);
    let intensity_only = kind.starts_with("最大予測震度のみ");
    let warning = !cancelled && flag("isWarn");
    let serial = packet["Serial"]
        .as_u64()
        .or_else(|| text(&packet["Serial"]).parse().ok())
        .filter(|&n| n > 0);
    let final_report = !cancelled && flag("isFinal");
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
        for area in entries(&packet["WarnArea"]) {
            let name = text(&area["Chiiki"]).trim();
            if name.is_empty() {
                continue;
            }
            if warning && matches!(text(&area["Type"]), "" | "警報") && warnings_seen.insert(name)
            {
                warning_areas.push(name.into());
            }
            let maximum = forecast_level(&area["Shindo1"]);
            let minimum = forecast_level(&area["Shindo2"]);
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
        name: text(&packet["Hypocenter"]).into(),
        eew: true,
        eew_cancelled: cancelled,
        eew_serial: serial,
        eew_final: final_report,
        eew_event_id: Some(text(&packet["EventID"]).into()),
        warning,
        warning_areas,
        forecast_areas,
        origin: (!intensity_only)
            .then(|| parse_time(text(&packet["OriginTime"])))
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
        bulletin.maximum = Level::parse(text(&packet["MaxIntensity"]));
        if !intensity_only {
            bulletin.epicenter = epicenter(&packet["Longitude"], &packet["Latitude"]);
            if !assumed {
                bulletin.depth = depth(&packet["Depth"]).filter(|&km| km != 0);
                bulletin.magnitude = magnitude(&packet["Magnitude"])
                    .or_else(|| magnitude(&packet["Magunitude"]))
                    .filter(|v| v != "0.0")
                    .unwrap_or_default();
            }
        }
    }
    Some(bulletin)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveQuery {
    Eew,
    Quakes,
    Tsunami,
}

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

pub enum LiveUpdate {
    Connection(bool),
    Eew(Box<Bulletin>),
    Quakes(Arc<[Report]>),
    Tsunami(Tsunami),
}

fn live_update(packet: &Value, requested: Option<LiveQuery>) -> Option<LiveUpdate> {
    let kind = match text(&packet["type"]) {
        "" => requested?.source(),
        kind => kind,
    };
    match kind {
        "jma_eew" => read_eew(packet).map(|report| LiveUpdate::Eew(Box::new(report))),
        "jma_eqlist" if packet.is_object() => Some(LiveUpdate::Quakes(read_reports(packet).into())),
        "jma_tsunami" => read_tsunami(packet).ok().map(LiveUpdate::Tsunami),
        _ => None,
    }
}

type LiveSocket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

fn connect_live(url: &str) -> Result<LiveSocket> {
    use tungstenite::{
        client::IntoClientRequest, client_tls_with_config, http::HeaderValue,
        protocol::WebSocketConfig, stream::MaybeTlsStream,
    };

    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        "User-Agent",
        HeaderValue::from_static(concat!("TuiQuake/", env!("CARGO_PKG_VERSION"))),
    );
    let host = request
        .uri()
        .host()
        .ok_or("Wolfx WebSocket のホストが不明です")?;
    let timeout = Duration::from_secs(7);
    let tcp = (host, 443)
        .to_socket_addrs()?
        .find_map(|addr| TcpStream::connect_timeout(&addr, timeout).ok())
        .ok_or("Wolfx WebSocket に接続できません")?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    let config = WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (mut socket, _) = client_tls_with_config(request, tcp, Some(config), None)?;
    let MaybeTlsStream::Rustls(stream) = socket.get_mut() else {
        return Err("Wolfx WebSocket が TLS を使用していません".into());
    };
    stream
        .sock
        .set_read_timeout(Some(Duration::from_millis(300)))?;
    Ok(socket)
}

fn read_live(socket: &mut LiveSocket) -> tungstenite::Result<Option<Value>> {
    use tungstenite::{Error, Message};
    match socket.read()? {
        Message::Text(message) => Ok(serde_json::from_str(message.as_str()).ok()),
        Message::Ping(_) => {
            socket.flush()?;
            Ok(None)
        }
        Message::Close(_) => {
            let _ = socket.flush();
            Err(Error::ConnectionClosed)
        }
        _ => Ok(None),
    }
}

fn read_timed_out(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error)
        if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock))
}

fn live_snapshot(query: LiveQuery) -> Result<LiveUpdate> {
    let mut socket = connect_live(&format!("wss://ws-api.wolfx.jp/{}", query.source()))?;
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

fn live_connection(
    requests: &mpsc::Receiver<LiveQuery>,
    notify: &mut impl FnMut(LiveUpdate) -> bool,
) -> Result<()> {
    let mut socket = connect_live(LIVE_SOCKET)?;
    if !notify(LiveUpdate::Connection(true)) {
        return Ok(());
    }

    let mut pending = VecDeque::from(LiveQuery::ALL);
    let mut next_query = Instant::now();
    let mut last_message = next_query;
    let mut last_quakes = String::new();
    loop {
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
                        if !notify(update) {
                            return Ok(());
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

pub fn stream_live(
    requests: mpsc::Receiver<LiveQuery>,
    mut notify: impl FnMut(LiveUpdate) -> bool,
) {
    let mut retry = Duration::from_secs(5);
    loop {
        let started = Instant::now();
        if live_connection(&requests, &mut notify).is_ok() || !notify(LiveUpdate::Connection(false))
        {
            break;
        }
        if started.elapsed() >= Duration::from_secs(60) {
            retry = Duration::from_secs(5);
        }
        thread::sleep(retry);
        retry = (retry * 2).min(Duration::from_secs(60));
    }
}

pub fn merge_reports(list: &Arc<[Report]>, details: &Arc<[Report]>) -> Arc<[Report]> {
    if list.is_empty() {
        return Arc::clone(details);
    }
    if details.is_empty() {
        return Arc::clone(list);
    }
    let detailed_ids: HashSet<_> = details.iter().map(|report| &report.id).collect();
    let mut new_reports = list
        .iter()
        .filter(|report| !detailed_ids.contains(&report.id))
        .peekable();
    if new_reports.peek().is_none() {
        return Arc::clone(details);
    }
    let mut joined = details.to_vec();
    joined.extend(new_reports.cloned());
    joined
        .sort_by_key(|report| std::cmp::Reverse(report.bulletin.origin.or(report.bulletin.issued)));
    joined.truncate(100);
    joined.into()
}

pub struct Tsunami {
    pub title: &'static str,
    event_ids: Vec<String>,
    event_notice: Option<&'static str>,
    pub alert: bool,
    pub valid_until: Option<Stamp>,
    pub issued: Option<Stamp>,
    pub areas: Vec<String>,
}

impl Tsunami {
    pub fn notice_for(&self, id: &str) -> Option<&'static str> {
        self.event_notice
            .filter(|_| self.event_ids.iter().any(|event| event == id))
    }

    pub fn current(&self, now: DateTime<Utc>) -> bool {
        self.valid_until.is_some_and(|until| now < until)
    }
}

fn read_tsunami(packet: &Value) -> Result<Tsunami> {
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

struct CachedReports {
    etag: Option<String>,
    reports: Arc<[Report]>,
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    cached: Arc<Mutex<Option<CachedReports>>>,
}

impl Client {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(7)))
                .build()
                .into(),
            cached: Arc::default(),
        }
    }

    fn request(&self, url: &str, etag: Option<&str>) -> Result<ureq::http::Response<ureq::Body>> {
        let mut request = self
            .agent
            .get(url)
            .header(
                "User-Agent",
                concat!("TuiQuake/", env!("CARGO_PKG_VERSION")),
            )
            .header("Accept", "application/json");
        if let Some(etag) = etag {
            request = request.header("If-None-Match", etag);
        }
        Ok(request.call()?)
    }

    fn detailed_reports(&self, cached: Option<&CachedReports>) -> Result<CachedReports> {
        let mut response =
            self.request(EARTHQUAKE_DETAIL, cached.and_then(|c| c.etag.as_deref()))?;
        if response.status() == ureq::http::StatusCode::NOT_MODIFIED {
            let cached = cached.ok_or("Wolfx 地震詳細のキャッシュがありません")?;
            return Ok(CachedReports {
                etag: cached.etag.clone(),
                reports: Arc::clone(&cached.reports),
            });
        }
        let etag = response
            .headers()
            .get("ETag")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let packet: Value = response.body_mut().read_json()?;
        let events = packet["events"]
            .as_array()
            .ok_or("Wolfx 地震詳細のイベント一覧が不正です")?;
        let mut reports: Vec<_> = events.iter().filter_map(event_report).collect();
        reports.sort_by_key(|report| {
            std::cmp::Reverse(report.bulletin.origin.or(report.bulletin.issued))
        });
        Ok(CachedReports {
            etag,
            reports: reports.into(),
        })
    }

    pub fn details(&self) -> Result<Arc<[Report]>> {
        let mut cached = self
            .cached
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let next = self.detailed_reports(cached.as_ref())?;
        let reports = Arc::clone(&next.reports);
        *cached = Some(next);
        Ok(reports)
    }

    pub fn reports(&self) -> Result<Arc<[Report]>> {
        match self.details() {
            Ok(reports) if !reports.is_empty() => Ok(reports),
            _ => match live_snapshot(LiveQuery::Quakes)? {
                LiveUpdate::Quakes(reports) => Ok(reports),
                _ => Err("Wolfx の地震一覧の応答が不正です".into()),
            },
        }
    }

    pub fn eew(&self) -> Result<Option<Bulletin>> {
        match live_snapshot(LiveQuery::Eew)? {
            LiveUpdate::Eew(report) => Ok(Some(*report)),
            _ => Err("Wolfx の緊急地震速報の応答が不正です".into()),
        }
    }
}

pub fn set_demo_intensity(report: &mut Bulletin, maximum: u8) {
    report.maximum = Some(Level::Known(maximum));
    for (index, area) in report.forecast_areas.iter_mut().enumerate() {
        let upper = maximum.saturating_sub(u8::from(index >= 3));
        area.minimum = Some(Level::Known(upper.saturating_sub(1)));
        area.maximum = Some(Level::Known(upper));
    }
}

pub fn demo() -> Vec<Bulletin> {
    let origin = parse_time("2000-01-01T14:32:00+09:00");
    let bulletin = |label: &str, time: &str, lon, lat| Bulletin {
        name: "宮城県沖".into(),
        label: label.into(),
        origin,
        issued: parse_time(time),
        epicenter: Location::new(lon, lat),
        ..Bulletin::default()
    };
    let mut first = bulletin(
        "EEW 警報  第1報",
        "2000-01-01T14:32:07+09:00",
        142.49,
        38.29,
    );
    first.warning = true;
    first.eew = true;
    first.eew_serial = Some(1);
    first.magnitude = "5.8".into();
    first.depth = Some(50);
    first.warning_areas = ["宮城県北部", "宮城県中部", "宮城県南部"]
        .map(String::from)
        .into();
    let mut second = bulletin(
        "EEW 警報  第2報（最終報）",
        "2000-01-01T14:32:12+09:00",
        142.38,
        38.34,
    );
    second.warning = true;
    second.eew = true;
    second.eew_serial = Some(2);
    second.eew_final = true;
    second.magnitude = "6.1".into();
    second.depth = Some(45);
    second.warning_areas = first.warning_areas.clone();
    second.warning_areas.extend(
        [
            "岩手県内陸南部",
            "岩手県沿岸南部",
            "福島県浜通り",
            "福島県中通り",
        ]
        .map(String::from),
    );
    for (report, maximum) in [(&mut first, 4), (&mut second, 5)] {
        report.forecast_areas = report
            .warning_areas
            .iter()
            .cloned()
            .map(|name| ForecastArea {
                name,
                minimum: None,
                maximum: None,
            })
            .collect();
        set_demo_intensity(report, maximum);
    }
    let mut actual = bulletin("観測震度", "2000-01-01T14:34:00+09:00", 142.38, 38.34);
    actual.depth = Some(45);
    actual.magnitude = "6.1".into();
    actual.maximum = Some(Level::Known(4));
    actual.stations = [
        ("宮城県", "石巻", 141.30, 38.43, 4),
        ("宮城県", "南三陸", 141.45, 38.68, 4),
        ("宮城県", "気仙沼", 141.57, 38.91, 3),
        ("宮城県", "仙台", 140.87, 38.27, 3),
        ("岩手県", "大船渡", 141.71, 39.07, 3),
        ("岩手県", "一関", 141.13, 38.93, 2),
        ("福島県", "相馬", 140.92, 37.80, 2),
        ("岩手県", "釜石", 141.88, 39.27, 2),
        ("福島県", "福島", 140.47, 37.76, 1),
        ("福島県", "郡山", 140.38, 37.40, 1),
        ("岩手県", "盛岡", 141.16, 39.70, 1),
        ("山形県", "山形", 140.34, 38.25, 0),
        ("福島県", "いわき", 140.89, 37.05, 0),
        ("秋田県", "大館", 140.56, 40.27, 0),
    ]
    .into_iter()
    .map(|(prefecture, name, lon, lat, level)| Observation {
        name: name.into(),
        level: Level::Known(level),
        location: Location::new(lon, lat),
        area_code: String::new(),
        prefecture: Some(prefecture.into()),
    })
    .collect();
    actual.regions = [
        ("222", "宮城県中部", 4),
        ("220", "宮城県北部", 4),
        ("211", "岩手県沿岸南部", 3),
        ("213", "岩手県内陸南部", 2),
        ("251", "福島県浜通り", 2),
        ("250", "福島県中通り", 1),
        ("212", "岩手県内陸北部", 1),
        ("242", "山形県村山", 0),
        ("232", "秋田県内陸北部", 0),
    ]
    .into_iter()
    .map(|(code, name, level)| Observation {
        name: name.into(),
        level: Level::Known(level),
        location: None,
        area_code: code.into(),
        prefecture: None,
    })
    .collect();
    vec![first, second, actual]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wolfx_detail_keeps_one_observation_and_positions_named_stations() {
        let mut event = json!({
            "eventId": "20261116132858",
            "status": "reported",
            "hypocenters": [{"name":"静岡県東部","latitude":35.3,"longitude":139.0,"depth":280.0}],
            "reports": [
                {"product":"VXSE51","status":"issued","time":"2026-11-16T04:30:00Z",
                 "observation":{"maxIntensity":"4","areas":[{"code":"200","name":"別の区域","int":"4"}]}},
                {"product":"VXSE53","status":"issued","time":"2026-11-16T04:31:00Z","originTime":"2026-11-16T04:28:00Z",
                 "hypocenter":0,"magnitude":2.8,"observation":{
                    "maxIntensity":"5-","areas":[{"code":"361","name":"神奈川県西部","int":"5-"}],
                    "cities":[{"area":0,"stations":[{"code":"1420600","name":"小田原市久野＊","int":"5-"}]}]
                 }}
            ]
        });
        let bulletin = event_report(&event).unwrap().bulletin;
        assert_eq!(
            bulletin.issued.unwrap().format("%H:%M").to_string(),
            "13:31"
        );
        assert_eq!(bulletin.maximum.unwrap().label(), "5弱");
        assert_eq!(bulletin.depth, Some(280));
        assert_eq!(bulletin.regions.len(), 1);
        assert_eq!(bulletin.regions[0].area_code, "361");
        assert_eq!(bulletin.stations.len(), 1);
        assert!((bulletin.stations[0].location.unwrap().lon - 139.083333).abs() < 0.00001);

        event["reports"].as_array_mut().unwrap().push(json!({
            "product":"VXSE53","status":"cancelled","time":"2026-11-16T04:33:00Z"
        }));
        let bulletin = event_report(&event).unwrap().bulletin;
        assert_eq!(bulletin.maximum.unwrap().label(), "4");
        assert!(bulletin.stations.is_empty());
    }

    #[test]
    fn station_prefectures_follow_the_observation_indexes() {
        let report = event_report(&json!({
            "eventId": "test",
            "status": "reported",
            "reports": [{"product":"VXSE53", "status":"issued", "observation": {
                "prefs": [{"name":"岩手県"}, {"name":"宮城県"}],
                "areas": [
                    {"pref":0, "stations":[{"code":"1", "name":"大船渡", "int":"3"}]},
                    {"pref":1}
                ],
                "cities": [
                    {"area":0, "stations":[{"code":"1", "name":"大船渡", "int":"3"}]},
                    {"area":1, "stations":[{"code":"2", "name":"石巻", "int":"4"}]},
                    {"area":9, "stations":[{"code":"3", "name":"区域不明の観測点", "int":"2"}]}
                ]
            }}]
        }))
        .unwrap();
        let prefectures: Vec<_> = report
            .bulletin
            .stations
            .iter()
            .map(|station| (station.name.as_str(), station.prefecture.as_deref()))
            .collect();
        assert_eq!(
            prefectures,
            [
                ("石巻", Some("宮城県")),
                ("大船渡", Some("岩手県")),
                ("区域不明の観測点", None)
            ]
        );
    }

    #[test]
    fn websocket_dispatches_the_jma_sources_and_ignores_heartbeats() {
        assert!(live_update(&json!({"type":"heartbeat"}), None).is_none());
        let eew = json!({
            "type": "jma_eew", "EventID": "test",
            "AnnouncedTime": Utc::now().to_rfc3339(), "isWarn": true
        });
        assert!(matches!(live_update(&eew, None), Some(LiveUpdate::Eew(report)) if report.warning));
        let list = json!({"type":"jma_eqlist", "No1":{
            "EventID":"new", "time":"2026/11/22 10:00", "location":"宮城県沖"
        }});
        assert!(
            matches!(live_update(&list, None), Some(LiveUpdate::Quakes(reports)) if reports[0].id == "new")
        );
        let tsunami = json!({"type":"jma_tsunami","status":"unknown"});
        assert!(
            matches!(live_update(&tsunami, None), Some(LiveUpdate::Tsunami(report)) if !report.alert)
        );
    }

    #[test]
    fn pending_quakes_are_added_without_losing_matching_details() {
        let list = Arc::from(read_reports(&json!({
            "No1":{"EventID":"new", "time":"2026/11/22 10:01"},
            "No2":{"EventID":"known", "time":"2026/11/22 10:00"}
        })));
        let detail = Report {
            id: "known".into(),
            bulletin: Bulletin {
                origin: parse_time("2026/11/22 10:00"),
                comment: "震度の詳細".into(),
                ..Bulletin::default()
            },
        };
        let details = Arc::from([detail]);
        assert!(Arc::ptr_eq(&merge_reports(&details, &details), &details));
        let combined = merge_reports(&list, &details);
        assert_eq!(combined.len(), 2);
        assert_eq!(combined[0].id, "new");
        assert_eq!(combined[1].bulletin.comment, "震度の詳細");
    }

    #[test]
    fn tsunami_alert_validity_expires_and_untrusted_reports_stay_unconfirmed() {
        let mut packet = serde_json::json!({
            "status": "active",
            "trusted": true,
            "validUntil": "2026-11-18T00:05:00Z",
            "report": {"time": "2026-11-18T00:00:00Z"},
            "areas": [{
                "name": "岩手県",
                "level": "major_warning",
                "maxWave": {"height": 10, "text": "10m超"},
                "firstWave": {"time": "2026-11-18T00:10:00Z"}
            }]
        });
        let report = read_tsunami(&packet).unwrap();
        assert!(report.alert);
        assert!(report.current(parse_time("2026-11-18T00:04:59Z").unwrap().to_utc()));
        assert!(!report.current(parse_time("2026-11-18T00:05:00Z").unwrap().to_utc()));
        assert_eq!(
            report.areas[0],
            "大津波警報  岩手県  10m超  第1波 11/18 09:10 JST"
        );
        packet["trusted"] = false.into();
        assert!(
            !read_tsunami(&packet)
                .unwrap()
                .current(report.issued.unwrap().to_utc())
        );
    }

    #[test]
    fn event_notices_require_an_explicit_source() {
        let info = "この地震による津波の心配はありません。\nこの地震について、緊急地震速報を発表しています。";
        assert_eq!(eew_notice(info), Some(true));
        assert_eq!(tsunami_notice(info), Some("心配なし"));
        assert_eq!(tsunami_notice("津波の有無を調査中です。"), None);
        assert_eq!(eew_notice(""), None);
        let bulletin = read_reports(&json!({"No1":{
            "EventID":"20261116132858", "time":"2026/11/16 13:28", "info":info
        }}))
        .remove(0)
        .bulletin;
        assert_eq!(bulletin.eew_notice, Some(true));
        assert_eq!(bulletin.tsunami_notice, Some("心配なし"));

        let tsunami =
            read_tsunami(&json!({"status":"active","report":{"eventIds":["20261116132858"]}}))
                .unwrap();
        assert_eq!(tsunami.notice_for("20261116132858"), Some("警報等あり"));
        assert_eq!(tsunami.notice_for("20261116132859"), None);
    }

    #[test]
    fn wolfx_eew_distinguishes_warning_forecast_and_cancellation() {
        let mut packet = json!({
            "EventID":"20261116132858","AnnouncedTime":"2026/11/16 13:29:31","Serial":4,
            "isWarn":true,"Latitude":35.3,"Longitude":139.0,"Magnitude":3.5,"Depth":10,
            "WarnArea":[
                {"Chiiki":"静岡県東部","Type":"警報","Shindo1":"5強","Shindo2":"5弱"},
                {"Chiiki":"神奈川県西部","Type":"予報","Shindo1":"4","Shindo2":"///"},
                {"Chiiki":"神奈川県東部","Type":"予報","Shindo1":"不明"}
            ]
        });
        let bulletin = read_eew(&packet).unwrap();
        assert!(bulletin.warning);
        assert_eq!(bulletin.warning_areas, ["静岡県東部"]);
        assert_eq!(bulletin.forecast_areas.len(), 2);
        let area = &bulletin.forecast_areas[0];
        assert_eq!(area.minimum.as_ref().unwrap().label(), "5弱");
        assert_eq!(area.maximum.as_ref().unwrap().label(), "5強");
        assert!(bulletin.forecast_areas[1].minimum.is_none());
        assert_eq!(bulletin.eew_serial, Some(4));
        assert!(!bulletin.eew_final);
        packet["isWarn"] = json!(false);
        packet["isFinal"] = json!(true);
        packet["Serial"] = json!("4");
        let bulletin = read_eew(&packet).unwrap();
        assert_eq!(bulletin.eew_serial, Some(4));
        assert!(bulletin.eew_final);
        assert!(bulletin.label.starts_with("EEW 予報"));
        assert!(bulletin.warning_areas.is_empty());
        assert_eq!(bulletin.forecast_areas.len(), 2);
        packet["isCancel"] = json!(true);
        let cancelled = read_eew(&packet).unwrap();
        assert!(cancelled.epicenter.is_none());
        assert!(cancelled.forecast_areas.is_empty());
        packet["isTraining"] = json!(true);
        assert!(read_eew(&packet).is_none());
    }
}
