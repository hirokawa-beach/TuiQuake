use std::fmt::Write;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    data::{Bulletin, ForecastArea, INTENSITIES, Level, Observation, Report, Result},
    graphics::{self, Image, Map},
    map::{
        self, Atlas, Color, Layer, PALETTE, Raster, Rgb, TEXT_CELL_HEIGHT, TEXT_CELL_WIDTH, View,
    },
};

use crate::settings::StationGrouping;

pub const MIN_SPLIT_WIDTH: usize = 58;
pub const APP_NAME: &str = "TuiQuake";
const MAP_MAX_COLUMNS: usize = 44;
const SIDEBAR_GAP: usize = 3;
const INK: map::Rgb = (220, 239, 237);
const MUTED: map::Rgb = (136, 173, 182);
const ACCENT: map::Rgb = (125, 213, 210);
const ALERT: map::Rgb = (255, 175, 97);
const FORECAST_INK: Rgb = (255, 222, 94);
const WARNING_INK: Rgb = (255, 97, 98);
const FRAME: map::Rgb = (72, 139, 148);
const FRAME_ALERT: map::Rgb = (177, 125, 79);
const PANEL_GAP: usize = 1;
const TEXT_FRAME_MARGIN: usize = 4;
const SELECTED: map::Rgb = (37, 70, 87);
const RESET: &str = "\x1b[0m";

// Rendered with the FIGlet Slant font by Glenn Chappell.
const LOGO: [&str; 5] = [
    r#"  ______      _ ____              __"#,
    r#" /_  __/_  __(_) __ \__  ______ _/ /_____"#,
    r#"  / / / / / / / / / / / / / __ `/ //_/ _ \"#,
    r#" / / / /_/ / / /_/ / /_/ / /_/ / ,< /  __/"#,
    r#"/_/  \__,_/_/\___\_\__,_/\__,_/_/|_|\___/"#,
];
// The split column follows the slant between Tui and Quake on each row.
const LOGO_STYLE: [(usize, Rgb, Rgb); LOGO.len()] = [
    (16, (153, 237, 241), (255, 235, 194)),
    (16, (116, 224, 231), (255, 219, 156)),
    (15, (87, 210, 224), (251, 199, 124)),
    (14, (65, 185, 203), (240, 178, 106)),
    (13, (47, 155, 178), (216, 155, 83)),
];

#[derive(Clone, Copy)]
pub struct Display {
    pub columns: usize,
    pub rows: usize,
    pub colors: bool,
    pub map: Map,
}

impl Display {
    pub fn width(self) -> usize {
        self.columns.saturating_sub(1).max(12)
    }
}

#[derive(Default)]
pub struct Frame {
    pub text: String,
    pub graphic: String,
    pub map_row: usize,
    pub map_col: usize,
    pub map_view: Option<View>,
}

pub struct History<'a> {
    pub reports: &'a [Report],
    pub selected: usize,
    pub status: &'a str,
    pub row_limit: usize,
}

#[derive(Clone, Copy, Default)]
pub struct QuakeNotices {
    pub eew: Option<bool>,
    pub tsunami: Option<&'static str>,
}

pub struct Scene<'a> {
    pub bulletin: &'a Bulletin,
    pub banner: &'a str,
    pub eew_source: Option<&'a str>,
    pub series: &'a [Bulletin],
    pub view: View,
    pub layer: Layer,
    pub station_grouping: StationGrouping,
    pub controls: &'a [String],
    pub history: Option<History<'a>>,
    pub menu: &'a [String],
    pub reading_start: usize,
    pub quake_notices: QuakeNotices,
}

impl<'a> Scene<'a> {
    pub fn single(bulletin: &'a Bulletin, national: bool, banner: &'a str) -> Self {
        Self {
            bulletin,
            banner,
            eew_source: None,
            series: &[],
            view: View::new(national),
            layer: Layer::Stations,
            station_grouping: StationGrouping::default(),
            controls: &[],
            history: None,
            menu: &[],
            reading_start: 0,
            quake_notices: QuakeNotices::default(),
        }
    }
}

pub fn fit(value: &str, mut width: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take_while(|c| {
            let n = c.width().unwrap_or(0);
            if n > width {
                false
            } else {
                width -= n;
                true
            }
        })
        .collect()
}

fn pad(value: &str, width: usize) -> String {
    let mut value = fit(value, width);
    value.extend(std::iter::repeat_n(
        ' ',
        width.saturating_sub(value.width()),
    ));
    value
}

fn color(value: &str, rgb: map::Rgb, ansi: bool) -> String {
    if !ansi {
        return value.to_owned();
    }
    let mut out = String::new();
    map::rgb(&mut out, rgb, false);
    out.push_str(value);
    out.push_str(RESET);
    out
}

#[derive(Clone, Copy)]
struct Border {
    width: usize,
    rgb: Rgb,
    ansi: bool,
}

impl Border {
    fn new(width: usize, rgb: Rgb, ansi: bool) -> Self {
        Self { width, rgb, ansi }
    }

    fn edge(self) -> String {
        color("│", self.rgb, self.ansi)
    }

    fn top(self, title: &str) -> String {
        let title = fit(title, self.width.saturating_sub(6));
        let rest = "─".repeat(self.width - 5 - title.width());
        let title = color(
            &title,
            if self.rgb == FRAME_ALERT {
                ALERT
            } else {
                ACCENT
            },
            self.ansi,
        );
        format!(
            "{}{title}{}",
            color("╭─ ", self.rgb, self.ansi),
            color(&format!(" {rest}╮"), self.rgb, self.ansi),
        )
    }

    fn bottom(self) -> String {
        color(
            &format!("╰{}╯", "─".repeat(self.width - 2)),
            self.rgb,
            self.ansi,
        )
    }

    // Panel content may already include SGR color sequences from the renderers.
    fn text(self, lines: Vec<String>, title: &str, height: usize) -> Vec<String> {
        let row = self.edge();
        let inner = self.width - TEXT_FRAME_MARGIN;
        let mut panel = Vec::with_capacity(height);
        panel.push(self.top(title));
        let empty = String::new();
        for line in lines
            .iter()
            .chain(std::iter::repeat(&empty))
            .take(height - 2)
        {
            let padding = " ".repeat(inner.saturating_sub(styled_width(line)));
            panel.push(format!("{row} {line}{padding} {row}"));
        }
        panel.push(self.bottom());
        panel
    }
}

fn styled_width(line: &str) -> usize {
    let mut parts = line.split('');
    parts.next().unwrap_or_default().width()
        + parts
            .filter_map(|part| part.split_once('m'))
            .map(|(_, text)| text.width())
            .sum::<usize>()
}

fn magnitude_and_depth(bulletin: &Bulletin) -> String {
    let mut facts = Vec::new();
    if !bulletin.magnitude.is_empty() {
        facts.push(format!("M{}", bulletin.magnitude));
    }
    if let Some(depth) = bulletin.depth {
        facts.push(format!("深さ {depth} km"));
    }
    facts.join("  ·  ")
}

fn times(b: &Bulletin, width: usize) -> Vec<String> {
    let same_day = b
        .origin
        .zip(b.issued)
        .is_some_and(|(origin, issued)| origin.date_naive() == issued.date_naive());
    let mut stamps = [("発生", b.origin), ("発表", b.issued)]
        .into_iter()
        .filter_map(|(label, stamp)| stamp.map(|stamp| (label, stamp)));
    let full = |(label, stamp): (&str, crate::data::Stamp)| {
        format!("{label} {} JST", stamp.format("%m/%d %H:%M:%S"))
    };
    let Some(first) = stamps.next() else {
        return Vec::new();
    };
    let Some(second) = stamps.next() else {
        return vec![full(first)];
    };
    let format = if same_day {
        "%H:%M:%S"
    } else {
        "%m/%d %H:%M:%S"
    };
    let combined = format!(
        "{} {}   {} {} JST",
        first.0,
        first.1.format("%m/%d %H:%M:%S"),
        second.0,
        second.1.format(format)
    );
    if combined.width() <= width {
        vec![combined]
    } else {
        vec![full(first), full(second)]
    }
}

fn event_header(b: &Bulletin, context: &str, width: usize, ansi: bool) -> Vec<String> {
    let mut status = color(&fit(APP_NAME, width), ACCENT, ansi);
    if !context.is_empty() {
        let context = fit(context, width.saturating_sub(APP_NAME.width() + 2));
        if !context.is_empty() {
            status.push_str("  ");
            status.push_str(&color(
                &context,
                if b.warning { ALERT } else { MUTED },
                ansi,
            ));
        }
    }
    let mut header = vec![status];
    header.extend(event_facts(b, width, ansi));
    header
}

fn short_intensity_label(level: &Level) -> &str {
    level.index().map_or_else(
        || level.label(),
        |index| map::SHORT_INTENSITIES[index as usize],
    )
}

fn event_facts(b: &Bulletin, width: usize, ansi: bool) -> Vec<String> {
    let name = if b.name.is_empty() { &b.label } else { &b.name };
    let mut header = Vec::new();
    if let Some(level) = &b.maximum {
        let label = if b.eew {
            "推定最大"
        } else {
            "最大震度"
        };
        let value = if b.eew {
            short_intensity_label(level)
        } else {
            level.label()
        };
        let mut maximum = format!("{label} {value}");
        if maximum.width() > width {
            let compact = if b.eew { "予測" } else { "震度" };
            maximum = format!("{compact} {value}");
        }
        let ink = level
            .index()
            .map_or(INK, |i| PALETTE[(map::STATION + i) as usize]);
        let maximum_width = maximum.width();
        let maximum = color(&fit(&maximum, width), ink, ansi);
        if name.width() + maximum_width + 3 <= width {
            header.push(format!(
                "{}   {maximum}",
                color(&fit(name, width), INK, ansi)
            ));
        } else {
            header.push(color(&fit(name, width), INK, ansi));
            header.push(maximum);
        }
    } else {
        header.push(color(&fit(name, width), INK, ansi));
    }
    let facts = magnitude_and_depth(b);
    if !facts.is_empty() {
        header.push(color(&fit(&facts, width), MUTED, ansi));
    }
    header.extend(
        times(b, width)
            .into_iter()
            .map(|line| color(&fit(&line, width), MUTED, ansi)),
    );
    header
}

fn observation_label(layer: Layer) -> &'static str {
    match layer {
        Layer::Regions => "広域・最大観測震度",
        Layer::Stations => "各観測点の震度",
    }
}

fn reading_rgb(level: &Level) -> Rgb {
    level
        .index()
        .map_or(INK, |i| PALETTE[(map::STATION + i) as usize])
}

fn reading_row(item: &Observation) -> (String, Rgb) {
    let label = short_intensity_label(&item.level);
    let (level, note) = if label.width() <= 2 {
        (label, String::new())
    } else {
        ("—", format!("  （{label}）"))
    };
    let inset = " ".repeat(2 - level.width());
    (
        format!("{inset}{level}  {}{note}", item.name),
        reading_rgb(&item.level),
    )
}

fn station_groups(b: &Bulletin, grouping: StationGrouping) -> Vec<(&str, Vec<&Observation>)> {
    const UNKNOWN: &str = "都道府県不明";
    let mut groups: Vec<(&str, Vec<&Observation>)> = Vec::new();
    // Observation order is already strongest first, including within groups.
    for station in &b.stations {
        let label = match grouping {
            StationGrouping::Prefecture => station.prefecture.as_deref().unwrap_or(UNKNOWN),
            StationGrouping::Intensity => short_intensity_label(&station.level),
        };
        if let Some((_, stations)) = groups.iter_mut().find(|(name, _)| *name == label) {
            stations.push(station);
        } else {
            groups.push((label, vec![station]));
        }
    }
    if grouping == StationGrouping::Prefecture {
        groups.sort_by_key(|(label, _)| *label == UNKNOWN);
    }
    groups
}

fn station_heading(label: &str, first: &Observation, grouping: StationGrouping) -> (String, Rgb) {
    match grouping {
        StationGrouping::Prefecture => (format!("── {label}"), ACCENT),
        StationGrouping::Intensity if first.level.index().is_some() => {
            (format!("── 震度 {label}"), reading_rgb(&first.level))
        }
        StationGrouping::Intensity => (format!("── {label}"), MUTED),
    }
}

