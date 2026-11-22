use std::fmt::Write;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    data::{Bulletin, ForecastArea, INTENSITIES, Level, Observation, Report},
    graphics::Map,
    map::{self, Layer, PALETTE, Rgb, View},
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

#[derive(Clone, Copy)]
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

pub struct NoticePanel<'a> {
    pub title: &'a str,
    pub status: &'a str,
    pub alert: bool,
    pub lines: &'a [String],
    pub controls: &'a str,
}

pub fn panel(
    menu: &[String],
    notice: NoticePanel<'_>,
    display: Display,
    scroll: &mut usize,
) -> Frame {
    let NoticePanel {
        title,
        status,
        alert,
        lines,
        controls,
    } = notice;
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
                "通常配信  Wolfx Project",
                "予備配信  P2P地震情報",
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

mod panels;
mod raster;
mod render;
#[cfg(test)]
mod tests;

pub use panels::detail;
pub use render::Renderer;
