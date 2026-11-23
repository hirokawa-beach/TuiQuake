use std::fmt::Write;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    data::{Bulletin, ForecastArea, INTENSITIES, Level, Observation, Report},
    graphics::Map,
    map::{self, Layer, PALETTE, Rgb, View},
};

use crate::settings::{AUTO_HISTORY_ROWS, StationGrouping};

pub const MIN_SPLIT_WIDTH: usize = 58;
pub const APP_NAME: &str = "TuiQuake";
const MAP_MAX_COLUMNS: usize = 44;
const SIDEBAR_GAP: usize = 3;
const INK: map::Rgb = (220, 239, 237);
const MUTED: map::Rgb = (136, 173, 182);
const MENU_KEY: Rgb = (111, 143, 151);
const ACCENT: map::Rgb = (125, 213, 210);
const ALERT: map::Rgb = (255, 175, 97);
const FORECAST_INK: Rgb = (255, 222, 94);
const WARNING_INK: Rgb = (255, 97, 98);
const FRAME: Rgb = (71, 100, 108);
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
    pub warning_seconds: Option<u64>,
    pub demo: bool,
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
            warning_seconds: None,
            demo: false,
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
    title_rgb: Rgb,
    ansi: bool,
}

impl Border {
    fn new(width: usize, ansi: bool) -> Self {
        Self {
            width,
            rgb: FRAME,
            title_rgb: INK,
            ansi,
        }
    }

    fn notice(width: usize, ansi: bool, alert: bool) -> Self {
        if alert {
            Self {
                rgb: FRAME_ALERT,
                title_rgb: ALERT,
                ..Self::new(width, ansi)
            }
        } else {
            Self::new(width, ansi)
        }
    }

    fn edge(self) -> String {
        color("│", self.rgb, self.ansi)
    }