fn station_reading_row(station: &Observation, grouping: StationGrouping) -> (String, Rgb) {
    match grouping {
        StationGrouping::Prefecture => reading_row(station),
        StationGrouping::Intensity => (station.name.clone(), INK),
    }
}

fn readings(b: &Bulletin, layer: Layer, grouping: StationGrouping) -> Vec<(String, Rgb)> {
    match layer {
        Layer::Regions => b.regions.iter().map(reading_row).collect(),
        Layer::Stations => station_groups(b, grouping)
            .into_iter()
            .flat_map(|(label, stations)| {
                std::iter::once(station_heading(label, stations[0], grouping)).chain(
                    stations
                        .into_iter()
                        .map(|station| station_reading_row(station, grouping)),
                )
            })
            .collect(),
    }
}

fn station_readings_window(
    b: &Bulletin,
    grouping: StationGrouping,
    start: usize,
    slots: usize,
) -> (String, Vec<(String, Rgb)>) {
    let count = b.stations.len();
    let groups = station_groups(b, grouping);
    let start = if count + groups.len() <= slots {
        0
    } else {
        start.min(count.saturating_sub(1))
    };
    let mut skip = start;
    let mut title = match grouping {
        StationGrouping::Prefecture => "各地の震度 · 観測点",
        StationGrouping::Intensity => "観測点 · 震度別",
    }
    .to_owned();
    let mut rows = Vec::new();
    let mut shown = 0;
    for (label, stations) in groups {
        if skip >= stations.len() {
            skip -= stations.len();
            continue;
        }
        if slots - rows.len() < 2 {
            break;
        }
        rows.push(station_heading(label, stations[skip], grouping));
        let visible = stations.into_iter().skip(skip).take(slots - rows.len());
        shown += visible.len();
        rows.extend(visible.map(|station| station_reading_row(station, grouping)));
        skip = 0;
    }
    if shown < count {
        write!(title, "  {}–{}/{count}", start + 1, start + shown).unwrap();
    }
    (title, rows)
}

impl History<'_> {
    fn title(&self) -> String {
        if self.reports.is_empty() {
            "地震履歴".into()
        } else {
            format!("地震履歴  {}/{}", self.selected + 1, self.reports.len())
        }
    }

    fn target_height(&self) -> usize {
        self.row_limit + 3
    }

    fn frame(self, border: Border, height: usize) -> Vec<String> {
        let height = height.min(self.target_height());
        let mut title = self.title();
        if height == 3 && !self.reports.is_empty() && !self.status.is_empty() {
            write!(title, " · {}", self.status).unwrap();
        }
        let lines = history_lines(
            self,
            border.width - TEXT_FRAME_MARGIN,
            height - 2,
            border.ansi,
        );
        border.text(lines, &title, height)
    }

    fn bottom_frame(self, border: Border, height: usize) -> Vec<String> {
        let panel = self.frame(border, height);
        std::iter::repeat_with(String::new)
            .take(height - panel.len())
            .chain(panel)
            .collect()
    }
}

fn history_lines(history: History<'_>, width: usize, rows: usize, ansi: bool) -> Vec<String> {
    let History {
        reports,
        selected,
        status,
        row_limit,
    } = history;
    let mut out = Vec::new();
    if !status.is_empty() && (rows > 1 || reports.is_empty()) {
        out.push(color(&fit(status, width), MUTED, ansi));
    }
    if reports.is_empty() {
        return out;
    }
    let date_width = if width >= 29 { 11 } else { 5 };
    let show_mag = width >= 40;
    let header = format!(
        "  {} 震度 {}震源地",
        pad(
            if date_width == 11 {
                "発生 JST"
            } else {
                "発生"
            },
            date_width
        ),
        if show_mag { "M     " } else { "" }
    );
    if rows > out.len() + 1 {
        out.push(color(&fit(&header, width), MUTED, ansi));
    }
    let count = rows.saturating_sub(out.len()).min(row_limit);
    let start = selected
        .saturating_sub(count / 2)
        .min(reports.len().saturating_sub(count));
    for (index, report) in reports.iter().enumerate().skip(start).take(count) {
        let item = &report.bulletin;
        let date = item
            .origin
            .map(|t| {
                t.format(if date_width == 11 {
                    "%m/%d %H:%M"
                } else {
                    "%H:%M"
                })
                .to_string()
            })
            .unwrap_or_default();
        let magnitude = if show_mag {
            pad(&item.magnitude, 6)
        } else {
            String::new()
        };
        let level = item.maximum.as_ref();
        let selected = index == selected;
        let prefix = format!(
            "{} {} ",
            if selected { '>' } else { ' ' },
            pad(&date, date_width),
        );
        let intensity = fit(level.map_or("—", short_intensity_label), 4);
        let left = (4 - intensity.width()) / 2;
        let cell = pad(&format!("{}{intensity}", " ".repeat(left)), 4);
        let suffix = format!(
            " {magnitude}{}",
            if item.name.is_empty() {
                "震源地不明"
            } else {
                &item.name
            },
        );
        if !ansi {
            out.push(fit(&format!("{prefix}{cell}{suffix}"), width));
            continue;
        }

        let mut line = String::new();
        if selected {
            map::rgb(&mut line, SELECTED, true);
            map::rgb(&mut line, INK, false);
        }
        line.push_str(&fit(&prefix, width));
        let cell_width = width.saturating_sub(prefix.width());
        if let Some(index) = level
            .and_then(|level| level.index())
            .filter(|_| cell_width > 0)
        {
            let background = PALETTE[(map::STATION + index) as usize];
            map::rgb(&mut line, background, true);
            map::rgb(&mut line, badge_ink(background), false);
            line.push_str(&fit(&cell, cell_width));
            line.push_str(RESET);
            if selected {
                map::rgb(&mut line, SELECTED, true);
                map::rgb(&mut line, INK, false);
            }
        } else {
            line.push_str(&fit(&cell, cell_width));
        }
        let suffix_width = cell_width.saturating_sub(cell.width());
        if selected {
            line.push_str(&pad(&suffix, suffix_width));
            line.push_str(RESET);
        } else {
            line.push_str(&fit(&suffix, suffix_width));
        }
        out.push(line);
    }
    out
}

fn badge_ink((r, g, b): Rgb) -> Rgb {
    let linear = |channel| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    if luminance > 0.179 {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    }
}

pub fn control_lines(heading: &str, bindings: &[(&str, &str)], display: Display) -> Vec<String> {
    let width = display.width();
    let heading = format!("{heading}  ");
    let indent = heading.width();
    let mut line = color(&heading, MUTED, display.colors);
    let mut used = indent;
    let mut has_item = false;
    let mut lines = Vec::new();
    for (key, action) in bindings {
        let key = format!("[{key}]");
        let item_width = key.width() + 1 + action.width();
        let separator = if has_item { "  " } else { "" };
        if used + separator.len() + item_width > width {
            lines.push(std::mem::take(&mut line));
            used = if indent + item_width <= width {
                indent
            } else {
                0
            };
            line.push_str(&" ".repeat(used));
        } else {
            line.push_str(separator);
            used += separator.len();
        }
        let action = fit(action, width.saturating_sub(used + key.width() + 1));
        write!(
            line,
            "{} {}",
            color(&key, ACCENT, display.colors),
            color(&action, INK, display.colors),
        )
        .unwrap();
        used += key.width() + 1 + action.width();
        has_item = true;
    }
    lines.push(line);
    lines
}

pub fn compact(history: History<'_>, display: Display, menu: &[String]) -> Frame {
    let mut list = menu.to_vec();
    let controls = control_lines(
        "履歴",
        &[
            ("↑↓", "地震を選ぶ"),
            ("Enter", "地図"),
            ("I", "詳細"),
            ("R", "更新"),
            ("Q", "終了"),
        ],
        display,
    );
    let border = Border::new(display.width(), FRAME, display.colors);
    list.extend(
        history.frame(
            border,
            display
                .rows
                .saturating_sub(menu.len() + controls.len())
                .max(3),
        ),
    );
    list.extend(controls);
    Frame {
        text: list.join("\n"),
        ..Frame::default()
    }
}

pub fn panel(
    menu: &[String],
    title: &str,
    status: &str,
    alert: bool,
    lines: &[String],
    display: Display,
    scroll: &mut usize,
    controls: &str,
) -> Frame {
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let mut text = menu.to_vec();
    let mut content: Vec<_> = wrap(status, inner)
        .into_iter()
        .map(|line| color(&line, if alert { ALERT } else { INK }, display.colors))
        .collect();
    let rows: Vec<_> = lines.iter().flat_map(|line| wrap(line, inner)).collect();
    let capacity = display.rows.saturating_sub(menu.len() + content.len() + 3);
    *scroll = (*scroll).min(rows.len().saturating_sub(capacity));
    content.extend(rows.iter().skip(*scroll).take(capacity).cloned());
    let height = content.len() + 2;
    text.extend(
        Border::new(
            width,
            if alert { FRAME_ALERT } else { FRAME },
            display.colors,
        )
        .text(content, title, height),
    );
    let keys = if capacity > 0 && rows.len() > capacity {
        format!(
            "↑↓ スクロール  {}–{}/{}  {controls}",
            *scroll + 1,
            *scroll + capacity,
            rows.len()
        )
    } else {
        controls.into()
    };
    text.push(fit(&keys, width));
    Frame {
        text: text.join("\n"),
        ..Frame::default()
    }
}

pub fn settings(
    menu: &[String],
    entries: &[(&str, &str)],
    selected: usize,
    dirty: bool,
    active_map: Option<&str>,
    notice: Option<(&str, bool)>,
    display: Display,
) -> Frame {
    let width = display.width().min(64);
    let inner = width - TEXT_FRAME_MARGIN;
    let on_save = selected == entries.len();
    let value_key = if on_save {
        ("Enter", "保存して適用")
    } else {
        ("←→", "値を変更")
    };
    let back = if dirty {
        "変更を破棄して戻る"
    } else {
        "戻る"
    };
    let controls = control_lines(
        "操作",
        &[("↑↓", "選ぶ"), value_key, ("S", "保存"), ("Esc", back)],
        display,
    );
    let capacity = display.rows.saturating_sub(menu.len() + controls.len() + 2);
    let row_count = capacity.min(entries.len() + 1);
    let start = (selected + 1).saturating_sub(row_count);
    let mut rows = Vec::new();
    for index in (start..entries.len() + 1).take(row_count) {
        let active = index == selected;
        let marker = if active { '▸' } else { ' ' };
        let line = if let Some((label, value)) = entries.get(index) {
            let label_width = if inner >= 38 { 18 } else { label.width() };
            let value = if active {
                format!("‹ {value} ›")
            } else {
                format!("  {value}")
            };
            format!("{marker} {}  {value}", pad(label, label_width))
        } else {
            format!("{marker} [ 保存して適用 ]")
        };
        let line = fit(&line, inner);
        if active && display.colors {
            let mut styled = String::new();
            map::rgb(&mut styled, SELECTED, true);
            styled.push_str(&color(&pad(&line, inner), INK, true));
            rows.push(styled);
        } else {
            let rgb = if index == entries.len() { ACCENT } else { INK };
            rows.push(color(&line, rgb, display.colors));
        }
    }
    if let Some((message, alert)) = notice {
        rows.extend(
            wrap(message, inner)
                .into_iter()
                .take(capacity.saturating_sub(rows.len()))
                .map(|line| color(&line, if alert { ALERT } else { ACCENT }, display.colors)),
        );
    } else if let Some(active_map) = active_map
        && rows.len() < capacity
    {
        rows.push(color(
            &fit(&format!("使用中の描画方式  {active_map}"), inner),
            MUTED,
            display.colors,
        ));
    }
    let title = if dirty {
        "設定 · 未保存"
    } else {
        "設定"
    };
    let height = rows.len() + 2;
    let mut text = menu.to_vec();
    text.extend(Border::new(width, FRAME, display.colors).text(rows, title, height));
    text.extend(controls);
    Frame {
        text: text.join("\n"),
        ..Frame::default()
    }
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    let cleaned: String = value.chars().filter(|c| !c.is_control()).collect();
    let mut remaining = cleaned.as_str();
    let mut lines = Vec::new();
    while !remaining.is_empty() {
        let line = fit(remaining, width);
        remaining = remaining[line.len()..].trim_start();
        lines.push(line);
    }
    lines
}

