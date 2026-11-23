use crate::data::{TsunamiArea, TsunamiLevel};
use crate::screen::panels::eew_badge;

use super::*;

fn area_rows(area: &TsunamiArea, width: usize, ansi: bool) -> Vec<String> {
    const BADGE_WIDTH: usize = 12;
    let Some(background) = map::tsunami_color(area.level)
        .filter(|_| ansi && width >= BADGE_WIDTH)
        .map(|index| PALETTE[index as usize])
    else {
        return wrap(&area.line(), width);
    };

    let badge = eew_badge(
        &format!(" {} ", pad(area.level.label(), BADGE_WIDTH - 2)),
        Some(background),
        ansi,
    );
    let mut rows = vec![badge];
    if BADGE_WIDTH + 2 + area.name.width() <= width {
        write!(
            rows[0],
            "  {}",
            color(&fit(&area.name, width), background, ansi)
        )
        .unwrap();
    } else {
        rows.extend(
            wrap(&area.name, width)
                .iter()
                .map(|name| color(name, background, ansi)),
        );
    }

    let details = area.details.trim();
    if !details.is_empty() {
        let last = rows.last_mut().unwrap();
        let remaining = width.saturating_sub(styled_width(last) + 2);
        if details.width() <= remaining {
            write!(last, "  {}", color(&fit(details, remaining), INK, ansi)).unwrap();
        } else {
            rows.extend(
                wrap(details, width - 2)
                    .iter()
                    .map(|line| format!("  {}", color(line, INK, ansi))),
            );
        }
    }
    rows
}

impl Renderer {
    pub fn tsunami(
        &mut self,
        menu: &[String],
        notice: NoticePanel<'_>,
        coasts: &[TsunamiArea],
        display: Display,
        view: View,
        scroll: &mut usize,
    ) -> &Frame {
        let width = display.width();
        let mut header = menu.to_vec();
        let status_ink = if notice.alert { WARNING_INK } else { INK };
        header.extend(
            wrap(notice.status, width)
                .iter()
                .map(|line| color(line, status_ink, display.colors)),
        );
        let mut legend = Vec::new();
        let mut legend_line = String::new();
        let mut legend_width = 0;
        for level in [
            TsunamiLevel::MajorWarning,
            TsunamiLevel::Warning,
            TsunamiLevel::Advisory,
            TsunamiLevel::Forecast,
        ] {
            if !coasts
                .iter()
                .any(|area| area.level == level && map::has_coast(&area.code))
            {
                continue;
            }
            let ink = map::tsunami_color(level).unwrap();
            let marker = if display.colors {
                '■'
            } else {
                map::plain(ink)
            };
            let label = format!("{marker} {}", level.label());
            let label_width = label.width();
            if legend_width > 0 && legend_width + 2 + label_width > width {
                legend.push(std::mem::take(&mut legend_line));
                legend_width = 0;
            }
            if legend_width > 0 {
                legend_line.push_str("  ");
                legend_width += 2;
            }
            legend_line.push_str(&color(
                &fit(&label, width),
                PALETTE[ink as usize],
                display.colors,
            ));
            legend_width += label_width;
        }
        if !legend_line.is_empty() {
            legend.push(legend_line);
        }
        let available = display.rows.saturating_sub(header.len() + legend.len() + 1);
        let wide = width >= 72;
        let show_map = available >= if wide { 3 } else { 6 };
        let split = show_map && wide;

        let map_width = if split { width * 56 / 100 } else { width };
        let panel_width = if split {
            width - map_width - PANEL_GAP
        } else {
            width
        };
        let inner = panel_width - TEXT_FRAME_MARGIN;
        let mut readings: Vec<_> = if coasts.is_empty() {
            notice
                .lines
                .iter()
                .flat_map(|line| wrap(line, inner))
                .collect()
        } else {
            coasts
                .iter()
                .flat_map(|area| area_rows(area, inner, display.colors))
                .collect()
        };
        let missing = coasts
            .iter()
            .filter(|area| map::tsunami_color(area.level).is_some() && !map::has_coast(&area.code))
            .count();
        if show_map && missing > 0 {
            readings.extend(
                wrap(&format!("地図にない予報区 {missing}件"), inner)
                    .iter()
                    .map(|line| color(line, MUTED, display.colors)),
            );
        }
        readings.extend(
            notice
                .notes
                .iter()
                .flat_map(|line| wrap(line, inner))
                .map(|line| color(&line, MUTED, display.colors)),
        );
        let panel_height = if !show_map {
            display.rows.saturating_sub(header.len() + 1).max(2)
        } else if split {
            available.min(62)
        } else {
            (readings.len() + 2).min(available / 2)
        };
        let capacity = panel_height - 2;
        *scroll = (*scroll).min(readings.len().saturating_sub(capacity));
        let mut actions = Vec::new();
        if readings.len() > capacity && capacity > 0 {
            actions.push("↑↓ 一覧");
        }
        if show_map {
            actions.extend(["WASD 移動", "+/- 拡縮"]);
        }
        let keys = footer(&actions, display);
        let title = if readings.len() > capacity && capacity > 0 {
            format!(
                "{}  {}–{}/{}",
                notice.title,
                *scroll + 1,
                *scroll + capacity,
                readings.len()
            )
        } else {
            notice.title.to_owned()
        };
        let sidebar = Border::notice(panel_width, display.colors, notice.alert).text(
            readings.into_iter().skip(*scroll).take(capacity).collect(),
            &title,
            panel_height,
        );
        if !show_map {
            header.extend(sidebar);
            header.push(keys);
            self.frame = Frame {
                text: header.join("\n"),
                ..Frame::default()
            };
            return &self.frame;
        }
        let map_height = if split {
            panel_height
        } else {
            (available - panel_height).min(62)
        };
        let rows = map_height - 2;
        let map_cols = map_width - 2;
        let text_map = self.render_map(display.map, (map_cols, rows), |atlas, raster, cell| {
            atlas.draw_tsunami(raster, coasts, view, cell.is_some())
        });
        self.frame.map_row = header.len() + 2;
        self.frame.map_col = 2;
        let border = Border::new(map_width, display.colors);
        let edge = border.edge();
        let mode = if view.national || legend.is_empty() {
            "全国"
        } else {
            "対象沿岸"
        };
        let title = format!(
            "地図 · {mode}  {:.1}x",
            self.frame.map_view.as_ref().map_or(1.0, View::zoom_factor)
        );
        let text = &mut self.frame.text;
        text.clear();
        for line in header {
            writeln!(text, "{line}").unwrap();
        }
        let mut next_side = sidebar.iter();
        for row in 0..map_height {
            if row == 0 {
                text.push_str(&border.top(&title));
            } else if row == map_height - 1 {
                text.push_str(&border.bottom());
            } else {
                text.push_str(&edge);
                if text_map {
                    text_row(
                        text,
                        &self.raster,
                        row - 1,
                        display.colors,
                        display.map == Map::Braille,
                    );
                } else {
                    text.extend(std::iter::repeat_n(' ', map_cols));
                }
                text.push_str(&edge);
            }
            if split && let Some(line) = next_side.next() {
                write!(text, " {line}").unwrap();
            }
            text.push('\n');
        }
        for line in legend
            .into_iter()
            .chain(sidebar.into_iter().filter(|_| !split))
        {
            writeln!(text, "{line}").unwrap();
        }
        text.push_str(&keys);
        &self.frame
    }
}