    fn top(self, title: &str) -> String {
        let title = fit(title, self.width.saturating_sub(6));
        let rest = "─".repeat(self.width - 5 - title.width());
        let title = color(&title, self.title_rgb, self.ansi);
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

const SHALLOW_DEPTH: &str = "ごく浅い";

fn depth_label(km: u32) -> String {
    if km == 0 {
        SHALLOW_DEPTH.into()
    } else {
        format!("{km} km")
    }
}

const PLUM_HYPOCENTER: &str = "PLUM法による仮定震源";

fn plum_hypocenter_label(width: usize) -> &'static str {
    if width >= PLUM_HYPOCENTER.width() {
        PLUM_HYPOCENTER
    } else {
        "PLUM仮定震源"
    }
}

fn magnitude_and_depth(bulletin: &Bulletin) -> String {
    let mut facts = Vec::new();
    if !bulletin.magnitude.is_empty() {
        facts.push(format!("M{}", bulletin.magnitude));
    }
    if let Some(depth) = bulletin.depth {
        facts.push(format!("深さ {}", depth_label(depth)));
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
    let facts = if b.plum_assumed_hypocenter {
        plum_hypocenter_label(width).to_owned()
    } else {
        magnitude_and_depth(b)
    };
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
        Layer::Regions => "地域別・最大観測震度",
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

fn observation_groups(
    observations: &[Observation],
    grouping: StationGrouping,
) -> Vec<(&str, Vec<&Observation>)> {
    const UNKNOWN: &str = "都道府県不明";
    let mut groups: Vec<(&str, Vec<&Observation>)> = Vec::new();
    // Observation order is already strongest first, including within groups.
    for station in observations {
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

fn prefecture_heading(prefecture: &str) -> ReadingLine {
    ReadingLine::Text(format!("── {prefecture}"), MUTED)
}

fn readings(
    b: &Bulletin,
    layer: Layer,
    grouping: StationGrouping,
    width: usize,
) -> Vec<ReadingLine> {
    match (layer, grouping) {
        (Layer::Regions, _) => regional_window(b, 0, usize::MAX).2,
        (Layer::Stations, StationGrouping::Intensity) => {
            intensity_readings(b, width, 0, usize::MAX).2
        }
        (Layer::Stations, StationGrouping::Prefecture) => observation_groups(&b.stations, grouping)
            .into_iter()
            .flat_map(|(label, stations)| {
                std::iter::once(prefecture_heading(label))
                    .chain(stations.into_iter().map(prefecture_station_row))
            })
            .collect(),
    }
}

fn prefecture_station_row(station: &Observation) -> ReadingLine {
    let (line, ink) = reading_row(station);
    ReadingLine::StationNames(line, ink)
}

fn station_readings_window(
    b: &Bulletin,
    grouping: StationGrouping,
    start: usize,
    slots: usize,
    width: usize,
) -> (String, Vec<ReadingLine>) {
    let (title, start, shown, rows) = match grouping {
        StationGrouping::Intensity => {
            let (start, shown, rows) = intensity_readings(b, width, start, slots);
            ("観測点 · 震度別", start, shown, rows)
        }
        StationGrouping::Prefecture => {
            let (start, shown, rows) = prefecture_window(b, start, slots);
            ("各地の震度 · 観測点", start, shown, rows)
        }
    };
    let mut title = title.to_owned();
    let count = b.stations.len();
    if shown != 0 && shown < count {
        write!(title, "  {}–{}/{count}", start + 1, start + shown).unwrap();
    }
    (title, rows)
}

fn prefecture_window(b: &Bulletin, start: usize, slots: usize) -> (usize, usize, Vec<ReadingLine>) {
    grouped_window(
        &b.stations,
        StationGrouping::Prefecture,
        start,
        slots,
        |label, _| prefecture_heading(label),
        prefecture_station_row,
    )
}

fn regional_window(b: &Bulletin, start: usize, slots: usize) -> (usize, usize, Vec<ReadingLine>) {
    grouped_window(
        &b.regions,
        StationGrouping::Intensity,
        start,
        slots,
        |label, regions| intensity_heading(label, &regions[0].level, regions.len(), "地域"),
        |region| ReadingLine::Text(format!("  {}", region.name), INK),
    )
}

fn grouped_window(
    observations: &[Observation],
    grouping: StationGrouping,
    start: usize,
    slots: usize,
    heading: impl Fn(&str, &[&Observation]) -> ReadingLine,
    row: impl Fn(&Observation) -> ReadingLine,
) -> (usize, usize, Vec<ReadingLine>) {
    let count = observations.len();
    let groups = observation_groups(observations, grouping);
    let start = if count + groups.len() <= slots {
        0
    } else {
        start.min(count.saturating_sub(1))
    };
    let mut skip = start;
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
        rows.push(heading(label, &stations));
        let visible = stations.into_iter().skip(skip).take(slots - rows.len());
        shown += visible.len();
        rows.extend(visible.map(&row));
        skip = 0;
    }
    (start, shown, rows)
}

fn section_heading(title: &str, width: usize, ansi: bool) -> String {
    let title = fit(title, width - 4);
    let rule = "─".repeat(width - 4 - title.width());
    format!(
        "{}{} {}",
        color("── ", FRAME, ansi),
        color(&title, INK, ansi),
        color(&rule, FRAME, ansi),
    )
}

impl History<'_> {
    fn title(&self) -> String {
        if self.reports.is_empty() {
            "地震履歴".into()
        } else {
            format!("地震履歴  {}/{}", self.selected + 1, self.reports.len())
        }
    }

    fn target_height(&self, available: usize) -> usize {
        if self.row_limit == AUTO_HISTORY_ROWS {
            available
        } else {
            self.row_limit.saturating_add(2).min(available)
        }
    }

    fn section(self, width: usize, height: usize, ansi: bool) -> Vec<String> {
        let height = self.target_height(height);
        let mut title = self.title();
        if height <= 2 && !self.reports.is_empty() && !self.status.is_empty() {
            write!(title, " · {}", self.status).unwrap();
        }
        let mut lines = vec![section_heading(&title, width, ansi)];
        lines.extend(history_lines(self, width, height - 1, ansi));
        lines.resize_with(height, String::new);
        lines
    }

    fn bottom_section(self, width: usize, height: usize, ansi: bool) -> Vec<String> {
        let section = self.section(width, height, ansi);
        std::iter::repeat_with(String::new)
            .take(height - section.len())
            .chain(section)
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
    const MAG_WIDTH: usize = 5;
    const DEPTH_WIDTH: usize = 9;
    let date_width = if width >= 36 { 11 } else { 5 };
    let show_mag = width >= 40;
    let show_depth = width >= 28;
    let header = format!(
        "{}{} 震度 {}{}震源地",
        if ansi { "" } else { "  " },
        pad(
            if date_width == 11 {
                "発生 JST"
            } else {
                "発生"
            },
            date_width
        ),
        if show_mag {
            pad("M", MAG_WIDTH)
        } else {
            String::new()
        },
        if show_depth {
            pad("深さ", DEPTH_WIDTH)
        } else {
            String::new()
        },
    );
    if rows > out.len() + 1 {
        out.push(color(&fit(&header, width), MUTED, ansi));
    }
    let available = rows.saturating_sub(out.len());
    let count = if row_limit == AUTO_HISTORY_ROWS {
        available
    } else {
        available.min(row_limit)
    };
    let start = selected
        .saturating_sub(count / 2)
        .min(reports.len().saturating_sub(count));
    let mut previous_day = None;
    for (index, report) in reports.iter().enumerate().skip(start).take(count) {
        let item = &report.bulletin;
        let date = item
            .origin
            .map(|stamp| {
                let day = stamp.date_naive();
                let format = if date_width == 5 {
                    "%H:%M"
                } else if previous_day == Some(day) {
                    "      %H:%M"
                } else {
                    "%m/%d %H:%M"
                };
                previous_day = Some(day);
                stamp.format(format).to_string()
            })
            .unwrap_or_default();
        let magnitude = if show_mag {
            format!(
                "{:>digits$} ",
                fit(&item.magnitude, MAG_WIDTH - 1),
                digits = MAG_WIDTH - 1
            )
        } else {
            String::new()
        };
        let depth = if show_depth {
            let value = match item.depth {
                Some(0) => SHALLOW_DEPTH.into(),
                Some(km) => format!("{km}km"),
                None => "—".into(),
            };
            format!(
                "{}{value} ",
                " ".repeat(DEPTH_WIDTH.saturating_sub(value.width() + 1))
            )
        } else {
            String::new()
        };
        let level = item.maximum.as_ref();
        let selected = index == selected;
        let cursor = if ansi {
            ""
        } else if selected {
            "> "
        } else {
            "  "
        };
        let prefix = format!("{cursor}{} ", pad(&date, date_width));
        let intensity = fit(level.map_or("—", short_intensity_label), 4);
        let left = (4 - intensity.width()) / 2;
        let cell = pad(&format!("{}{intensity}", " ".repeat(left)), 4);
        let suffix = format!(
            " {magnitude}{depth}{}",
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
            let intensity_color = PALETTE[(map::STATION + index) as usize];
            let foreground = if index >= 2 {
                map::rgb(&mut line, intensity_color, true);
                badge_ink(intensity_color)
            } else {
                intensity_color
            };
            map::rgb(&mut line, foreground, false);
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

fn control_lines(heading: &str, bindings: &[(&str, &str)], display: Display) -> Vec<String> {
    let width = display.width();
    let heading = format!("{heading}  ");
    let indent = heading.width();
    let mut line = heading;
    let mut used = indent;
    let mut has_item = false;
    let mut lines = Vec::new();
    for (key, action) in bindings {
        let key = format!("[{key}]");
        let item_width = key.width() + 1 + action.width();
        let separator = if has_item { "  " } else { "" };
        if used + separator.len() + item_width > width {
            lines.push(color(&line, MUTED, display.colors));
            line.clear();
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
        write!(line, "{key} {action}").unwrap();
        used += key.width() + 1 + action.width();
        has_item = true;
    }
    lines.push(color(&line, MUTED, display.colors));
    lines
}

pub const QUAKE_KEYS: [&str; 2] = ["↑↓ 選択", "B 地図へ"];

pub fn footer(actions: &[&str], display: Display) -> String {
    let width = display.width();
    let full = "H ヘルプ  Q 終了";
    let tail = if full.width() <= width {
        full
    } else {
        "H ヘルプ  Q"
    };
    let mut line = String::new();
    let mut remaining = width - tail.width();
    for action in actions {
        let used = action.width() + 2;
        if used <= remaining {
            write!(line, "{action}  ").unwrap();
            remaining -= used;
        }
    }
    line.push_str(tail);
    color(&line, MUTED, display.colors)
}

pub fn compact(history: History<'_>, display: Display, menu: &[String]) -> Frame {
    let mut list = menu.to_vec();
    list.extend(history.section(
        display.width(),
        display.rows.saturating_sub(menu.len() + 1).max(2),
        display.colors,
    ));
    list.push(footer(&QUAKE_KEYS, display));
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
    pub notes: &'a [String],
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
        notes,
    } = notice;
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let mut text = menu.to_vec();
    let mut content: Vec<_> = wrap(status, inner)
        .into_iter()
        .map(|line| color(&line, if alert { ALERT } else { INK }, display.colors))
        .collect();
    let rows: Vec<_> = lines
        .iter()
        .flat_map(|line| wrap(line, inner))
        .chain(
            notes
                .iter()
                .flat_map(|line| wrap(line, inner))
                .map(|line| color(&line, MUTED, display.colors)),
        )
        .collect();
    let capacity = display.rows.saturating_sub(menu.len() + content.len() + 3);
    *scroll = (*scroll).min(rows.len().saturating_sub(capacity));
    content.extend(rows.iter().skip(*scroll).take(capacity).cloned());
    let height = content.len() + 2;
    let title = if capacity > 0 && rows.len() > capacity {
        format!(
            "{title}  {}–{}/{}",
            *scroll + 1,
            *scroll + capacity,
            rows.len()
        )
    } else {
        title.to_owned()
    };
    text.extend(Border::notice(width, display.colors, alert).text(content, &title, height));
    let actions: &[&str] = if capacity > 0 && rows.len() > capacity {
        &["↑↓ スクロール"]
    } else {
        &[]
    };
    text.push(footer(actions, display));
    Frame {
        text: text.join("\n"),
        ..Frame::default()
    }
}

pub struct SettingChoice {
    pub label: std::borrow::Cow<'static, str>,
    pub selected: bool,
    pub supported: bool,
}

pub struct SettingRow {
    pub label: &'static str,
    pub description: &'static str,
    pub hint: Option<&'static str>,
    pub choices: Vec<SettingChoice>,
}

pub fn settings(
    menu: &[String],
    entries: &[SettingRow],
    selected: usize,
    active_map: &str,
    notice: Option<(&str, bool)>,
    display: Display,
) -> Frame {
    let width = display.width().min(88);
    let inner = width - TEXT_FRAME_MARGIN;
    let capacity = display.rows.saturating_sub(menu.len() + 3);
    let label_width = entries
        .iter()
        .map(|entry| entry.label.width())
        .max()
        .unwrap_or_default();
    let mut content = Vec::new();
    let mut selected_end = 0;
    for (index, entry) in entries.iter().enumerate() {
        let active = index == selected;
        content.extend(setting_lines(
            entry,
            active,
            label_width,
            inner,
            display.colors,
        ));
        if index == 0 {
            content.push(color(
                &fit(&format!("  使用中  {active_map}"), inner),
                MUTED,
                display.colors,
            ));
        }
        if active {
            selected_end = content.len();
        }
    }
    let mut details = Vec::new();
    if let Some((message, error)) = notice {
        details.push(color(
            &fit(message, inner),
            if error { ALERT } else { ACCENT },
            display.colors,
        ));
    }
    if let Some(entry) = entries.get(selected) {
        details.extend(
            wrap(entry.description, inner)
                .into_iter()
                .take(2)
                .map(|line| color(&line, MUTED, display.colors)),
        );
    }
    details.truncate(capacity.saturating_sub(1));
    let rows = capacity.saturating_sub(details.len());
    let start = selected_end.saturating_sub(rows);
    let mut visible: Vec<_> = content.into_iter().skip(start).take(rows).collect();
    visible.extend(details);
    let height = visible.len() + 2;
    let mut text = menu.to_vec();
    text.extend(Border::new(width, display.colors).text(visible, "設定", height));
    let action = if entries
        .get(selected)
        .is_some_and(|row| row.choices.is_empty())
    {
        "Enter 開く"
    } else {
        "←→/Space 変更"
    };
    text.push(footer(&["↑↓ 選択", action, "Esc 戻る"], display));
    Frame {
        text: text.join("\n"),
        ..Frame::default()
    }
}

fn setting_lines(
    entry: &SettingRow,
    active: bool,
    label_width: usize,
    width: usize,
    colors: bool,
) -> Vec<String> {
    let marker = if active { '▸' } else { ' ' };
    if entry.choices.is_empty() {
        let label = format!("{marker} {}", pad(entry.label, label_width));
        let mut row = color(&fit(&label, width), INK, colors);
        if let Some(hint) = entry
            .hint
            .filter(|hint| label.width() + 2 + hint.width() <= width)
        {
            row.push_str("  ");
            row.push_str(&color(hint, ACCENT, colors));
        }
        return vec![row];
    }
    let mut rows = Vec::new();
    let indent = if width >= 56 {
        label_width + 4
    } else {
        rows.push(color(
            &fit(&format!("{marker} {}", entry.label), width),
            INK,
            colors,
        ));
        2
    };
    let mut line = if indent == 2 {
        " ".repeat(indent)
    } else {
        color(
            &format!("{marker} {}  ", pad(entry.label, label_width)),
            INK,
            colors,
        )
    };
    let mut used = indent;
    for choice in &entry.choices {
        let available = width.saturating_sub(indent);
        let label = if choice.supported {
            fit(
                &choice.label,
                available.saturating_sub(usize::from(choice.selected) * 2),
            )
        } else {
            let name = fit(
                &choice.label,
                available.saturating_sub(2 + usize::from(choice.selected) * 2),
            );
            format!("{name} ×")
        };
        let label = if choice.selected {
            format!("[{label}]")
        } else {
            label
        };
        let option_width = label.width();
        if used > indent && used + 3 + option_width > width {
            rows.push(std::mem::take(&mut line));
            line.push_str(&" ".repeat(indent));
            used = indent;
        }
        if used > indent {
            line.push_str(&color(" │ ", FRAME, colors));
            used += 3;
        }
        if choice.selected && colors {
            map::rgb(&mut line, SELECTED, true);
        }
        let ink = if choice.supported { INK } else { MUTED };
        line.push_str(&color(&label, ink, colors));
        used += option_width;
    }
    if let Some(hint) = entry.hint.filter(|hint| used + 3 + hint.width() <= width) {
        line.push_str("   ");
        line.push_str(&color(hint, MUTED, colors));
    }
    rows.push(line);
    rows
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
    let mut rows = Vec::new();
    let mut line = format!(" {}", compact_logo(display.colors));
    let mut used = 1 + APP_NAME.width();
    for item in items {
        let key = format!("{} ", item.key);
        let dot = if item.notice.is_some() { "● " } else { "" };
        let label = fit(
            item.label,
            width.saturating_sub(key.width() + dot.width() + 2),
        );
        let tab_width = key.width() + dot.width() + label.width() + 2;
        if used > 0 && used + 1 + tab_width > width {
            rows.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push(' ');
            used += 1;
        }
        if display.colors {
            if item.active {
                map::rgb(&mut line, SELECTED, true);
            }
            line.push(' ');
            map::rgb(&mut line, MENU_KEY, false);
            line.push_str(&key);
            let ink = match item.notice {
                Some(MenuNotice::Forecast) => FORECAST_INK,
                Some(MenuNotice::Warning) => WARNING_INK,
                None if item.active => INK,
                None => MUTED,
            };
            map::rgb(&mut line, ink, false);
            write!(line, "{dot}{label} {RESET}").unwrap();
        } else if item.active {
            write!(line, "[{key}{dot}{label}]").unwrap();
        } else {
            write!(line, " {key}{dot}{label} ").unwrap();
        }
        used += tab_width;
    }
    rows.push(line);
    rows
}

pub struct FeedStatus {
    pub connection: &'static str,
    pub online: bool,
    pub updated: Option<crate::data::Stamp>,
    pub eew: (&'static str, Option<MenuNotice>),
    pub tsunami: (&'static str, Option<MenuNotice>),
}

pub fn append_feed_status(rows: &mut Vec<String>, status: FeedStatus, display: Display) {
    let updated = status
        .updated
        .map(|stamp| format!("{} 更新", stamp.format("%H:%M")));
    let tone = |notice| match notice {
        Some(MenuNotice::Warning) => WARNING_INK,
        Some(MenuNotice::Forecast) => FORECAST_INK,
        None => MUTED,
    };
    let fields = [
        (
            status.connection,
            if status.online {
                (142, 211, 176)
            } else {
                MUTED
            },
        ),
        (updated.as_deref().unwrap_or_default(), MUTED),
        (status.eew.0, tone(status.eew.1)),
        (status.tsunami.0, tone(status.tsunami.1)),
    ];
    let width = display.width();
    let total = fields
        .iter()
        .map(|(label, _)| label.width() + 2)
        .sum::<usize>()
        - if updated.is_some() { 2 } else { 4 };
    let mut line = String::new();
    let mut used = 0;
    if let Some(last) = rows.last_mut()
        && styled_width(last) + 3 + total <= width
    {
        line = std::mem::take(last);
        rows.pop();
        used = width - total - 2;
        line.push_str(&" ".repeat(used - styled_width(&line)));
    }
    for (label, ink) in fields.into_iter().filter(|(label, _)| !label.is_empty()) {
        let label = fit(label, width);
        if used > 0 && used + 2 + label.width() > width {
            rows.push(std::mem::take(&mut line));
            used = 0;
        }
        if used > 0 {
            line.push_str("  ");
            used += 2;
        }
        used += label.width();
        line.push_str(&color(&label, ink, display.colors));
    }
    rows.push(line);
    rows.push(color(&"─".repeat(width), FRAME, display.colors));
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
            let prefix = format!("{marker} [{}] ", index + 1);
            let available = inner.saturating_sub(prefix.width());
            let label = if map == Map::Braille && available < map.label().width() {
                let short = "Braille 非推奨";
                if available >= short.width() {
                    short
                } else {
                    "Braille"
                }
            } else {
                map.label()
            };
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
            let describe = descriptions && map != Map::Braille;
            let name_width = if describe { 18 } else { available };
            line.push_str(&pad(label, name_width));
            if describe {
                let note = if map.image().is_some() {
                    "画像で地図を描画"
                } else {
                    "文字で地図を描画"
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
        Border::new(width, display.colors).text(rows, "地図の描画方式", height)
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

mod about;
mod help;
mod panels;
mod raster;
mod render;
mod stations;
#[cfg(test)]
mod tests;

pub use about::{AboutInfo, about};
pub use help::{HelpOverlay, HelpTopic, help};
pub use panels::detail;
pub use render::Renderer;
use stations::{ReadingLine, intensity_heading, intensity_readings};
