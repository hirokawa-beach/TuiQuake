use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use serde::Deserialize;
use serde_json::Value;

use super::{
    Bulletin, Level, Observation, Report, Result, Stamp, depth, epicenter, magnitude, parse_time,
    station_location, text,
};

fn report_time(report: &Value) -> Option<Stamp> {
    parse_time(text(&report["time"])).or_else(|| parse_time(text(&report["issuedAt"])))
}

pub(super) fn eew_notice(message: &str) -> Option<bool> {
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

pub(super) fn tsunami_notice(message: &str) -> Option<&'static str> {
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

#[derive(Deserialize)]
pub(super) struct WolfxDetailFeed {
    events: Vec<WolfxDetailEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WolfxDetailEvent {
    event_id: String,
    status: String,
    #[serde(default)]
    reports: Vec<Value>,
    #[serde(default)]
    hypocenters: Vec<Value>,
    summary: Option<Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WolfxObservation {
    max_intensity: Option<String>,
    max_long_period: Option<String>,
    prefs: Option<Vec<WolfxPrefecture>>,
    areas: Option<Vec<WolfxArea>>,
    cities: Option<Vec<WolfxCity>>,
}

#[derive(Deserialize)]
struct WolfxPrefecture {
    name: Option<String>,
}

#[derive(Deserialize)]
struct WolfxArea {
    name: Option<String>,
    code: Option<String>,
    int: Option<String>,
    pref: Option<usize>,
    stations: Option<Vec<WolfxStation>>,
}

#[derive(Deserialize)]
struct WolfxCity {
    area: Option<usize>,
    stations: Option<Vec<WolfxStation>>,
}

#[derive(Deserialize)]
struct WolfxStation {
    name: Option<String>,
    code: Option<String>,
    int: Option<String>,
    #[serde(default)]
    longitude: Value,
    #[serde(default)]
    latitude: Value,
}

fn observations(observed: &WolfxObservation) -> (Vec<Observation>, Vec<Observation>) {
    let prefs = observed.prefs.as_deref().unwrap_or_default();
    let areas = observed.areas.as_deref().unwrap_or_default();
    let cities = observed.cities.as_deref().unwrap_or_default();
    let mut regions: Vec<_> = areas
        .iter()
        .filter_map(|area| {
            Some(Observation {
                name: area.name.as_deref().unwrap_or_default().into(),
                level: area.int.as_deref().and_then(Level::parse)?,
                location: None,
                area_code: area.code.as_deref().unwrap_or_default().into(),
                prefecture: None,
            })
        })
        .collect();
    let station_sets = areas
        .iter()
        .map(|area| (Some(area), area.stations.as_deref()))
        .chain(cities.iter().map(|city| {
            (
                city.area.and_then(|i| areas.get(i)),
                city.stations.as_deref(),
            )
        }));
    let mut seen = HashSet::new();
    let mut stations = Vec::new();
    for (area, points) in station_sets {
        let prefecture = area
            .and_then(|area| area.pref)
            .and_then(|i| prefs.get(i))
            .and_then(|pref| pref.name.as_deref())
            .map(str::trim)
            .filter(|name| !name.is_empty());
        for station in points.into_iter().flatten() {
            let name = station.name.as_deref().unwrap_or_default();
            let code = station
                .code
                .as_deref()
                .filter(|code| !code.is_empty())
                .unwrap_or(name);
            let Some(level) = station.int.as_deref().and_then(Level::parse) else {
                continue;
            };
            if !name.is_empty() && seen.insert(code) {
                stations.push(Observation {
                    name: name.into(),
                    level,
                    location: epicenter(&station.longitude, &station.latitude)
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
    (regions, stations)
}

fn rank(item: &Value) -> (Option<Stamp>, Option<Stamp>, u8) {
    (
        report_time(item),
        parse_time(text(&item["issuedAt"])),
        match text(&item["status"]) {
            "cancelled" => 2,
            "corrected" => 1,
            _ => 0,
        },
    )
}

fn active_product_reports(reports: &[Value]) -> Vec<&Value> {
    let mut products = HashMap::<&str, &Value>::new();
    for report in reports {
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
    reports
        .iter()
        .filter(|report| {
            matches!(text(&report["status"]), "issued" | "corrected")
                && products.contains_key(text(&report["product"]))
        })
        .collect()
}

pub(super) fn event_report(event: &WolfxDetailEvent) -> Result<Option<Report>> {
    if event.event_id.is_empty() {
        return Err("地震詳細の eventId が空です".into());
    }
    if event.status == "cancelled" {
        return Ok(None);
    }
    let active = active_product_reports(&event.reports);
    let active_reports = || active.iter().copied();
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
    let summary = event.summary.as_ref().filter(|value| value.is_object());
    let Some(source) = observation_report.or(hypocenter_report).or_else(|| {
        if event.status == "preliminary" {
            summary
        } else {
            None
        }
    }) else {
        return Ok(None);
    };
    let hypocenter_source = if source["hypocenter"].is_u64() {
        source
    } else {
        hypocenter_report.unwrap_or(source)
    };
    let hypocenter = hypocenter_source["hypocenter"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| event.hypocenters.get(index))
        .unwrap_or(&Value::Null);
    let observed = if source["observation"].is_null() {
        WolfxObservation::default()
    } else {
        WolfxObservation::deserialize(&source["observation"])?
    };
    let (regions, stations) = observations(&observed);
    let comments = &source["comments"];
    let mut comment: Vec<_> = ["forecast", "warning", "note", "text"]
        .into_iter()
        .map(|key| text(&comments[key]))
        .filter(|value| !value.is_empty())
        .collect();
    if !text(&source["text"]).is_empty() {
        comment.push(text(&source["text"]));
    }
    if event.status == "preliminary" {
        comment.push("Wolfx の暫定概要です。");
    }
    let mut comment = comment.join("\n");
    let period = observed.max_long_period.as_deref().unwrap_or_default();
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
        maximum: observed
            .max_intensity
            .as_deref()
            .and_then(Level::parse)
            .or_else(|| Level::parse(text(&source["maxIntensity"]))),
        regions,
        stations,
        comment,
        ..Bulletin::default()
    };
    Ok(Some(Report {
        id: event.event_id.clone(),
        bulletin,
    }))
}

pub(super) fn read_details(feed: WolfxDetailFeed) -> Result<Arc<[Report]>> {
    let mut reports = Vec::new();
    for event in &feed.events {
        let report = event_report(event)
            .map_err(|error| format!("地震詳細 {} · {error}", event.event_id))?;
        reports.extend(report);
    }
    reports
        .sort_by_key(|report| std::cmp::Reverse(report.bulletin.origin.or(report.bulletin.issued)));
    Ok(reports.into())
}

pub(super) fn read_reports(packet: &Value) -> Vec<Report> {
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