#[derive(Clone, Copy)]
pub enum MenuNotice {
    Forecast,
    Warning,
}

pub struct MenuItem<'a> {
    pub key: char,
    pub label: &'a str,
    pub active: bool,
    pub notice: Option<MenuNotice>,
}

pub fn menu_lines(items: &[MenuItem<'_>], display: Display) -> Vec<String> {
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let border = Border::new(width, FRAME, display.colors);
    let divider = color("│", FRAME, display.colors);
    let mut rows = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for item in items {
        let prefix = format!("{}{}", if item.active { '▸' } else { ' ' }, item.key);
        let label = fit(&format!(" {} ", item.label), inner - prefix.width());
        let item_width = prefix.width() + label.width();
        if used > 0 && used + 1 + item_width > inner {
            rows.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push_str(&divider);
            used += 1;
        }
        let alert_ink = item.notice.map(|notice| match notice {
            MenuNotice::Forecast => FORECAST_INK,
            MenuNotice::Warning => WARNING_INK,
        });
        if item.active {
            if display.colors {
                map::rgb(&mut line, SELECTED, true);
            }
            line.push_str(&color(
                &format!("{prefix}{label}"),
                alert_ink.unwrap_or(INK),
                display.colors,
            ));
        } else {
            line.push_str(&color(&prefix, alert_ink.unwrap_or(ACCENT), display.colors));
            line.push_str(&color(&label, alert_ink.unwrap_or(MUTED), display.colors));
        }
        used += item_width;
    }
    let hint = "Tab / Shift+Tab";
    if rows.is_empty() && used + 2 + hint.width() <= inner {
        line.push_str(&" ".repeat(inner - used - hint.width()));
        line.push_str(&color(hint, MUTED, display.colors));
    }
    rows.push(line);
    let height = rows.len() + 2;
    border.text(rows, APP_NAME, height)
}

fn color_logo(text: &str, split: usize, cyan: Rgb, amber: Rgb, ansi: bool) -> String {
    let (tui, quake) = text.split_at(split);
    format!("{}{}", color(tui, cyan, ansi), color(quake, amber, ansi))
}

fn logo_lines(ansi: bool) -> impl Iterator<Item = String> {
    LOGO.into_iter()
        .zip(LOGO_STYLE)
        .map(move |(row, (split, cyan, amber))| color_logo(row, split, cyan, amber, ansi))
}

fn compact_logo(ansi: bool) -> String {
    let (_, cyan, amber) = LOGO_STYLE[2];
    color_logo(APP_NAME, "Tui".len(), cyan, amber, ansi)
}

pub fn map_picker(options: &[Map], selected: usize, display: Display) -> Frame {
    let available_width = display.columns.saturating_sub(1);
    let width = available_width.min(60);
    let framed = width >= 20 && display.rows >= options.len() + 2;
    let inner = if framed {
        width - TEXT_FRAME_MARGIN
    } else {
        width
    };
    let descriptions = inner >= 46;
    let rows: Vec<_> = options
        .iter()
        .enumerate()
        .map(|(index, &map)| {
            let active = index == selected;
            let marker = if active { "▸" } else { " " };
            let label = if map == Map::Braille && inner < 22 {
                "Braille"
            } else {
                map.label()
            };
            let prefix = format!("{marker} [{}] ", index + 1);
            let foreground = if active { INK } else { ACCENT };
            let mut line = String::new();
            if display.colors && active {
                map::rgb(&mut line, SELECTED, true);
            }
            if display.colors {
                map::rgb(&mut line, foreground, false);
            }
            line.push_str(&fit(&prefix, inner));
            if display.colors {
                map::rgb(&mut line, INK, false);
            }
            let name_width = if descriptions {
                18
            } else {
                inner.saturating_sub(prefix.width())
            };
            line.push_str(&pad(label, name_width));
            if descriptions {
                let note = match map {
                    Map::Image(_) => "画像で地図を描画",
                    Map::Braille => "点字で地図を描画  ⣠⣴⣿⠟",
                    Map::Text => "文字で地図を描画",
                };
                if display.colors && !active {
                    map::rgb(&mut line, MUTED, false);
                }
                line.push_str(&pad(note, inner - prefix.width() - name_width));
            }
            if display.colors {
                line.push_str(RESET);
            }
            line
        })
        .collect();
    let panel = if framed {
        let mut rows = rows;
        if width >= 50 && display.rows >= 16 {
            rows.insert(0, String::new());
            rows.push(String::new());
        }
        let height = rows.len() + 2;
        Border::new(width, FRAME, display.colors).text(rows, "地図の描画方式", height)
    } else {
        let start = selected.saturating_add(1).saturating_sub(display.rows);
        rows.into_iter().skip(start).take(display.rows).collect()
    };

    let controls = if width >= 12 {
        control_lines(
            "",
            &[("↑↓", "選択"), ("Enter", "決定"), ("Q", "終了")],
            Display {
                columns: width + 1,
                ..display
            },
        )
    } else {
        Vec::new()
    };
    let spare_rows = display.rows.saturating_sub(panel.len());
    let show_controls = !controls.is_empty() && spare_rows >= controls.len();
    let header_space = if show_controls {
        spare_rows - controls.len()
    } else {
        spare_rows
    };
    let logo_width = LOGO.iter().map(|row| row.len()).max().unwrap_or(0);
    let mut body = Vec::new();
    if framed && width >= logo_width && header_space > LOGO.len() {
        let indent = " ".repeat((width - logo_width) / 2);
        body.extend(logo_lines(display.colors).map(|line| format!("{indent}{line}")));
        body.push(String::new());
    } else if framed && header_space >= 2 {
        let indent = " ".repeat((width - APP_NAME.len()) / 2);
        body.push(format!("{indent}{}", compact_logo(display.colors)));
        body.push(String::new());
    }
    body.extend(panel);
    if show_controls {
        if body.len() + controls.len() < display.rows {
            body.push(String::new());
        }
        body.extend(controls);
    }

    let left = " ".repeat((available_width - width) / 2);
    let top = display.rows.saturating_sub(body.len()) / 2;
    let mut text = "\n".repeat(top);
    for (index, line) in body.into_iter().enumerate() {
        if index != 0 {
            text.push('\n');
        }
        if !line.is_empty() {
            text.push_str(&left);
            text.push_str(&line);
        }
    }
    Frame {
        text,
        ..Frame::default()
    }
}

pub fn about(menu: &[String], display: Display, scroll: &mut usize) -> Frame {
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let controls = control_lines(
        "About",
        &[
            ("↑↓", "スクロール"),
            ("Home/End", "先頭/末尾"),
            ("Esc", "地震へ"),
            ("Q", "終了"),
        ],
        display,
    );
    let capacity = display.rows.saturating_sub(menu.len() + controls.len() + 2);
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let logo_width = LOGO.iter().map(|row| row.len()).max().unwrap_or(0);
    let mut content = Vec::new();
    if inner >= logo_width && capacity >= LOGO.len() + 5 {
        content.extend(logo_lines(display.colors));
        content.push(color(
            &format!("{version:>logo_width$}"),
            MUTED,
            display.colors,
        ));
        content.push(String::new());
    } else {
        content.push(compact_logo(display.colors));
        content.extend(
            wrap(version, inner)
                .into_iter()
                .map(|line| color(&line, MUTED, display.colors)),
        );
    }
    let renderer = format!("使用中の描画方式  {}", display.map.label());
    let sections: &[(&str, &[&str])] = &[
        (
            "地震・速報を見る",
            &[
                "地震  履歴を選ぶと、震源と各地の観測震度を地図に表示します。",
                "EEW   緊急地震速報をライブ受信します。予報は黄色、警報は赤です。",
                "津波  警報・注意報・予報の対象地域や第1波の見込みを確認できます。",
                "デモ  架空の速報で、予報・警報や震度の表示を試せます。",
            ],
        ),
        (
            "地図の描画",
            &[
                &renderer,
                "Sixel / Kitty は画像で、Unicode Braille と文字地図は文字で描きます。",
            ],
        ),
        (
            "主な操作",
            &[
                "↑↓ 地震を選ぶ  V 広域/観測点  I 詳細",
                "WASD 地図を移動  +/- 拡大/縮小  Z 全国/周辺  0 表示を戻す",
                "デモ  ←→ 震度変更  F 予報/警報  Space 再生・停止",
            ],
        ),
        (
            "データとクレジット",
            &[
                "気象庁発表の地震情報・EEW・津波情報を閲覧するアプリです。",
                "データ提供 Wolfx Project  地震情報・EEW・津波情報",
                "気象庁  震度観測点一覧・地震情報／細分区域GISをTuiQuakeが加工",
                "地図の加工内容と元データの承認番号は README.md に記載しています。",
                "国土交通省  国土数値情報（湖沼データ）をTuiQuakeが加工",
                "Natural Earth 1:50m Land  東アジアの周辺陸地",
                "FIGlet Slant / Glenn Chappell  ASCIIロゴの書体",
            ],
        ),
    ];

    for &(heading, details) in sections {
        if !content.last().is_some_and(String::is_empty) {
            content.push(String::new());
        }
        content.push(color(&fit(heading, inner), ACCENT, display.colors));
        content.extend(
            details
                .iter()
                .flat_map(|detail| wrap(detail, inner))
                .map(|line| color(&line, INK, display.colors)),
        );
    }

    let total = content.len();
    *scroll = (*scroll).min(total.saturating_sub(capacity));
    let end = (*scroll + capacity).min(total);
    let title = if capacity > 0 && total > capacity {
        format!("About  {}–{end}/{total}", *scroll + 1)
    } else {
        "About".into()
    };
    let visible = content.into_iter().skip(*scroll).take(capacity).collect();
    let height = end - *scroll + 2;
    let mut rows = menu.to_vec();
    rows.extend(Border::new(width, FRAME, display.colors).text(visible, &title, height));
    rows.extend(controls);
    Frame {
        text: rows.join("\n"),
        ..Frame::default()
    }
}

fn earthquake_summary(
    b: &Bulletin,
    notices: QuakeNotices,
    width: usize,
    rows: usize,
    ansi: bool,
) -> Vec<String> {
    let eew = match notices.eew {
        Some(true) => "あり",
        Some(false) => "なし",
        None => "記録なし",
    };
    let flags = [
        format!("EEW {eew}"),
        format!("津波 {}", notices.tsunami.unwrap_or("未確認")),
    ];
    let mut content = Vec::new();
    if rows >= 6
        && width >= BADGE_TEXT_OFFSET + 14
        && let Some(badge) = b
            .maximum
            .as_ref()
            .and_then(|level| intensity_badge_rows(level, ansi))
    {
        let mut heading = color(&pad("最大震度", BADGE_WIDTH), MUTED, ansi);
        if rows < 8
            && let Some(stamp) = b.origin.or(b.issued)
        {
            let label = if b.origin.is_some() {
                "発生"
            } else {
                "発表"
            };
            let date = format!("{label} {}", stamp.format("%m/%d %H:%M"));
            write!(
                heading,
                "  {}",
                color(&fit(&date, width - BADGE_TEXT_OFFSET), MUTED, ansi)
            )
            .unwrap();
        }
        content.push(heading);
        let facts = overview_facts(b);
        for (badge, fact) in badge.into_iter().zip(facts.iter().chain(&flags)) {
            content.push(format!(
                "{badge}  {}",
                color(&fit(fact, width - BADGE_TEXT_OFFSET), INK, ansi),
            ));
        }
    } else {
        let name = if b.name.is_empty() { &b.label } else { &b.name };
        content.push(color(&fit(name, width), INK, ansi));
        if let Some(level) = &b.maximum {
            let label = if width >= "最大震度".width() + 1 + level.label().width() {
                "最大震度"
            } else {
                "震度"
            };
            let value = fit(level.label(), width.saturating_sub(label.width() + 1));
            let background = level
                .index()
                .map(|index| PALETTE[(map::STATION + index) as usize]);
            content.push(format!(
                "{} {}",
                color(label, MUTED, ansi),
                eew_badge(&value, background, ansi)
            ));
        }
        let facts = magnitude_and_depth(b);
        if !facts.is_empty() {
            content.push(color(&fit(&facts, width), MUTED, ansi));
        }
        let combined = flags.join("  ");
        if combined.width() <= width {
            content.push(color(&combined, INK, ansi));
        } else {
            content.extend(flags.iter().map(|flag| color(&fit(flag, width), INK, ansi)));
        }
    }
    let stamps = times(b, width);
    let available = rows.saturating_sub(content.len());
    content.extend(
        stamps
            .iter()
            .skip(stamps.len().saturating_sub(available))
            .map(|date| color(&fit(date, width), MUTED, ansi)),
    );
    content.truncate(rows);
    content
}

fn earthquake_sidebar(
    b: &Bulletin,
    notices: QuakeNotices,
    history: History<'_>,
    layer: Layer,
    grouping: StationGrouping,
    start: usize,
    width: usize,
    rows: usize,
    ansi: bool,
) -> Vec<String> {
    let border = Border::new(width, FRAME, ansi);
    const MIN_HISTORY: usize = 3;
    if rows < MIN_HISTORY + 7 {
        return history.bottom_frame(border, rows);
    }

    let inner = width - TEXT_FRAME_MARGIN;
    let summary_rows = (rows - MIN_HISTORY - 6).min(8);
    let facts = earthquake_summary(b, notices, inner, summary_rows, ansi);
    let summary_height = summary_rows + 2;
    let mut lines = border.text(facts, "地震の概要", summary_height);

    let shared_rows = rows - summary_height - 2;
    let history_height = history.target_height().min(shared_rows - 2);
    let slots = shared_rows - history_height;
    let (title, entries) = match layer {
        Layer::Stations => station_readings_window(b, grouping, start, slots),
        Layer::Regions => {
            let count = b.regions.len();
            let start = if count <= slots {
                0
            } else {
                start.min(count - 1)
            };
            let end = (start + slots).min(count);
            let mut title = format!("各地の震度 · {}", layer.label());
            if count > slots {
                write!(title, "  {}–{end}/{count}", start + 1).unwrap();
            }
            (
                title,
                b.regions[start..end].iter().map(reading_row).collect(),
            )
        }
    };
    let readings = if entries.is_empty() {
        vec![color(&fit("震度データなし", inner), MUTED, ansi)]
    } else {
        entries
            .iter()
            .map(|(line, rgb)| color(&fit(line, inner), *rgb, ansi))
            .collect()
    };
    let readings_height = slots + 2;
    lines.extend(Border::new(width, FRAME_ALERT, ansi).text(readings, &title, readings_height));
    lines.extend(history.frame(border, history_height));
    lines
}

fn eew_badge(value: &str, background: Option<Rgb>, ansi: bool) -> String {
    if let Some(background) = background.filter(|_| ansi) {
        let mut badge = String::new();
        map::rgb(&mut badge, background, true);
        badge.push_str(&color(value, badge_ink(background), true));
        badge
    } else {
        value.to_owned()
    }
}

const BADGE_WIDTH: usize = 10;
const BADGE_TEXT_OFFSET: usize = BADGE_WIDTH + 2;

fn intensity_badge_rows(level: &Level, ansi: bool) -> Option<[String; 5]> {
    // Each terminal row carries two dot rows; the first upper half is padding.
    const DIGITS: [[&str; 4]; 7] = [
        ["  ▄ ", "▀▀█ ", "  █ ", "▄▄█▄"],
        [" ▄▄ ", "▀  █", " ▄▀ ", "█▄▄▄"],
        ["▄▄▄▄", "   █", "▀▀▀█", "▄▄▄█"],
        ["▄  ▄  ", "█  █  ", "▀▀▀█▀▀", "   █  "],
        ["▄▄▄▄", "█   ", "▀▀▀█", "▄▄▄█"],
        ["▄▄▄▄", "█   ", "█▀▀█", "█▄▄█"],
        ["▄▄▄▄", "  ▄▀", " ▄▀ ", " █  "],
    ];
    const MINUS: [&str; 4] = ["   ", "▀▀▀", "   ", "   "];
    const PLUS: [&str; 4] = [" ▄ ", "▀█▀", "   ", "   "];
    let background = PALETTE[(map::STATION + level.index()?) as usize];
    let label = short_intensity_label(level);
    let digits = DIGITS[usize::from(label.as_bytes()[0] - b'1')];
    let sign = match &label[1..] {
        "-" => Some(MINUS),
        "+" => Some(PLUS),
        _ => None,
    };
    let left_padding = " ".repeat((BADGE_WIDTH - digits[0].width()) / 2);
    Some(std::array::from_fn(|row| {
        if let Some(digit) = digits.get(row) {
            let value = match sign {
                Some(sign) => format!(" {digit} {} ", sign[row]),
                None => pad(&format!("{left_padding}{digit}"), BADGE_WIDTH),
            };
            eew_badge(&value, Some(background), ansi)
        } else if ansi {
            // Paint half a cell of color to match the one-dot top margin.
            color(&"▀".repeat(BADGE_WIDTH), background, true)
        } else {
            " ".repeat(BADGE_WIDTH)
        }
    }))
}

fn overview_facts(b: &Bulletin) -> [String; 3] {
    let name = if b.name.is_empty() { "不明" } else { &b.name };
    [
        name.to_owned(),
        if b.magnitude.is_empty() {
            "M 不明".into()
        } else {
            format!("M{}", b.magnitude)
        },
        b.depth
            .map_or_else(|| "深さ 不明".into(), |km| format!("深さ {km} km")),
    ]
}

fn eew_banner(b: &Bulletin, width: usize, ansi: bool) -> Vec<String> {
    let (kind, background) = if b.eew_cancelled {
        ("取消", None)
    } else if b.warning {
        ("警報", Some(WARNING_INK))
    } else {
        ("予報", Some(FORECAST_INK))
    };
    let title = format!("緊急地震速報（{kind}）");
    let mut report = b
        .eew_serial
        .map_or_else(String::new, |number| format!("第{number}報"));
    if b.eew_final && !b.eew_cancelled {
        report.push_str(if report.is_empty() {
            "最終報"
        } else {
            "（最終報）"
        });
    }
    let mut rows = if report.is_empty() {
        wrap(&title, width)
    } else if title.width() + 1 + report.width() <= width {
        vec![format!("{title} {report}")]
    } else {
        let mut rows = wrap(&title, width);
        rows.extend(wrap(&report, width));
        rows
    };
    for row in &mut rows {
        let inset = (width - row.width()) / 2;
        let centered = pad(&format!("{}{row}", " ".repeat(inset)), width);
        *row = eew_badge(&centered, background, ansi);
    }
    rows
}

fn eew_overview(
    b: &Bulletin,
    width: usize,
    height: usize,
    ansi: bool,
    source: Option<&str>,
) -> Vec<String> {
    let inner = width - TEXT_FRAME_MARGIN;
    let capacity = height.saturating_sub(2);
    let level = b.maximum.as_ref();
    let label = level.map_or("不明", short_intensity_label);
    let background = level
        .and_then(Level::index)
        .map(|index| PALETTE[(map::STATION + index) as usize]);
    let facts = overview_facts(b);
    let mut content = Vec::new();
    if capacity >= 6
        && inner >= BADGE_WIDTH
        && let Some(badge) = level.and_then(|level| intensity_badge_rows(level, ansi))
    {
        content.push(color("推定最大", MUTED, ansi));
        let side_by_side = inner >= BADGE_TEXT_OFFSET + 13;
        for (row, badge) in badge.into_iter().enumerate() {
            if side_by_side && let Some(fact) = facts.get(row) {
                content.push(format!(
                    "{badge}  {}",
                    color(&fit(fact, inner - BADGE_TEXT_OFFSET), INK, ansi),
                ));
            } else {
                content.push(badge);
            }
        }
        if !side_by_side {
            content.extend(facts.iter().map(|fact| color(&fit(fact, inner), INK, ansi)));
        }
    } else {
        let heading = if inner >= "推定最大".width() + 1 + label.width() {
            "推定最大"
        } else {
            "予測"
        };
        let value = fit(label, inner.saturating_sub(heading.width() + 1));
        content.push(format!(
            "{} {}",
            color(heading, MUTED, ansi),
            eew_badge(&value, background, ansi),
        ));
        content.extend(facts.iter().map(|fact| color(&fit(fact, inner), INK, ansi)));
    }

    // Show publication time even when the origin time needs a separate line.
    let stamps = times(b, inner);
    let available = capacity.saturating_sub(content.len());
    content.extend(
        stamps
            .iter()
            .skip(stamps.len().saturating_sub(available))
            .map(|line| color(&fit(line, inner), MUTED, ansi)),
    );
    let used = content.len().min(capacity) + 2;
    let title = if let Some(source) = source {
        if "速報概要 · ".width() + source.width() + 6 <= width {
            format!("速報概要 · {source}")
        } else {
            source.to_owned()
        }
    } else {
        "速報概要".into()
    };
    Border::new(width, if b.warning { FRAME_ALERT } else { FRAME }, ansi)
        .text(content, &title, used)
}

fn eew_area_entries(b: &Bulletin) -> Option<(&'static str, Vec<(String, Rgb)>)> {
    if !b.warning && b.forecast_areas.is_empty() {
        return None;
    }
    let title = if b.warning && b.forecast_areas.is_empty() {
        "警報対象地域"
    } else if b.warning {
        "警報対象地域 · 予想震度"
    } else {
        "地域別の予想震度"
    };
    let mut rows = Vec::new();
    for name in &b.warning_areas {
        if let Some(area) = b.forecast_areas.iter().find(|area| area.name == *name) {
            rows.push(eew_prediction_row(area, false));
        } else {
            rows.push((name.clone(), INK));
        }
    }
    rows.extend(
        b.forecast_areas
            .iter()
            .filter(|area| !b.warning_areas.contains(&area.name))
            .map(|area| eew_prediction_row(area, b.warning)),
    );
    Some((title, rows))
}

fn eew_prediction_row(area: &ForecastArea, forecast_only: bool) -> (String, Rgb) {
    let lower = area.minimum.as_ref().map(short_intensity_label);
    let upper = area.maximum.as_ref().map(short_intensity_label);
    let range = match (lower, upper) {
        (Some(low), Some(high)) if low == high => low.to_owned(),
        (Some(low), Some(high)) => format!("{low}〜{high}"),
        (Some(low), None) => format!("下限 {low}"),
        (None, Some(high)) => format!("上限 {high}"),
        (None, None) => "不明".into(),
    };
    let rgb = area
        .maximum
        .as_ref()
        .or(area.minimum.as_ref())
        .map_or(INK, reading_rgb);
    let flag = if forecast_only { "予報  " } else { "" };
    (
        format!("{}  {flag}{}", pad(&range, range.width().max(7)), area.name),
        rgb,
    )
}

fn eew_area_panel(
    title: &str,
    areas: &[(String, Rgb)],
    warning: bool,
    missing: usize,
    width: usize,
    height: usize,
    ansi: bool,
) -> Vec<String> {
    let inner = width - TEXT_FRAME_MARGIN;
    let capacity = height - 2;
    let shown = if areas.len() > capacity && capacity > 1 {
        capacity - 1
    } else {
        capacity
    };
    let mut content: Vec<_> = areas
        .iter()
        .take(shown)
        .map(|(line, rgb)| color(&fit(line, inner), *rgb, ansi))
        .collect();
    if areas.is_empty() {
        content.push(color(&fit("対象地域の情報なし", inner), MUTED, ansi));
    } else if areas.len() > shown && content.len() < capacity {
        content.push(color(
            &fit(
                &format!("ほか {} 地域 · I 詳細", areas.len() - shown),
                inner,
            ),
            MUTED,
            ansi,
        ));
    } else if missing > 0 && content.len() < capacity {
        content.push(color(
            &fit(&format!("地図にない地域 {missing}件"), inner),
            MUTED,
            ansi,
        ));
    }
    let title = if areas.len() > shown {
        format!("{title} {}", areas.len())
    } else {
        title.to_owned()
    };
    Border::new(width, if warning { FRAME_ALERT } else { FRAME }, ansi)
        .text(content, &title, height)
}

pub fn detail(
    b: &Bulletin,
    banner: &str,
    layer: Layer,
    grouping: StationGrouping,
    display: Display,
    scroll: &mut usize,
    menu: &[String],
) -> Frame {
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let mut header = menu.to_vec();
    let mut content = event_facts(b, inner, display.colors);
    let mut rows = Vec::new();
    let mut intensity_rows_start = None;
    let mut comment = b.comment.split_whitespace().collect::<Vec<_>>().join(" ");
    comment.retain(|c| !c.is_control());
    while !comment.is_empty() {
        let line = fit(&comment, inner);
        comment.drain(..line.len());
        rows.push((line, INK));
    }
    if !rows.is_empty() {
        rows.push((String::new(), INK));
    }
    if let Some((title, areas)) = eew_area_entries(b) {
        rows.push((
            format!("{title}  {} 地域", areas.len()),
            if b.warning { ALERT } else { ACCENT },
        ));
        rows.extend(areas);
    } else if b.has_observations() {
        let count = match layer {
            Layer::Regions => b.regions.len(),
            Layer::Stations => b.stations.len(),
        };
        rows.push((format!("{}  {count} 件", observation_label(layer)), MUTED));
        if layer == Layer::Stations && grouping == StationGrouping::Intensity {
            intensity_rows_start = Some(rows.len());
        }
        rows.extend(readings(b, layer, grouping));
    }
    let mut actions = vec![("↑↓", "スクロール"), ("I/B", "地図へ戻る")];
    if b.has_observations() {
        actions.push(("V", "広域/観測点"));
    }
    actions.push(("Q", "終了"));
    let controls = control_lines("詳細", &actions, display);
    let mut capacity = display
        .rows
        .saturating_sub(header.len() + content.len() + controls.len() + 3);
    let separator = capacity > 1 && !rows.is_empty();
    capacity -= usize::from(separator);
    *scroll = (*scroll).min(rows.len().saturating_sub(capacity));
    if separator {
        let continuation = intensity_rows_start
            .filter(|start| *scroll > *start && !rows[*scroll].0.starts_with("── "))
            .and_then(|start| {
                rows[start..*scroll]
                    .iter()
                    .rfind(|(line, _)| line.starts_with("── "))
            });
        content.push(continuation.map_or_else(String::new, |(line, rgb)| {
            color(&fit(line, inner), *rgb, display.colors)
        }));
    }
    content.extend(
        rows.iter()
            .skip(*scroll)
            .take(capacity)
            .map(|(line, rgb)| color(&fit(line, inner), *rgb, display.colors)),
    );
    let height = content.len() + 2;
    let border = Border::new(
        width,
        if b.warning { FRAME_ALERT } else { FRAME },
        display.colors,
    );
    header.extend(border.text(content, &format!("文字詳細 · {banner}"), height));
    if capacity > 0 && rows.len() > capacity {
        header.push(color(
            &fit(
                &format!("表示 {}–{}/{}", *scroll + 1, *scroll + capacity, rows.len()),
                width,
            ),
            MUTED,
            display.colors,
        ));
    }
    header.extend(controls);
    Frame {
        text: header.join("\n"),
        ..Frame::default()
    }
}

pub struct Renderer {
    pub atlas: Atlas,
    raster: Raster,
    image_raster: Raster,
    rgb: Vec<u8>,
    frame: Frame,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        Ok(Self {
            atlas: Atlas::load()?,
            raster: Raster::default(),
            image_raster: Raster::default(),
            rgb: Vec::new(),
            frame: Frame::default(),
        })
    }

    pub fn render(&mut self, scene: Scene<'_>, display: Display) -> &Frame {
        let width = display.width();
        let Scene {
            bulletin: b,
            banner,
            eew_source,
            series,
            view,
            layer,
            station_grouping,
            controls,
            history,
            menu,
            reading_start,
            quake_notices,
        } = scene;
        let framed = !menu.is_empty();
        let eew_panel = framed && b.eew;
        let area_entries = eew_area_entries(b);
        let warning_missing = b
            .warning_areas
            .iter()
            .filter(|area| !self.atlas.has_area(area))
            .count();
        let map_edges = if framed { 2 } else { 0 };
        let gap = if framed { PANEL_GAP } else { SIDEBAR_GAP };
        let split = history.is_some() && width >= MIN_SPLIT_WIDTH;
        let (map_cols, sidebar_width) = if split {
            let map_cols = width * 46 / 100;
            (map_cols, width - map_cols - map_edges - gap)
        } else if framed {
            let has_aside = eew_panel || !b.warning_areas.is_empty() || b.has_observations();
            if history.is_none() && has_aside && width >= MIN_SPLIT_WIDTH {
                let map_cols = width * 58 / 100;
                (map_cols, width - map_cols - map_edges - gap)
            } else {
                (width - map_edges, 0)
            }
        } else {
            let map_cols = width.min(MAP_MAX_COLUMNS);
            let sidebar_width = if history.is_none() && width >= map_cols + gap + 26 {
                26
            } else {
                0
            };
            (map_cols, sidebar_width)
        };
        let mut status = if banner.is_empty() {
            b.label.clone()
        } else {
            banner.to_owned()
        };
        if b.has_observations() {
            write!(status, " · {}", layer.label()).unwrap();
        }
        let mut header = menu.to_vec();
        if menu.is_empty() && history.is_none() {
            header = event_header(b, &status, width, display.colors);
        } else if eew_panel {
            header.extend(eew_banner(b, width, display.colors));
            if !banner.is_empty() {
                header.push(color(&fit(banner, width), MUTED, display.colors));
            }
        } else if history.is_none() || sidebar_width == 0 {
            header.push(color(
                &fit(&status, width),
                if b.warning { ALERT } else { MUTED },
                display.colors,
            ));
            if history.is_some() {
                let summary_rows = display.rows.saturating_sub(menu.len() + 9).min(5);
                header.extend(earthquake_summary(
                    b,
                    quake_notices,
                    width,
                    summary_rows,
                    display.colors,
                ));
            } else {
                header.extend(event_facts(b, width, display.colors));
            }
        }
        header.push(String::new());
        let mut aside = Vec::new();
        if history.is_none() && !eew_panel {
            if let Some((title, areas)) = &area_entries {
                aside.push(((*title).into(), if b.warning { ALERT } else { ACCENT }));
                if warning_missing > 0 {
                    aside.push((format!("地図にない地域 {warning_missing}件"), MUTED));
                }
                aside.extend(areas.iter().cloned());
            } else if b.has_observations() {
                aside.push((observation_label(layer).into(), MUTED));
                aside.extend(readings(b, layer, station_grouping));
            }
        }
        let mut legend = Vec::new();
        if b.epicenter.is_some() {
            legend.push(('✚', if b.eew { "推定震央" } else { "震央" }, map::EPICENTER));
        }
        if !b.warning_areas.is_empty() {
            legend.push(('■', "警報対象", map::WARNING));
        }
        if b.has_observations() && (display.colors || layer == Layer::Regions) {
            let (glyph, start) = if layer == Layer::Regions {
                ('■', map::REGION)
            } else {
                ('●', map::STATION)
            };
            legend.extend(
                INTENSITIES
                    .iter()
                    .enumerate()
                    .map(|(i, name)| (glyph, *name, start + i as u8)),
            );
        }
        let mut footer = Vec::new();
        if !framed {
            footer.push(String::new());
        }
        let mut line = String::new();
        let mut used = 0;
        for (glyph, label, rgb) in legend {
            let count = label.width() + 2;
            if used > 0 && used + count + 2 > width {
                footer.push(std::mem::take(&mut line));
                used = 0;
            }
            if used > 0 {
                line.push_str("  ");
                used += 2;
            }
            let glyph = if display.colors {
                glyph
            } else {
                map::plain(rgb)
            };
            line.push_str(&color(
                &glyph.to_string(),
                PALETTE[rgb as usize],
                display.colors,
            ));
            write!(line, " {label}").unwrap();
            used += count;
        }
        if !line.is_empty() {
            footer.push(line);
        }
        if split {
            // Reserve the same space for the legend even when observations are unavailable.
            let full_width = "震央".width()
                + 2
                + INTENSITIES
                    .iter()
                    .map(|label| label.width() + 4)
                    .sum::<usize>();
            let legend_rows = 1 + usize::from(width < full_width);
            footer.resize_with(footer.len().max(legend_rows), String::new);
        }
        if sidebar_width == 0 && !aside.is_empty() {
            footer.push(color(
                &fit(
                    &aside
                        .iter()
                        .map(|(line, _)| line.as_str())
                        .collect::<Vec<_>>()
                        .join("  /  "),
                    width,
                ),
                MUTED,
                display.colors,
            ));
        }
        footer.extend_from_slice(controls);

        if eew_panel && sidebar_width == 0 {
            let available = display
                .rows
                .saturating_sub(header.len() + footer.len() + map_edges + 5);
            let area_height = if area_entries.is_some() && available >= 6 {
                (available / 3).clamp(3, 7)
            } else {
                0
            };
            if available >= 3 {
                header.extend(eew_overview(
                    b,
                    width,
                    available - area_height,
                    display.colors,
                    eew_source,
                ));
            } else {
                let intensity = b.maximum.as_ref().map_or("不明", short_intensity_label);
                let mut summary = format!("推定最大 {intensity}");
                if let Some(source) = eew_source {
                    write!(summary, " · {source}").unwrap();
                }
                header.push(fit(&summary, width));
            }
            if area_height > 0
                && let Some((title, areas)) = &area_entries
            {
                footer.splice(
                    ..0,
                    eew_area_panel(
                        title,
                        areas,
                        b.warning,
                        warning_missing,
                        width,
                        area_height,
                        display.colors,
                    ),
                );
            }
        }
        self.frame.map_row = header.len() + 1 + usize::from(framed);
        self.frame.map_col = 1 + usize::from(framed);

        let rows = display
            .rows
            .saturating_sub(header.len() + footer.len() + map_edges + 1)
            .clamp(1, 60);
        self.frame.map_view = None;
        if let Some(image) = display.map.image() {
            let (dx, dy) = image.cell();
            self.image_raster.resize(map_cols * dx, rows * dy);
            let clamped = self.atlas.draw(
                &mut self.image_raster,
                b,
                series,
                layer,
                view,
                Some((dx, dy)),
            );
            self.frame.map_view = Some(clamped);
            match image {
                Image::Sixel(_) => self.image_raster.sixel(&mut self.frame.graphic),
                Image::Kitty => {
                    graphics::kitty(&self.image_raster, &mut self.rgb, &mut self.frame.graphic)
                }
            }
        } else {
            self.frame.graphic.clear();
        }
        // A Windows console host or multiplexer can drop Kitty commands even
        // when the outer terminal supports them. Leave a usable map underneath.
        let text_map = !matches!(display.map, Map::Image(Image::Sixel(_)));
        if text_map {
            self.raster
                .resize(map_cols * TEXT_CELL_WIDTH, rows * TEXT_CELL_HEIGHT);
            let clamped = self
                .atlas
                .draw(&mut self.raster, b, series, layer, view, None);
            self.frame.map_view.get_or_insert(clamped);
        }
        let map_border = Border::new(map_cols + 2, FRAME, display.colors);
        let sidebar = if sidebar_width == 0 {
            Vec::new()
        } else if eew_panel {
            let height = rows + map_edges;
            let area_space = if area_entries.is_some() && height >= 6 {
                3
            } else {
                0
            };
            let mut panels = eew_overview(
                b,
                sidebar_width,
                height - area_space,
                display.colors,
                eew_source,
            );
            let remaining = height - panels.len();
            if remaining >= 3
                && let Some((title, areas)) = &area_entries
            {
                panels.extend(eew_area_panel(
                    title,
                    areas,
                    b.warning,
                    warning_missing,
                    sidebar_width,
                    remaining,
                    display.colors,
                ));
            }
            panels
        } else if let Some(history) = history {
            earthquake_sidebar(
                b,
                quake_notices,
                history,
                layer,
                station_grouping,
                reading_start,
                sidebar_width,
                rows + map_edges,
                display.colors,
            )
        } else {
            let title = aside
                .first()
                .map(|(label, _)| label.clone())
                .unwrap_or_default();
            let capacity = rows;
            if framed && !aside.is_empty() {
                aside.remove(0);
            }
            if aside.len() > capacity {
                let omitted = aside.len() - capacity + 1;
                aside.truncate(capacity - 1);
                aside.push((format!("ほか {omitted} 件"), MUTED));
            }
            if framed && aside.is_empty() && title.is_empty() {
                Vec::new()
            } else {
                let inner = sidebar_width - if framed { TEXT_FRAME_MARGIN } else { 0 };
                let lines = aside
                    .into_iter()
                    .map(|(line, rgb)| color(&fit(&line, inner), rgb, display.colors))
                    .collect();
                if framed {
                    let tone = if b.warning { FRAME_ALERT } else { FRAME };
                    Border::new(sidebar_width, tone, display.colors).text(lines, &title, rows + 2)
                } else {
                    lines
                }
            }
        };
        let separator = if framed {
            " ".repeat(gap)
        } else {
            color(" │ ", MUTED, display.colors)
        };
        let text = &mut self.frame.text;
        text.clear();
        for line in header {
            writeln!(text, "{line}").unwrap();
        }
        let mut next_side = sidebar.iter();
        let edge = map_border.edge();
        let map_label = format!(
            "地図 · {}  {:.1}x",
            if view.national {
                "全国"
            } else {
                "震源周辺"
            },
            self.frame.map_view.as_ref().map_or(1.0, View::zoom_factor),
        );
        for y in 0..rows + map_edges {
            if framed && y == 0 {
                text.push_str(&map_border.top(&map_label));
            } else if framed && y == rows + 1 {
                text.push_str(&map_border.bottom());
            } else {
                if framed {
                    text.push_str(&edge);
                }
                if text_map {
                    text_row(
                        text,
                        &self.raster,
                        y - usize::from(framed),
                        display.colors,
                        display.map == Map::Braille,
                    );
                } else {
                    text.extend(std::iter::repeat_n(' ', map_cols));
                }
                if framed {
                    text.push_str(&edge);
                }
            }
            if let Some(line) = next_side.next() {
                text.push_str(&separator);
                text.push_str(line);
            }
            text.push('\n');
        }
        text.push_str(&footer.join("\n"));
        &self.frame
    }
}

