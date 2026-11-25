use std::sync::Arc;

use serde_json::Value;

use super::{
    Bulletin, Client, Report, Result, Tsunami, entries, parse_time, text, wolfx, wolfx_details,
};

#[derive(Clone, Copy)]
enum Kind {
    Quake,
    Eew,
    Tsunami,
}

#[derive(Clone, Copy)]
pub struct Example {
    pub label: &'static str,
    kind: Kind,
    file: &'static str,
}

impl Example {
    const fn new(label: &'static str, kind: Kind, file: &'static str) -> Self {
        Self { label, kind, file }
    }

    pub fn category(self) -> &'static str {
        match self.kind {
            Kind::Quake => "地震",
            Kind::Eew => "EEW",
            Kind::Tsunami => "津波",
        }
    }

    pub fn load(self, client: &Client) -> Result<Sample> {
        let json: Value = match self.kind {
            Kind::Quake => {
                serde_json::from_str(include_str!("../../tests/fixtures/wolfx-details.json"))?
            }
            Kind::Eew => self.read_remote(client, "jma_eew")?,
            Kind::Tsunami => self.read_remote(client, "jma_tsunami")?,
        };
        let data = match self.kind {
            Kind::Quake => {
                let reports = wolfx_details::read_details(serde_json::from_value(json.clone())?)?;
                if reports.is_empty() {
                    return Err("表示できる地震情報がありません".into());
                }
                SampleData::Quakes(reports)
            }
            Kind::Eew => SampleData::Eew(Box::new(
                wolfx::read_eew(&json)?.ok_or("表示できるEEWがありません")?,
            )),
            Kind::Tsunami => SampleData::Tsunami {
                report: wolfx::read_tsunami(&json)?,
                observations: tsunami_observations(&json),
            },
        };
        Ok(Sample { data, json })
    }

    fn read_remote(self, client: &Client, folder: &str) -> Result<Value> {
        let url = format!("https://wolfx.jp/examples/{folder}/{}.json", self.file);
        Ok(client
            .request(&url, None)?
            .body_mut()
            .with_config()
            .limit(512 * 1024)
            .read_json()?)
    }
}

fn wave_measurements(item: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(first) = parse_time(text(&item["firstWave"]["time"])) {
        parts.push(format!("第1波 {} JST", first.format("%H:%M")));
    }
    let peak = &item["maxWave"];
    let height = text(&peak["text"]);
    let condition = text(&peak["condition"]);
    if !height.is_empty() {
        parts.push(format!("最大波 {height}"));
    } else if let Some(height) = peak["height"].as_f64() {
        parts.push(format!("最大波 {height} m"));
    } else if !condition.is_empty() {
        parts.push(condition.to_owned());
    }
    if let Some(time) = parse_time(text(&peak["time"])) {
        parts.push(time.format("%H:%M JST").to_string());
    }
    parts.join("  ")
}

fn tsunami_observations(json: &Value) -> Vec<String> {
    let mut rows = Vec::new();
    for report in entries(&json["observations"]) {
        let title = text(&report["title"]);
        if !title.is_empty() {
            rows.push(title.into());
        }
        for area in entries(&report["observed"]) {
            for station in entries(&area["stations"]) {
                let name = text(&station["name"]);
                if !name.is_empty() {
                    rows.push(format!("観測 · {name}  {}", wave_measurements(station)));
                }
            }
        }
        for area in entries(&report["estimated"]) {
            let name = text(&area["name"]);
            if !name.is_empty() {
                rows.push(format!("沿岸推定 · {name}  {}", wave_measurements(area)));
            }
        }
    }
    rows
}

pub struct Sample {
    pub data: SampleData,
    pub json: Value,
}

pub enum SampleData {
    Quakes(Arc<[Report]>),
    Eew(Box<Bulletin>),
    Tsunami {
        report: Tsunami,
        observations: Vec<String>,
    },
}

pub const EXAMPLES: [Example; 19] = [
    Example::new("地震詳細・観測点震度", Kind::Quake, ""),
    Example::new("第1報（予報）", Kind::Eew, "forecast_first"),
    Example::new("続報（予報）", Kind::Eew, "forecast_update"),
    Example::new("最終報", Kind::Eew, "forecast_final"),
    Example::new("緊急地震速報（警報）", Kind::Eew, "warning"),
    Example::new("レベル法", Kind::Eew, "level_method"),
    Example::new("PLUM法を含む予報", Kind::Eew, "plum_forecast"),
    Example::new("PLUM法を含む警報", Kind::Eew, "plum_warning"),
    Example::new("取消報", Kind::Eew, "cancelled"),
    Example::new("仮定震源要素", Kind::Eew, "assumed_hypocenter"),
    Example::new("海域の地震", Kind::Eew, "sea_earthquake"),
    Example::new("津波予報", Kind::Tsunami, "forecast_only"),
    Example::new("津波注意報", Kind::Tsunami, "advisory"),
    Example::new("津波警報", Kind::Tsunami, "warning"),
    Example::new("大津波警報", Kind::Tsunami, "major_warning"),
    Example::new("取消", Kind::Tsunami, "cancelled"),
    Example::new("解除", Kind::Tsunami, "cleared"),
    Example::new("有効期限切れ", Kind::Tsunami, "expired"),
    Example::new("津波観測情報を含む例", Kind::Tsunami, "with_observations"),
];
