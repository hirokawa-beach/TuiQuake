use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use serde_json::Value;

const EARTHQUAKE_DETAIL: &str = "https://api.wolfx.jp/jma_earthquake_detail.json";
mod feeds;
mod p2p;
mod socket;
mod wolfx;
mod wolfx_details;

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

#[derive(Clone, Copy)]
pub struct ServerClock {
    pub utc: DateTime<Utc>,
    pub received: Instant,
}

impl ServerClock {
    pub fn at(self, now: Instant) -> DateTime<Utc> {
        let elapsed = now.saturating_duration_since(self.received);
        self.utc
            .checked_add_signed(chrono::TimeDelta::from_std(elapsed).unwrap_or_default())
            .unwrap_or(self.utc)
    }
}

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Wolfx,
    P2p,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wolfx => "Wolfx",
            Self::P2p => "P2P地震情報",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bulletin {
    pub provider: Provider,
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

fn station_location(name: &str) -> Option<Location> {
    static STATIONS: OnceLock<HashMap<&'static str, [f64; 2]>> = OnceLock::new();
    let locations = STATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("../stations.json")).expect("station locations")
    });
    let &[lon, lat] = locations.get(name.trim_end_matches(['＊', '*']).trim_end())?;
    Location::new(lon, lat)
}