fn map_ink(color: Color) -> Rgb {
    match color {
        map::LAND => (112, 174, 179),
        map::SURROUNDINGS => (34, 51, 66),
        _ => PALETTE[color as usize],
    }
}

const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];
const SAMPLES_PER_QUADRANT: u16 = (TEXT_CELL_WIDTH * TEXT_CELL_HEIGHT / 4) as u16;

fn average(colors: impl Iterator<Item = Rgb>) -> Rgb {
    let (mut red, mut green, mut blue, mut count) = (0u16, 0u16, 0u16, 0u16);
    for (r, g, b) in colors {
        red += u16::from(r);
        green += u16::from(g);
        blue += u16::from(b);
        count += 1;
    }
    (
        (red / count) as u8,
        (green / count) as u8,
        (blue / count) as u8,
    )
}

fn distance((r, g, b): Rgb, (red, green, blue): Rgb) -> u32 {
    let squared = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
    squared(r, red) + squared(g, green) + squared(b, blue)
}

fn color_quadrants(quadrants: [Rgb; 4]) -> (char, Rgb, Rgb) {
    let first = quadrants[0];
    if quadrants[1..].iter().all(|&color| color == first) {
        return (' ', first, first);
    }
    // Each glyph can use two inks. Score each distinct partition of the
    // four quarters against their sampled colors.
    let mut best = (u32::MAX, 0, first, first);
    for mask in 1..8 {
        let group = |i| usize::from(mask & (1usize << i) != 0);
        let cluster = |index| average((0..4).filter(|&i| group(i) == index).map(|i| quadrants[i]));
        let colors = [cluster(0), cluster(1)];
        let error = (0..4)
            .map(|i| distance(quadrants[i], colors[group(i)]))
            .sum();
        if error < best.0 {
            best = (error, mask, colors[1], colors[0]);
        }
    }
    (QUADRANTS[best.1], best.2, best.3)
}

