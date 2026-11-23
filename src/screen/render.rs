use super::{
    panels::{
        earthquake_sidebar, earthquake_summary, eew_area_entries, eew_area_panel, eew_banner,
        eew_overview,
    },
    raster::text_row,
    *,
};
use crate::{
    data::Result,
    graphics::{self, Image},
    map::{Atlas, Raster, TEXT_CELL_HEIGHT, TEXT_CELL_WIDTH},
};

pub struct Renderer {
    atlas: Atlas,
    pub(super) raster: Raster,
    pub(super) image_raster: Raster,
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

    fn render_map(
        &mut self,
        bulletin: &Bulletin,
        series: &[Bulletin],
        layer: Layer,
        view: View,
        map: Map,
        (map_cols, rows): (usize, usize),
    ) -> bool {
        self.frame.map_view = None;
        if let Some(image) = map.image() {
            let (dx, dy) = image.cell();
            self.image_raster.resize(map_cols * dx, rows * dy);
            let clamped = self.atlas.draw(
                &mut self.image_raster,
                bulletin,
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
        let text_map = !matches!(map, Map::Image(Image::Sixel(_)));
        if text_map {
            self.raster
                .resize(map_cols * TEXT_CELL_WIDTH, rows * TEXT_CELL_HEIGHT);
            let clamped = self
                .atlas
                .draw(&mut self.raster, bulletin, series, layer, view, None);
            self.frame.map_view.get_or_insert(clamped);
        }
        text_map
    }

    pub fn render(&mut self, scene: Scene<'_>, display: Display) -> &Frame {
        let width = display.width();
        let Scene {
            bulletin: b,
            banner,
            eew_source,
            warning_seconds,
            demo,
            series,
            view,
            layer,
            station_grouping,
            controls,
            history,
            menu,
            quake_notices,
            ..
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
            header.extend(eew_banner(b, width, display.colors, demo));
            if b.warning
                && !b.eew_cancelled
                && let Some(seconds) = warning_seconds
            {
                header.extend(
                    wrap(&format!("発表から {seconds}秒"), width)
                        .into_iter()
                        .map(|line| color(&line, INK, display.colors)),
                );
            }
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
                aside.push(ReadingLine::Text(
                    (*title).into(),
                    if b.warning { ALERT } else { ACCENT },
                ));
                if warning_missing > 0 {
                    aside.push(ReadingLine::Text(
                        format!("地図にない地域 {warning_missing}件"),
                        MUTED,
                    ));
                }
                aside.extend(areas.iter().cloned().map(ReadingLine::from));
            } else if b.has_observations() {
                aside.push(ReadingLine::Text(observation_label(layer).into(), MUTED));
                let reading_width = if sidebar_width == 0 {
                    width
                } else if framed {
                    sidebar_width - TEXT_FRAME_MARGIN
                } else {
                    sidebar_width
                };
                aside.extend(readings(b, layer, station_grouping, reading_width));
            }
        }
        let mut legend = Vec::new();
        if b.epicenter.is_some() {
            legend.push(('✚', if b.eew { "推定震央" } else { "震央" }, map::EPICENTER));
        }
        if !b.warning_areas.is_empty() {
            legend.push(('■', "警報対象", map::WARNING));
        }
        if b.has_observations()
            && (layer == Layer::Regions || (display.colors && display.map.image().is_some()))
        {
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
                        .map(|line| line.render(width, false))
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
        let text_map = self.render_map(b, series, layer, view, display.map, (map_cols, rows));
        let map_border = Border::new(map_cols + 2, display.colors);
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
                &scene,
                history,
                sidebar_width,
                rows + map_edges,
                display.colors,
            )
        } else {
            let title = aside
                .first()
                .map(|line| line.render(sidebar_width, false))
                .unwrap_or_default();
            let capacity = rows;
            if framed && !aside.is_empty() {
                aside.remove(0);
            }
            if aside.len() > capacity {
                let omitted = aside.len() - capacity + 1;
                aside.truncate(capacity - 1);
                aside.push(ReadingLine::Text(format!("ほか {omitted} 件"), MUTED));
            }
            if framed && aside.is_empty() && title.is_empty() {
                Vec::new()
            } else {
                let inner = sidebar_width - if framed { TEXT_FRAME_MARGIN } else { 0 };
                let lines = aside
                    .into_iter()
                    .map(|line| line.render(inner, display.colors))
                    .collect();
                if framed {
                    Border::notice(sidebar_width, display.colors, b.warning).text(
                        lines,
                        &title,
                        rows + 2,
                    )
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