impl Bulletin {
    pub fn has_observations(&self) -> bool {
        !self.stations.is_empty() || !self.regions.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveQuery {
    Eew,
    Quakes,
    Tsunami,
}

pub enum LiveUpdate {
    Connection(Provider, Option<String>),
    Clock(Provider, ServerClock),
    Eew(Box<Bulletin>),
    EewInvalid(String),
    Quakes(Arc<[Report]>),
    HistoryInvalid(String),
    Tsunami(Tsunami),
    TsunamiInvalid(String),
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
    pub provider: Provider,
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

struct CachedReports {
    etag: Option<String>,
    reports: Arc<[Report]>,
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    cached: Arc<Mutex<Option<CachedReports>>>,
}

pub trait Source: Send + Sync {
    fn details(&self) -> Result<Arc<[Report]>>;
    fn stream(
        &self,
        requests: mpsc::Receiver<LiveQuery>,
        notify: &mut dyn FnMut(LiveUpdate) -> bool,
    );
}

impl Source for Client {
    fn details(&self) -> Result<Arc<[Report]>> {
        Client::details(self)
    }

    fn stream(
        &self,
        requests: mpsc::Receiver<LiveQuery>,
        notify: &mut dyn FnMut(LiveUpdate) -> bool,
    ) {
        feeds::stream(self, requests, notify);
    }
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
        let reports = wolfx_details::read_details(response.body_mut().read_json()?)?;
        Ok(CachedReports { etag, reports })
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
        if let Ok(reports) = self.details()
            && !reports.is_empty()
        {
            return Ok(reports);
        }
        match wolfx::live_snapshot(LiveQuery::Quakes) {
            Ok(LiveUpdate::Quakes(reports)) if !reports.is_empty() => Ok(reports),
            _ => p2p::last_quakes(self),
        }
    }

    pub fn eew(&self) -> Result<Option<Bulletin>> {
        match wolfx::live_snapshot(LiveQuery::Eew) {
            Ok(LiveUpdate::Eew(report)) => Ok(Some(*report)),
            _ => p2p::last_eew(self),
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
    use super::wolfx::{live_update, read_eew, read_tsunami};
    use super::wolfx_details::{
        WolfxDetailEvent, WolfxDetailFeed, eew_notice, event_report, read_details, read_reports,
        tsunami_notice,
    };
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    fn event_from_json(value: &Value) -> Option<Report> {
        event_report(&WolfxDetailEvent::deserialize(value).unwrap()).unwrap()
    }

    #[test]
    fn recorded_wolfx_payloads_keep_the_eew_and_station_details() {
        let packet: Value =
            serde_json::from_str(include_str!("../tests/fixtures/wolfx-eew.json")).unwrap();
        let eew = read_eew(&packet).unwrap().unwrap();
        assert_eq!(eew.eew_serial, Some(4));
        assert_eq!(eew.name, "石垣島近海");
        assert_eq!(eew.maximum.unwrap().label(), "1");
        assert!(eew.eew_final);
        let feed =
            serde_json::from_str(include_str!("../tests/fixtures/wolfx-details.json")).unwrap();
        let history = read_details(feed).unwrap();
        assert_eq!(history.len(), 1);
        let bulletin = &history[0].bulletin;
        assert_eq!(bulletin.name, "トカラ列島近海");
        assert!(
            bulletin
                .stations
                .iter()
                .any(|station| station.prefecture.as_deref() == Some("鹿児島県"))
        );
    }

    #[test]
    fn malformed_live_eew_and_detail_schema_surface_errors() {
        let report = json!({ "type": "jma_eew", "EventID": 123, "AnnouncedTime": null });
        assert!(matches!(
            live_update(&report, None),
            Some(LiveUpdate::EewInvalid(_))
        ));
        let bad = json!({"events":[{"eventId":"test", "status":"reported", "reports":[{
            "product":"VXSE53", "status":"issued", "observation":{"areas":"unexpected"}
        }]}]});
        assert!(read_details(WolfxDetailFeed::deserialize(&bad).unwrap()).is_err());
    }

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
        let bulletin = event_from_json(&event).unwrap().bulletin;
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
        let bulletin = event_from_json(&event).unwrap().bulletin;
        assert_eq!(bulletin.maximum.unwrap().label(), "4");
        assert!(bulletin.stations.is_empty());
    }

    #[test]
    fn station_prefectures_follow_the_observation_indexes() {
        let report = event_from_json(&json!({
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
    fn tsunami_uses_server_time_and_expires_while_disconnected() {
        use crate::tsunami::{State, Validity};

        let report = read_tsunami(&json!({
            "status": "active",
            "trusted": true,
            "validUntil": "2026-11-22T10:21:00Z",
            "report": {"time": "2026-11-22T10:19:00Z"}
        }))
        .unwrap();
        let start = Instant::now();
        let mut tsunami = State::default();
        tsunami.receive(report, start);
        tsunami.update(start);
        assert!(tsunami.validity == Validity::AwaitingClock);
        let utc = parse_time("2026-11-22T10:20:00Z").unwrap().to_utc();
        tsunami.sync_wolfx(
            ServerClock {
                utc,
                received: start,
            },
            start,
        );
        tsunami.update(start + Duration::from_secs(59));
        assert!(tsunami.verified(Provider::Wolfx).unwrap().alert);
        let reference = p2p_tsunami_reference();
        tsunami.receive(reference, start + Duration::from_secs(59));
        assert!(tsunami.verified(Provider::P2p).is_none());
        let expired = start + Duration::from_secs(60);
        tsunami.update(expired);
        assert!(tsunami.validity == Validity::Expired);
        tsunami.sync_wolfx(
            ServerClock {
                utc,
                received: expired,
            },
            expired,
        );
        tsunami.update(expired);
        assert!(tsunami.verified(Provider::Wolfx).is_none());
    }

    fn p2p_tsunami_reference() -> Tsunami {
        Tsunami {
            provider: Provider::P2p,
            title: "津波の発表内容",
            alert: false,
            valid_until: None,
            issued: parse_time("2026-11-22T10:20:30Z"),
            areas: Vec::new(),
            event_ids: Vec::new(),
            event_notice: None,
        }
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
        let bulletin = read_eew(&packet).unwrap().unwrap();
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
        let bulletin = read_eew(&packet).unwrap().unwrap();
        assert_eq!(bulletin.eew_serial, Some(4));
        assert!(bulletin.eew_final);
        assert!(bulletin.label.starts_with("EEW 予報"));
        assert!(bulletin.warning_areas.is_empty());
        assert_eq!(bulletin.forecast_areas.len(), 2);
        packet["isCancel"] = json!(true);
        let cancelled = read_eew(&packet).unwrap().unwrap();
        assert!(cancelled.epicenter.is_none());
        assert!(cancelled.forecast_areas.is_empty());
        packet["isTraining"] = json!(true);
        assert!(read_eew(&packet).unwrap().is_none());
    }
}