fn text_cell(
    pixels: [[Color; SAMPLES_PER_QUADRANT as usize]; 4],
    marker: Option<Color>,
    colors: bool,
) -> (char, Rgb, Rgb) {
    let sea = PALETTE[map::SEA as usize];
    if let Some(marker) = marker {
        let glyph = if marker == map::EPICENTER {
            if colors { '✚' } else { '+' }
        } else {
            '●'
        };
        let land = pixels.iter().flatten().filter(|&&p| p >= map::LAND).count();
        let paper = if land >= usize::from(SAMPLES_PER_QUADRANT * 2) {
            PALETTE[map::LAND as usize]
        } else {
            sea
        };
        return (glyph, map_ink(marker), paper);
    }
    if colors {
        return color_quadrants(pixels.map(|quarter| average(quarter.into_iter().map(map_ink))));
    }

    let mut warning = false;
    let mut land = 0;
    let mut mask = 0;
    let mut surroundings = 0;
    for (i, quarter) in pixels.into_iter().enumerate() {
        let mut filled = 0;
        for color in quarter {
            match color {
                map::SEA => {}
                map::SURROUNDINGS => surroundings += 1,
                _ => {
                    filled += 1;
                    warning |= color == map::WARNING;
                }
            }
        }
        land += filled;
        if filled >= SAMPLES_PER_QUADRANT / 2 {
            mask |= 1 << i;
        }
    }
    let glyph = if warning {
        map::plain(map::WARNING)
    } else if mask != 0 {
        QUADRANTS[mask]
    } else if land != 0 {
        '·'
    } else if surroundings >= SAMPLES_PER_QUADRANT {
        '░'
    } else {
        ' '
    };
    (glyph, sea, sea)
}

