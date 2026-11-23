use super::*;

pub(super) fn earthquake_summary(
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

pub(super) fn earthquake_sidebar(
    scene: &Scene<'_>,
    history: History<'_>,
    width: usize,
    rows: usize,
    ansi: bool,
) -> Vec<String> {
    let Scene {
        bulletin: b,
        quake_notices: notices,
        layer,
        station_grouping: grouping,
        reading_start: start,
        ..
    } = *scene;
    const MIN_HISTORY: usize = 3;
    let min_readings =
        3 + usize::from(layer == Layer::Stations && grouping == StationGrouping::Intensity);
    if rows < MIN_HISTORY + min_readings + 2 {
        return history.bottom_section(width, rows, ansi);
    }

    let summary_rows = (rows - MIN_HISTORY - min_readings - 1).min(8);
    let mut lines = vec![section_heading("地震の概要", width, ansi)];
    lines.extend(earthquake_summary(b, notices, width, summary_rows, ansi));
    lines.resize_with(1 + summary_rows, String::new);

    let remaining = rows - lines.len();
    let history_height = history.target_height().min(remaining - min_readings);
    let slots = remaining - history_height - 1;
    let (title, entries) = match layer {
        Layer::Stations => station_readings_window(b, grouping, start, slots, width),
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
                b.regions[start..end]
                    .iter()
                    .map(reading_row)
                    .map(ReadingLine::from)
                    .collect(),
            )
        }
    };
    lines.push(section_heading(&title, width, ansi));
    if entries.is_empty() {
        lines.push(color(&fit("震度データなし", width), MUTED, ansi));
    } else {
        lines.extend(entries.iter().map(|line| line.render(width, ansi)));
    }
    lines.resize_with(rows - history_height, String::new);
    lines.extend(history.section(width, history_height, ansi));
    lines
}

pub(super) fn eew_badge(value: &str, background: Option<Rgb>, ansi: bool) -> String {
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

pub(super) fn intensity_badge_rows(level: &Level, ansi: bool) -> Option<[String; 5]> {
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

pub(super) fn eew_banner(b: &Bulletin, width: usize, ansi: bool) -> Vec<String> {
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

pub(super) fn eew_overview(
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
    Border::notice(width, ansi, b.warning).text(content, &title, used)
}

pub(super) fn eew_area_entries(b: &Bulletin) -> Option<(&'static str, Vec<(String, Rgb)>)> {
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

pub(super) fn eew_area_panel(
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
    Border::notice(width, ansi, warning).text(content, &title, height)
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
        rows.push(ReadingLine::Text(line, INK));
    }
    if !rows.is_empty() {
        rows.push(ReadingLine::Text(String::new(), INK));
    }
    if let Some((title, areas)) = eew_area_entries(b) {
        rows.push(ReadingLine::Text(
            format!("{title}  {} 地域", areas.len()),
            if b.warning { ALERT } else { ACCENT },
        ));
        rows.extend(areas.into_iter().map(ReadingLine::from));
    } else if b.has_observations() {
        let count = match layer {
            Layer::Regions => b.regions.len(),
            Layer::Stations => b.stations.len(),
        };
        rows.push(ReadingLine::Text(
            format!("{}  {count} 件", observation_label(layer)),
            MUTED,
        ));
        if layer == Layer::Stations && grouping == StationGrouping::Intensity {
            intensity_rows_start = Some(rows.len());
        }
        rows.extend(readings(b, layer, grouping, inner));
    }
    let mut capacity = display
        .rows
        .saturating_sub(header.len() + content.len() + 4);
    let separator = capacity > 1 && !rows.is_empty();
    capacity -= usize::from(separator);
    *scroll = (*scroll).min(rows.len().saturating_sub(capacity));
    if separator {
        let continuation = intensity_rows_start
            .filter(|start| {
                *scroll > *start && !matches!(rows[*scroll], ReadingLine::Intensity { .. })
            })
            .and_then(|start| {
                rows[start..*scroll]
                    .iter()
                    .rfind(|line| matches!(line, ReadingLine::Intensity { .. }))
            });
        content
            .push(continuation.map_or_else(String::new, |line| line.render(inner, display.colors)));
    }
    content.extend(
        rows.iter()
            .skip(*scroll)
            .take(capacity)
            .map(|line| line.render(inner, display.colors)),
    );
    let height = content.len() + 2;
    let border = Border::notice(width, display.colors, b.warning);
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
    header.push(footer(&["↑↓ スクロール", "I 地図"], display));
    Frame {
        text: header.join("\n"),
        ..Frame::default()
    }
}
