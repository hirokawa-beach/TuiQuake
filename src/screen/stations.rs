use std::collections::HashMap;

use super::{panels::eew_badge, *};

const MARK_NOTE: &str = "＊ 気象庁以外の観測点";
const MARK_INK: Rgb = (148, 148, 148);

pub(super) enum ReadingLine {
    Text(String, Rgb),
    StationNames(String, Rgb),
    Intensity {
        label: String,
        count: usize,
        unit: &'static str,
        background: Option<Rgb>,
    },
}

impl From<(String, Rgb)> for ReadingLine {
    fn from((text, ink): (String, Rgb)) -> Self {
        Self::Text(text, ink)
    }
}

impl ReadingLine {
    pub(super) fn render(&self, width: usize, ansi: bool) -> String {
        match self {
            Self::Text(text, ink) => color(&fit(text, width), *ink, ansi),
            Self::StationNames(text, ink) => {
                let visible = fit(text, width);
                if !ansi {
                    return visible;
                }
                let mut line = String::new();
                for part in visible.split_inclusive(['＊', '*']) {
                    if let Some(name) = part.strip_suffix(['＊', '*']) {
                        let symbol = &part[name.len()..];
                        if !name.is_empty() {
                            line.push_str(&color(name, *ink, true));
                        }
                        line.push_str(&color(symbol, MARK_INK, true));
                    } else {
                        line.push_str(&color(part, *ink, true));
                    }
                }
                line
            }
            Self::Intensity {
                label,
                count,
                unit,
                background,
            } => {
                let badge = fit(&format!(" {label} "), width.saturating_sub(1));
                let count = fit(
                    &format!("  {count}{unit}"),
                    width.saturating_sub(1 + badge.width()),
                );
                format!(
                    " {}{}",
                    eew_badge(&badge, *background, ansi),
                    color(&count, MUTED, ansi)
                )
            }
        }
    }
}

pub(super) fn intensity_heading(
    label: &str,
    level: &Level,
    count: usize,
    unit: &'static str,
) -> ReadingLine {
    let background = level.index().map(|_| reading_rgb(level));
    let label = if background.is_some() {
        format!("震度 {label}")
    } else {
        label.into()
    };
    ReadingLine::Intensity {
        label,
        count,
        unit,
        background,
    }
}

fn station_mark(station: &Observation) -> bool {
    station.name.trim_end().ends_with(['＊', '*'])
}

fn station_name(station: &Observation, show_city: bool, width: usize) -> String {
    let marked = station_mark(station);
    let name = station.name.trim().trim_end_matches(['＊', '*']).trim_end();
    let short = station
        .municipality
        .as_deref()
        .filter(|_| show_city)
        .and_then(|city| name.strip_prefix(city))
        .map(str::trim_start)
        .filter(|tail| !tail.is_empty())
        .unwrap_or(name);
    let mark = if marked { "＊" } else { "" };
    let width = width.saturating_sub(mark.width());
    let text = if short.width() > width && width > 1 {
        format!("{}…", fit(short, width - 1))
    } else {
        fit(short, width)
    };
    format!("{text}{mark}")
}

struct StationRow<'a> {
    city: &'a str,
    city_index: usize,
    names: Vec<String>,
}

fn pack<'a>(
    stations: &[&'a Observation],
    city_width: usize,
    names_width: usize,
) -> Vec<StationRow<'a>> {
    let mut cities: Vec<(Option<&str>, Vec<&Observation>)> = Vec::new();
    let mut city_indices = HashMap::new();
    for &station in stations {
        let key = (
            station.municipality.as_deref(),
            station
                .prefecture
                .as_deref()
                .filter(|_| station.municipality.is_some()),
        );
        let next = cities.len();
        let index = *city_indices.entry(key).or_insert(next);
        if index == next {
            cities.push((key.0, vec![station]));
        } else {
            cities[index].1.push(station);
        }
    }
    let mut rows = Vec::new();
    for (city_index, (city, points)) in cities.into_iter().enumerate() {
        let city = if city_width == 0 {
            ""
        } else {
            city.unwrap_or("—")
        };
        let mut names = Vec::new();
        let mut used = 0;
        for point in points {
            let name = station_name(point, city_width > 0, names_width);
            let width = name.width();
            if !names.is_empty() && used + 2 + width > names_width {
                rows.push(StationRow {
                    city,
                    city_index,
                    names: std::mem::take(&mut names),
                });
                used = 0;
            }
            if !names.is_empty() {
                used += 2;
            }
            used += width;
            names.push(name);
        }
        rows.push(StationRow {
            city,
            city_index,
            names,
        });
    }
    rows
}

pub(super) fn intensity_readings(
    bulletin: &Bulletin,
    width: usize,
    start: usize,
    slots: usize,
) -> (usize, usize, Vec<ReadingLine>) {
    let groups = observation_groups(&bulletin.stations, StationGrouping::Intensity);
    let city_width = bulletin
        .stations
        .iter()
        .filter_map(|station| station.municipality.as_deref())
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .min(width.saturating_sub(12));
    let city_padding = usize::from(city_width > 0) * 2;
    let gap = " ".repeat(city_padding);
    let names_width = width.saturating_sub(2 + city_width + city_padding);
    let marked = bulletin.stations.iter().any(station_mark) && slots >= 3;
    let available = slots - usize::from(marked);
    let groups: Vec<_> = groups
        .into_iter()
        .map(|(label, points)| {
            let count = points.len();
            let heading = intensity_heading(label, &points[0].level, count, "地点");
            let rows = pack(&points, city_width, names_width);
            (heading, count, rows)
        })
        .collect();
    let total = bulletin.stations.len();
    let start = if groups
        .iter()
        .map(|(_, _, rows)| rows.len() + 1)
        .sum::<usize>()
        <= available
    {
        0
    } else {
        start.min(total.saturating_sub(1))
    };
    let mut skip = start;
    let mut shown = 0;
    let mut lines = Vec::new();
    for (heading, count, rows) in groups {
        if skip >= count {
            skip -= count;
            continue;
        }
        if available - lines.len() < 2 {
            break;
        }
        lines.push(heading);
        let mut previous_city = None;
        for row in rows {
            if skip >= row.names.len() {
                skip -= row.names.len();
                continue;
            }
            if lines.len() == available {
                break;
            }
            let city = if previous_city == Some(row.city_index) {
                ""
            } else {
                row.city
            };
            let names = &row.names[skip..];
            let prefix = pad(city, city_width);
            lines.push(ReadingLine::StationNames(
                format!("  {prefix}{gap}{}", names.join("  ")),
                INK,
            ));
            shown += names.len();
            previous_city = Some(row.city_index);
            skip = 0;
        }
    }
    if marked && shown != 0 {
        lines.push(ReadingLine::StationNames(MARK_NOTE.into(), MUTED));
    }
    (start, shown, lines)
}