const BRAILLE_DOTS: [u8; 8] = [1, 8, 2, 16, 4, 32, 64, 128];

fn braille_cell(pixels: [[Color; 4]; 8], colors: bool) -> (char, Rgb, Rgb) {
    let mut land = 0;
    let mut small_islands = 0;
    let mut surrounding = 0;
    let mut area = None;
    for (samples, dot) in pixels.into_iter().zip(BRAILLE_DOTS) {
        let mut domestic = 0;
        let mut foreign = 0;
        for color in samples {
            match color {
                map::SEA => {}
                map::SURROUNDINGS => foreign += 1,
                _ => {
                    domestic += 1;
                    if color >= map::WARNING
                        && area.is_none_or(|old| {
                            color == map::WARNING || (old != map::WARNING && color > old)
                        })
                    {
                        area = Some(color);
                    }
                }
            }
        }
        if domestic >= 2 {
            land |= dot;
        } else if domestic == 1 {
            small_islands |= dot;
        }
        if foreign >= 2 {
            surrounding |= dot;
        }
    }
    let sea = map_ink(map::SEA);
    if !colors {
        if area == Some(map::WARNING) {
            return (map::plain(map::WARNING), sea, sea);
        }
        surrounding &= 0x95;
    }
    if land == 0 {
        land = small_islands;
    }
    let (dots, ink) = if land == 0 {
        (surrounding, map::SURROUNDINGS)
    } else {
        (land, area.unwrap_or(map::LAND))
    };
    let glyph = if dots == 0 {
        ' '
    } else {
        char::from_u32(0x2800 + u32::from(dots)).unwrap()
    };
    (glyph, map_ink(ink), sea)
}

fn text_row(out: &mut String, raster: &Raster, row: usize, colors: bool, braille: bool) {
    let mut foreground = None;
    let mut background = None;
    let width = raster.width / TEXT_CELL_WIDTH;
    let offset = row * TEXT_CELL_HEIGHT * raster.width;
    let half_width = TEXT_CELL_WIDTH / 2;
    let half_height = TEXT_CELL_HEIGHT / 2;
    for (column, &marker) in raster.markers[row * width..][..width].iter().enumerate() {
        let sample = |x, y| raster.pixels[offset + y * raster.width + column * TEXT_CELL_WIDTH + x];
        let (glyph, ink, paper) = if let Some(Some(label)) = raster.labels.get(row * width + column)
        {
            (
                label.glyph,
                map_ink(map::SEA),
                PALETTE[label.color as usize],
            )
        } else if braille && marker.is_none() {
            let dots = std::array::from_fn(|dot| {
                let left = dot % 2 * 2;
                let top = dot / 2 * 2;
                std::array::from_fn(|point| sample(left + point % 2, top + point / 2))
            });
            braille_cell(dots, colors)
        } else {
            let pixels = std::array::from_fn(|quarter| {
                let left = quarter % 2 * half_width;
                let top = quarter / 2 * half_height;
                std::array::from_fn(|dot| sample(left + dot % half_width, top + dot / half_width))
            });
            text_cell(pixels, marker, colors)
        };
        if colors {
            if background != Some(paper) {
                map::rgb(out, paper, true);
                background = Some(paper);
            }
            if glyph != ' ' && foreground != Some(ink) {
                map::rgb(out, ink, false);
                foreground = Some(ink);
            }
        }
        out.push(glyph);
    }
    if colors {
        out.push_str(RESET);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_names_start_in_the_same_column_at_every_intensity() {
        let mut bulletin = crate::data::demo().pop().unwrap();
        bulletin.regions[0].level = Level::Known(4);
        bulletin.regions[1].level = Level::Known(3);
        bulletin.regions[2].level = Level::Other("5弱以上・未入電".into());
        let lines = readings(&bulletin, Layer::Regions, StationGrouping::Prefecture);
        assert!(lines[0].0.starts_with("5-  "));
        assert!(lines[1].0.starts_with(" 4  "));
        assert!(lines[2].0.starts_with(" —  "));
        assert!(lines[2].0.ends_with("（5弱以上・未入電）"));
    }

    #[test]
    fn station_lists_group_by_prefecture_and_keep_the_heading_when_scrolling() {
        let bulletin = crate::data::demo().pop().unwrap();
        let rows = readings(&bulletin, Layer::Stations, StationGrouping::Prefecture);
        let headings: Vec<_> = rows
            .iter()
            .filter(|(line, _)| line.starts_with("── "))
            .map(|(line, _)| line.as_str())
            .collect();
        assert_eq!(
            headings,
            [
                "── 宮城県",
                "── 岩手県",
                "── 福島県",
                "── 山形県",
                "── 秋田県"
            ]
        );
        let (title, rows) = station_readings_window(&bulletin, StationGrouping::Prefecture, 4, 2);
        assert_eq!(title, "各地の震度 · 観測点  5–5/14");
        assert_eq!(rows[0].0, "── 岩手県");
        assert!(rows[1].0.contains("大船渡"));
        let (title, rows) = station_readings_window(&bulletin, StationGrouping::Prefecture, 3, 4);
        assert_eq!(title, "各地の震度 · 観測点  4–5/14");
        assert_eq!(rows[0].0, "── 宮城県");
        assert!(rows[1].0.contains("仙台"));
        assert_eq!(rows[2].0, "── 岩手県");
        assert!(rows[3].0.contains("大船渡"));
    }

    #[test]
    fn station_lists_can_group_intensities_across_prefectures() {
        let mut bulletin = crate::data::demo().pop().unwrap();
        bulletin.stations[0].level = Level::Known(5);
        let rows = readings(&bulletin, Layer::Stations, StationGrouping::Intensity);
        assert_eq!(
            rows[0],
            ("── 震度 5+".into(), reading_rgb(&Level::Known(5)))
        );
        assert_eq!(rows[1].0, "石巻");
        assert_eq!(
            rows[2],
            ("── 震度 5-".into(), reading_rgb(&Level::Known(4)))
        );
        let four = rows.iter().position(|row| row.0 == "── 震度 4").unwrap();
        assert!(rows[four + 1].0.contains("気仙沼"));
        assert!(rows[four + 2].0.contains("仙台"));
        assert!(rows[four + 3].0.contains("大船渡"));
        let (title, window) = station_readings_window(&bulletin, StationGrouping::Intensity, 1, 2);
        assert!(title.contains("震度別"));
        assert_eq!(window[0].0, "── 震度 5-");
        assert_eq!(window[1].0, "南三陸");
    }

    #[test]
    fn startup_picker_fits_and_keeps_the_selected_renderer_visible() {
        let options = [
            Map::Image(Image::Sixel((10, 20))),
            Map::Image(Image::Kitty),
            Map::Braille,
        ];
        let mut display = Display {
            columns: 120,
            rows: 30,
            colors: false,
            map: Map::Braille,
        };
        let wide = map_picker(&options, 1, display);
        assert!(wide.text.contains(LOGO[0]));
        assert!(wide.text.contains("▸ [2] Kitty"));
        assert!(wide.text.contains("点字で地図を描画"));
        let border = wide.text.lines().find(|line| line.contains("╭")).unwrap();
        assert_eq!(border.find('╭'), Some((display.columns - 1 - 60) / 2));

        for (columns, rows) in [
            (120, 30),
            (70, 14),
            (36, 8),
            (25, 5),
            (19, 4),
            (14, 2),
            (7, 1),
        ] {
            display.columns = columns;
            display.rows = rows;
            display.colors = true;
            let frame = map_picker(&options, 2, display);
            assert!(frame.text.contains("▸ [3]"), "{columns} × {rows}");
            assert!(frame.text.lines().count() <= rows);
            assert!(frame.text.lines().all(|line| styled_width(line) < columns));
        }
    }

    #[test]
    fn about_scrolls_from_the_overview_to_the_credits_in_a_small_terminal() {
        let menu = ["TuiQuake".into()];
        let display = Display {
            columns: 42,
            rows: 21,
            colors: true,
            map: Map::Braille,
        };
        let mut scroll = 0;
        let first = about(&menu, display, &mut scroll);
        assert!(first.text.contains("地震・速報を見る"));
        assert!(first.text.contains("Unicode Braille"));
        assert!(!first.text.contains("Glenn Chappell"));
        assert!(first.text.lines().count() <= display.rows);
        assert!(
            first
                .text
                .lines()
                .all(|line| styled_width(line) <= display.width())
        );
        scroll = usize::MAX;
        let last = about(&menu, display, &mut scroll);
        assert!(last.text.contains("データとクレジット"));
        assert!(last.text.contains("Glenn Chappell"));
        assert!(scroll > 0 && scroll < usize::MAX);
        assert!(last.text.lines().count() <= display.rows);
    }

    #[test]
    fn control_rows_wrap_whole_key_action_pairs() {
        let display = Display {
            columns: 28,
            rows: 24,
            colors: true,
            map: Map::Braille,
        };
        let lines = control_lines(
            "地図",
            &[
                ("WASD", "地図を移動"),
                ("+/-", "拡大/縮小"),
                ("Z", "全国/周辺"),
            ],
            display,
        );
        assert!(
            lines
                .iter()
                .all(|line| styled_width(line) <= display.width())
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("[+/-]") && line.contains("拡大/縮小"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("[Z]") && line.contains("全国/周辺"))
        );
        assert!(lines[0].contains("\x1b[38;2;125;213;210m[WASD]"));
    }

    #[test]
    fn interactive_eew_keeps_overview_and_warning_regions_separate() {
        let mut renderer = Renderer::new().unwrap();
        let mut bulletin = crate::data::demo().remove(1);
        let menu = ["TuiQuake".into()];
        for columns in [80, 48] {
            let display = Display {
                columns,
                rows: 28,
                colors: true,
                map: Map::Image(Image::Sixel((10, 20))),
            };
            let scene = || Scene {
                menu: &menu,
                eew_source: Some("Wolfx"),
                ..Scene::single(&bulletin, false, "")
            };
            let frame = renderer.render(scene(), display);
            let headline = frame.text.lines().nth(1).unwrap();
            assert!(headline.contains("緊急地震速報（警報） 第2報（最終報）"));
            assert!(headline.contains("\x1b[48;2;255;97;98m"));
            assert!(!headline.contains("Wolfx"));
            assert_eq!(styled_width(headline), columns - 1);
            let overview = frame.text.find("速報概要 · Wolfx").unwrap();
            let intensity = frame.text.find("推定最大").unwrap();
            let warning_areas = frame.text.find("警報対象地域").unwrap();
            assert!(overview < intensity && intensity < warning_areas);
            assert!(frame.text[intensity..warning_areas].contains(" ▄▄▄▄  ▄  "));
            assert!(frame.text[intensity..warning_areas].contains(" █    ▀█▀ "));
            assert!(frame.text[intensity..warning_areas].contains(" ▀▀▀█     "));
            assert!(frame.text[intensity..warning_areas].contains("\x1b[48;2;255;82;90m"));
            assert!(frame.text[intensity..warning_areas].contains("宮城県沖"));
            assert!(!frame.text[intensity..warning_areas].contains("震源 宮城県沖"));
            assert!(frame.text[intensity..warning_areas].contains("M6.1"));
            assert!(frame.text.lines().all(|line| styled_width(line) < columns));
            assert!(
                frame
                    .text
                    .lines()
                    .nth(frame.map_row - 2)
                    .unwrap()
                    .contains("地図")
            );
        }

        bulletin.maximum = Some(Level::Known(4));
        bulletin.warning = false;
        bulletin.warning_areas.clear();
        let frame = renderer.render(
            Scene {
                menu: &menu,
                ..Scene::single(&bulletin, false, "DEMO 架空 2/3")
            },
            Display {
                columns: 88,
                rows: 24,
                colors: false,
                map: Map::Braille,
            },
        );
        let headline = frame.text.lines().nth(1).unwrap();
        assert_eq!(headline.trim(), "緊急地震速報（予報） 第2報（最終報）");
        let indent = headline.len() - headline.trim_start().len();
        assert_eq!(indent, (87 - headline.trim().width()) / 2);
        assert!(frame.text.contains("DEMO 架空 2/3"));
        assert!(frame.text.contains("速報概要"));
        assert!(!frame.text.contains("警報対象地域"));
        assert!(frame.text.contains("地域別の予想震度"));
        assert!(frame.text.contains("5-〜5+"));
        assert!(frame.text.contains(" █    ▀▀▀ "));
        assert!(!frame.text.contains(" █    ▀█▀ "));
    }

    #[test]
    fn eew_banner_wraps_narrow_forecasts_and_does_not_color_cancellations() {
        let mut bulletin = crate::data::demo().remove(1);
        bulletin.warning = false;
        let rows = eew_banner(&bulletin, 20, true);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("緊急地震速報（予報）"));
        assert!(rows[1].contains("第2報（最終報）"));
        assert!(
            rows.iter()
                .all(|row| row.contains("\x1b[48;2;255;222;94m") && styled_width(row) == 20)
        );
        bulletin.eew_cancelled = true;
        let rows = eew_banner(&bulletin, 48, true);
        assert_eq!(rows[0].trim(), "緊急地震速報（取消） 第2報");
    }

    #[test]
    fn earthquake_overview_displays_the_badge_facts_and_event_notices() {
        let mut bulletin = crate::data::demo().pop().unwrap();
        bulletin.maximum = Some(Level::Known(0));
        let lines = earthquake_summary(
            &bulletin,
            QuakeNotices {
                eew: Some(true),
                tsunami: Some("心配なし"),
            },
            38,
            6,
            true,
        );
        assert!(lines[0].contains("最大震度"));
        assert!(lines[1].contains("\x1b[48;2;119;213;255m"));
        assert!(lines[1].contains("宮城県沖"));
        assert!(lines[2].contains("M6.1"));
        assert!(lines[3].contains("深さ 45 km"));
        assert!(lines[4].contains("EEW あり"));
        assert!(lines[5].contains("津波 心配なし"));
        assert!(lines.iter().all(|line| styled_width(line) <= 38));
    }

    #[test]
    fn menu_colors_forecasts_yellow_and_warnings_red() {
        let display = Display {
            columns: 80,
            rows: 24,
            colors: true,
            map: Map::Braille,
        };
        let mut item = MenuItem {
            key: '2',
            label: "EEW 予報",
            active: false,
            notice: Some(MenuNotice::Forecast),
        };
        assert!(
            menu_lines(&[item], display)
                .join("\n")
                .contains("\x1b[38;2;255;222;94m")
        );
        item = MenuItem {
            key: '2',
            label: "EEW 警報",
            active: true,
            notice: Some(MenuNotice::Warning),
        };
        assert!(
            menu_lines(&[item], display)
                .join("\n")
                .contains("\x1b[38;2;255;97;98m")
        );
    }

    #[test]
    fn eew_centers_unsigned_digits_in_a_fixed_width_badge() {
        let mut bulletin = crate::data::demo().remove(1);
        bulletin.maximum = Some(Level::Known(2));
        let three = eew_overview(&bulletin, 44, 16, true, None);
        assert!(three[2].contains("   ▄▄▄▄   \x1b[0m"));
        assert!(three[4].contains("   ▀▀▀█   \x1b[0m"));
        assert!(three[5].contains("   ▄▄▄█   \x1b[0m"));
        assert!(three[6].contains("\x1b[38;2;244;232;113m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));
        assert!(!three[6].contains("\x1b[48;"));

        bulletin.maximum = Some(Level::Known(3));
        let four = eew_overview(&bulletin, 44, 16, true, None);
        assert!(four[4].contains("  ▀▀▀█▀▀  \x1b[0m"));
        assert!(four[6].contains("\x1b[38;2;255;166;72m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));

        bulletin.maximum = Some(Level::Known(0));
        let one = eew_overview(&bulletin, 44, 16, true, None);
        assert!(one[5].contains("   ▄▄█▄   \x1b[0m"));

        bulletin.maximum = Some(Level::Known(5));
        let five_plus = eew_overview(&bulletin, 44, 16, true, None);
        assert!(five_plus[6].contains("\x1b[38;2;255;82;90m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));
    }

    #[test]
    fn monochrome_text_can_have_a_sixel_map() {
        let mut renderer = Renderer::new().unwrap();
        let bulletin = crate::data::demo().remove(0);
        let frame = renderer.render(
            Scene::single(&bulletin, false, "DEMO"),
            Display {
                columns: 80,
                rows: 24,
                colors: false,
                map: Map::Image(Image::Sixel((10, 20))),
            },
        );
        assert!(frame.graphic.starts_with("\x1bP7;1q\"1;1;440;"));
        assert!(frame.text.contains("警報対象地域"));
    }

    #[test]
    fn kitty_keeps_a_text_map_when_the_image_is_filtered() {
        let mut renderer = Renderer::new().unwrap();
        let bulletin = crate::data::demo().remove(0);
        let frame = renderer.render(
            Scene::single(&bulletin, true, "DEMO"),
            Display {
                columns: 80,
                rows: 24,
                colors: true,
                map: Map::Image(Image::Kitty),
            },
        );
        assert!(frame.graphic.starts_with(graphics::KITTY_COMMAND));
        assert!(frame.text.contains(|glyph| QUADRANTS[1..].contains(&glyph)));
    }

    #[test]
    fn quadrant_map_represents_both_sides_of_a_cell() {
        let [land, sea] = [map::LAND, map::SEA].map(|color| [color; SAMPLES_PER_QUADRANT as usize]);
        assert_eq!(text_cell([land, sea, land, sea], None, false).0, '▌');
        assert_eq!(text_cell([sea, land, sea, land], None, false).0, '▐');
        let (glyph, ink, paper) = text_cell([land, sea, land, sea], None, true);
        assert_eq!(
            (glyph, ink, paper),
            ('▌', map_ink(map::LAND), map_ink(map::SEA))
        );
        assert_eq!(glyph.width(), Some(1));
    }

    #[test]
    fn braille_places_dots_in_one_terminal_cell() {
        let [land, sea] = [map::LAND, map::SEA].map(|color| [color; 4]);
        assert_eq!(
            braille_cell([land, sea, land, sea, land, sea, land, sea], true).0,
            '⡇'
        );
        let right = braille_cell([sea, land, sea, land, sea, land, sea, land], false).0;
        assert_eq!(right, '⢸');
        assert_eq!(right.width(), Some(1));
    }

    #[test]
    fn text_map_keeps_observations_and_eew_visible() {
        let land = [[map::LAND; SAMPLES_PER_QUADRANT as usize]; 4];
        assert_eq!(
            text_cell(land, Some(map::EPICENTER), true),
            ('✚', map_ink(map::EPICENTER), PALETTE[map::LAND as usize])
        );
        assert_eq!(text_cell(land, Some(map::STATION + 1), false).0, '●');
        let mut warning = [[map::SEA; SAMPLES_PER_QUADRANT as usize]; 4];
        warning[3][7] = map::WARNING;
        assert_eq!(text_cell(warning, None, false).0, '█');
    }

    #[test]
    fn switching_to_stations_draws_dots_without_previous_region_labels() {
        let mut renderer = Renderer::new().unwrap();
        let bulletin = crate::data::demo().pop().unwrap();
        for map in [Map::Braille, Map::Image(Image::Sixel((10, 20)))] {
            let display = Display {
                columns: 100,
                rows: 30,
                colors: false,
                map,
            };
            let mut scene = Scene::single(&bulletin, false, "TEST");
            scene.layer = Layer::Regions;
            renderer.render(scene, display);
            let raster = if map.image().is_some() {
                &renderer.image_raster
            } else {
                &renderer.raster
            };
            assert!(raster.labels.iter().any(Option::is_some));
            let frame = renderer.render(Scene::single(&bulletin, false, "TEST"), display);
            if map.image().is_some() {
                assert!(frame.graphic.starts_with("\x1bP"));
                assert!(renderer.image_raster.labels.is_empty());
                assert!(
                    renderer
                        .image_raster
                        .pixels
                        .iter()
                        .any(|&pixel| pixel >= map::STATION)
                );
            } else {
                assert!(frame.text.contains('●'));
                assert!(renderer.raster.labels.is_empty());
            }
        }
    }

    #[test]
    fn national_region_map_includes_intensity_labels() {
        let mut renderer = Renderer::new().unwrap();
        let bulletin = crate::data::demo().pop().unwrap();
        let mut scene = Scene::single(&bulletin, true, "TEST");
        scene.layer = Layer::Regions;
        renderer.render(
            scene,
            Display {
                columns: 112,
                rows: 34,
                colors: false,
                map: Map::Text,
            },
        );
        assert!(
            renderer
                .raster
                .labels
                .iter()
                .flatten()
                .any(|label| label.glyph.is_ascii_digit())
        );
    }

    #[test]
    fn header_groups_event_facts_and_wraps_timestamps_on_a_narrow_terminal() {
        let bulletin = crate::data::demo().pop().unwrap();
        let wide = event_header(&bulletin, "地震 1/100 · Wolfx", 79, false);
        assert_eq!(wide[0], "TuiQuake  地震 1/100 · Wolfx");
        assert!(wide[1].contains("最大震度 5弱"));
        assert!(wide[2].contains("M6.1  ·  深さ 45 km"));
        assert_eq!(wide[3], "発生 01/01 14:32:00   発表 14:34:00 JST");
        let narrow = event_header(&bulletin, "", 25, false);
        assert_eq!(
            &narrow[narrow.len() - 2..],
            ["発生 01/01 14:32:00 JST", "発表 01/01 14:34:00 JST"]
        );
    }

    #[test]
    fn earthquake_sidebar_shows_observations_and_selected_history_in_a_standard_terminal() {
        let mut renderer = Renderer::new().unwrap();
        let report = Report {
            id: "event".into(),
            bulletin: Bulletin {
                name: "三陸沖".into(),
                maximum: Some(crate::data::Level::Known(2)),
                regions: vec![crate::data::Observation {
                    level: crate::data::Level::Known(2),
                    name: "宮城県北部".into(),
                    location: None,
                    area_code: String::new(),
                    prefecture: None,
                }],
                ..Bulletin::default()
            },
        };
        let menu = ["TuiQuake  [1 地震]  2 EEW  3 津波  4 デモ".into()];
        let reports = [report];
        let mut scene = Scene::single(&reports[0].bulletin, true, "");
        scene.menu = &menu;
        scene.layer = Layer::Regions;
        scene.history = Some(History {
            reports: &reports,
            selected: 0,
            status: "",
            row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
        });
        let frame = renderer.render(
            scene,
            Display {
                columns: 80,
                rows: 24,
                colors: false,
                map: Map::Braille,
            },
        );
        assert!(
            frame
                .text
                .lines()
                .any(|line| line.contains("╭─ 地震の概要"))
        );
        assert!(frame.text.lines().any(|line| line.contains("宮城県北部")));
        assert!(
            frame
                .text
                .lines()
                .any(|line| line.contains("│ >") && line.contains("三陸沖"))
        );
        assert!(frame.text.lines().all(|line| line.width() < 80));
        assert!(frame.text.lines().count() <= 24);
        let frame = renderer.render(
            Scene {
                menu: &menu,
                history: Some(History {
                    reports: &reports,
                    selected: 0,
                    status: "",
                    row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
                }),
                ..Scene::single(&reports[0].bulletin, true, "")
            },
            Display {
                columns: 59,
                rows: 24,
                colors: true,
                map: Map::Braille,
            },
        );
        assert!(frame.text.lines().all(|line| styled_width(line) <= 58));
    }

    #[test]
    fn history_stays_in_place_and_colors_each_known_intensity() {
        use crate::data::{Level, Observation};

        let quiet = Bulletin::default();
        let observed = Bulletin {
            name: "紀伊水道".into(),
            maximum: Some(Level::Known(8)),
            stations: vec![
                Observation {
                    name: "観測点".into(),
                    level: Level::Known(2),
                    location: None,
                    area_code: String::new(),
                    prefecture: None,
                };
                25
            ],
            ..Bulletin::default()
        };
        let reports = [
            Report {
                id: "1".into(),
                bulletin: observed.clone(),
            },
            Report {
                id: "2".into(),
                bulletin: quiet.clone(),
            },
        ];
        let menu = ["TuiQuake".into()];
        let display = Display {
            columns: 100,
            rows: 38,
            colors: true,
            map: Map::Braille,
        };
        let mut renderer = Renderer::new().unwrap();
        let mut render = |bulletin, selected, reading_start| {
            let scene = Scene {
                menu: &menu,
                history: Some(History {
                    reports: &reports,
                    selected,
                    status: "",
                    row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
                }),
                reading_start,
                ..Scene::single(bulletin, true, "")
            };
            renderer.render(scene, display).text.clone()
        };
        let with_readings = render(&observed, 0, 0);
        let at_end = render(&observed, 0, 24);
        let without_readings = render(&quiet, 1, 0);
        let history_row = |text: &str| {
            text.lines()
                .position(|line| line.contains("地震履歴"))
                .unwrap()
        };
        assert_eq!(history_row(&with_readings), history_row(&at_end));
        assert_eq!(history_row(&with_readings), history_row(&without_readings));
        let seven = with_readings
            .lines()
            .find(|line| line.contains("紀伊水道") && line.contains(">"))
            .unwrap();
        assert!(seven.contains("\x1b[48;2;158;95;235m\x1b[38;2;0;0;0m 7  "));
        assert!(seven.matches("\x1b[48;2;37;70;87m").count() >= 2);
        let unknown = without_readings
            .lines()
            .find(|line| line.contains("震源地不明") && line.contains(">"))
            .unwrap();
        assert!(!unknown.contains("\x1b[48;2;158;95;235m"));
        assert_eq!(styled_width(seven), 99);
    }

    #[test]
    fn readings_fill_the_space_above_bottom_aligned_history() {
        let bulletin = crate::data::demo().pop().unwrap();
        let reports = [Report {
            id: "event".into(),
            bulletin: bulletin.clone(),
        }];
        let render = |row_limit, status| {
            earthquake_sidebar(
                &bulletin,
                QuakeNotices::default(),
                History {
                    reports: &reports,
                    selected: 0,
                    status,
                    row_limit,
                },
                Layer::Stations,
                StationGrouping::Intensity,
                0,
                60,
                38,
                false,
            )
        };
        let normal = render(10, "");
        let short = render(4, "");
        let retrying = render(10, "Wolfx 再接続中…");
        let tall_history = render(50, "");
        let top = |rows: &[String], name: &str| {
            rows.iter()
                .position(|line| line.starts_with('╭') && line.contains(name))
                .unwrap()
        };
        let readings_bounds = |rows: &[String]| {
            let start = top(rows, "観測点");
            let end = rows
                .iter()
                .enumerate()
                .skip(start + 1)
                .find(|(_, line)| line.starts_with('╰'))
                .unwrap()
                .0;
            (start, end)
        };
        let (normal_top, normal_bottom) = readings_bounds(&normal);
        assert_eq!(readings_bounds(&short), (normal_top, normal_bottom + 6));
        assert_eq!(readings_bounds(&retrying), (normal_top, normal_bottom));
        assert_eq!(readings_bounds(&tall_history), (normal_top, normal_top + 3));
        for rows in [normal, short, retrying, tall_history] {
            assert_eq!(rows.len(), 38);
            assert_eq!(readings_bounds(&rows).1 + 1, top(&rows, "地震履歴"));
            assert!(rows[37].starts_with('╰'));
        }
    }

    #[test]
    fn wide_map_and_earthquake_sidebar_use_the_available_width() {
        let mut renderer = Renderer::new().unwrap();
        let bulletin = crate::data::demo().pop().unwrap();
        let reports: Vec<_> = (0..50)
            .map(|i| Report {
                id: i.to_string(),
                bulletin: Bulletin {
                    name: format!("試験地震 {i:02}"),
                    ..bulletin.clone()
                },
            })
            .collect();
        let mut scene = Scene::single(&bulletin, true, "地震 1/50");
        let menu = ["TuiQuake  [1 地震]  2 EEW  3 津波  4 デモ".into()];
        scene.menu = &menu;
        scene.station_grouping = StationGrouping::Prefecture;
        scene.history = Some(History {
            reports: &reports,
            selected: 0,
            status: "",
            row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
        });
        let frame = renderer.render(
            scene,
            Display {
                columns: 160,
                rows: 48,
                colors: false,
                map: Map::Image(Image::Sixel((10, 20))),
            },
        );
        assert!(frame.graphic.starts_with("\x1bP7;1q\"1;1;730;840"));
        assert_eq!(frame.map_row, 4);
        assert_eq!(frame.map_col, 2);
        assert!(frame.text.lines().next().unwrap().contains("3 津波"));
        let top = frame.text.lines().nth(frame.map_row - 2).unwrap();
        assert!(top.starts_with("╭─ 地図 · 全国"));
        assert!(top.contains("╭─ 地震の概要"));
        assert_eq!(top.width(), 159);
        assert!(
            frame
                .text
                .lines()
                .any(|line| line.contains("╭─ 各地の震度 · 観測点"))
        );
        assert!(frame.text.lines().any(|line| line.contains("│ ── 宮城県")));
        assert!(frame.text.lines().any(|line| line.contains("╭─ 地震履歴")));
        assert!(
            frame
                .text
                .lines()
                .any(|line| line.starts_with('╰') && line.matches('╰').count() == 2)
        );
        let entries = frame
            .text
            .lines()
            .filter(|line| line.contains("試験地震"))
            .count();
        assert_eq!(entries, 10);
        let compact = compact(
            History {
                reports: &reports,
                selected: 20,
                status: "",
                row_limit: 4,
            },
            Display {
                columns: 50,
                rows: 25,
                colors: false,
                map: Map::Text,
            },
            &menu,
        );
        assert_eq!(
            compact
                .text
                .lines()
                .filter(|line| line.contains("試験地震"))
                .count(),
            4
        );
        assert!(
            compact
                .text
                .lines()
                .any(|line| line.contains("│ >") && line.contains("試験地震 20"))
        );
    }
}
