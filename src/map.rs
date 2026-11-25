use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write,
};

use crate::data::{Bulletin, Location, Result};

mod travel_times;
mod tsunami;
mod waves;
pub use tsunami::{has_coast, tsunami_color};
pub use waves::Wavefront;

pub type Color = u8;
pub type Rgb = (u8, u8, u8);
pub const SEA: Color = 0;
pub const LAND: Color = 1;
pub const SURROUNDINGS: Color = 2;
pub const WARNING: Color = 3;
pub const EPICENTER: Color = 4;
pub const REGION: Color = 5;
pub const STATION: Color = 14;
pub const TSUNAMI_FORECAST: Color = 23;
const TSUNAMI_ADVISORY: Color = 24;
const TSUNAMI_WARNING: Color = 25;
const TSUNAMI_MAJOR_WARNING: Color = 26;
pub const TEXT_CELL_WIDTH: usize = 4;
pub const TEXT_CELL_HEIGHT: usize = 8;
pub(crate) const SHORT_INTENSITIES: [&str; 9] = ["1", "2", "3", "4", "5-", "5+", "6-", "6+", "7"];
const S_WAVE_OUTLINE: Color = 27;
pub const P_WAVE: Color = 28;
pub const S_WAVE: Color = 29;
pub const PALETTE: [Rgb; 30] = [
    (15, 27, 42),
    (54, 91, 98),
    (36, 55, 66),
    (231, 117, 48),
    (255, 95, 130),
    (40, 96, 131),
    (28, 128, 104),
    (152, 139, 53),
    (193, 108, 37),
    (207, 87, 60),
    (193, 48, 62),
    (204, 66, 116),
    (171, 62, 146),
    (106, 43, 179),
    (119, 213, 255),
    (81, 227, 178),
    (244, 232, 113),
    (255, 166, 72),
    (255, 129, 99),
    (255, 82, 90),
    (255, 115, 163),
    (231, 104, 201),
    (158, 95, 235),
    (119, 213, 255),
    (255, 222, 94),
    (255, 74, 83),
    (195, 95, 255),
    (27, 13, 42),
    (85, 213, 252),
    (246, 104, 207),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Regions,
    #[default]
    Stations,
}

impl Layer {
    pub fn other(self) -> Self {
        match self {
            Self::Regions => Self::Stations,
            Self::Stations => Self::Regions,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Regions => "地域別",
            Self::Stations => "観測点",
        }
    }
}

const ZOOM_STEP: f64 = 1.4;
const EEW_AUTO_ZOOM_STEPS: u16 = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EewLayer {
    #[default]
    Warning,
    Forecast,
}

impl EewLayer {
    pub fn other(self) -> Self {
        match self {
            Self::Warning => Self::Forecast,
            Self::Forecast => Self::Warning,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => "警報対象",
            Self::Forecast => "推定震度",
        }
    }
}

#[derive(Clone, Copy)]
struct MapViewport {
    center: (f64, f64),
    span: (f64, f64),
    zoom_steps: u16,
}

#[derive(Clone, Copy)]
pub struct View {
    pub national: bool,
    pub eew_layer: EewLayer,
    pub wavefront: Option<Wavefront>,
    manual_zoom: Option<u16>,
    center: Option<(f64, f64)>,
    rendered: Option<MapViewport>,
}

impl View {
    pub fn new(national: bool) -> Self {
        Self {
            national,
            eew_layer: EewLayer::default(),
            wavefront: None,
            manual_zoom: None,
            center: None,
            rendered: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.national);
    }

    pub fn zoom_factor(&self) -> f64 {
        self.rendered.map_or(1.0, |viewport| {
            ZOOM_STEP.powi(i32::from(viewport.zoom_steps))
        })
    }

    pub fn zoom_by(&mut self, direction: i16) -> bool {
        let Some(viewport) = self.rendered else {
            return false;
        };
        let steps = viewport.zoom_steps.saturating_add_signed(direction);
        // Avoid losing coordinate precision as the projected span approaches zero.
        let precision_limit = direction > 0 && viewport.span.0.min(viewport.span.1) < 1e-9;
        if steps == viewport.zoom_steps || precision_limit {
            return false;
        }
        self.manual_zoom = Some(steps);
        self.center = None;
        self.national = false;
        true
    }

    pub fn pan(&mut self, east: f64, north: f64) -> bool {
        let Some(viewport) = self.rendered else {
            return false;
        };
        let (x, y) = viewport.center;
        let (width, height) = viewport.span;
        self.center = Some((x + east * width, y + north * height));
        true
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}

impl Bounds {
    fn point(x: f64, y: f64) -> Self {
        Self {
            west: x,
            south: y,
            east: x,
            north: y,
        }
    }

    fn padded(self, margin: f64) -> Self {
        Self {
            west: self.west - margin,
            east: self.east + margin,
            south: self.south - margin,
            north: self.north + margin,
        }
    }

    fn clamp_center(self, x: f64, y: f64, half_width: f64, half_height: f64) -> (f64, f64) {
        let clamp = |center: f64, start: f64, end: f64, half: f64| {
            let midpoint = (start + end) / 2.0;
            center.clamp((start + half).min(midpoint), (end - half).max(midpoint))
        };
        (
            clamp(x, self.west, self.east, half_width),
            clamp(y, self.south, self.north, half_height),
        )
    }

    fn merge(self, other: Self) -> Self {
        Self {
            west: self.west.min(other.west),
            south: self.south.min(other.south),
            east: self.east.max(other.east),
            north: self.north.max(other.north),
        }
    }
}

fn project(place: Location) -> (f64, f64) {
    (
        place.lon,
        place
            .lat
            .clamp(-85.0, 85.0)
            .to_radians()
            .tan()
            .asinh()
            .to_degrees(),
    )
}

struct Edge {
    low: f64,
    high: f64,
    x: f64,
    slope: f64,
}

struct Shape {
    bounds: Bounds,
    first: (f64, f64),
    edges: Vec<Edge>,
    area: f64,
}

impl Shape {
    fn label_position(&self) -> Option<(f64, f64)> {
        let height = self.bounds.north - self.bounds.south;
        let mut cuts = Vec::new();
        let mut best = 0.0;
        let mut anchor = None;
        // Sample horizontal slices and favor a wide interior span near the
        // middle. The midpoint of the bounding box can lie offshore.
        for row in 1..10 {
            let y = self.bounds.south + f64::from(row) * height / 10.0;
            cuts.clear();
            cuts.extend(
                self.edges
                    .iter()
                    .filter(|edge| edge.low <= y && y < edge.high)
                    .map(|edge| edge.x + (y - edge.low) * edge.slope),
            );
            cuts.sort_unstable_by(f64::total_cmp);
            for span in cuts.as_chunks::<2>().0 {
                let score = (span[1] - span[0]) / (1.0 + f64::from((row - 5i32).abs()) / 10.0);
                if score > best {
                    best = score;
                    anchor = Some(((span[0] + span[1]) / 2.0, y));
                }
            }
        }
        anchor
    }
}

type Polygons = Vec<Vec<Vec<[f64; 2]>>>;

fn shapes(polygons: Polygons) -> Vec<Shape> {
    polygons
        .into_iter()
        .filter_map(|polygon| {
            let mut rings = polygon.into_iter().map(|ring| {
                ring.into_iter()
                    .map(|[lon, lat]| project(Location { lon, lat }))
                    .collect::<Vec<_>>()
            });
            let exterior = rings.next()?;
            let &(x, y) = exterior.first()?;
            let bounds = exterior
                .iter()
                .fold(Bounds::point(x, y), |bounds, &(x, y)| {
                    bounds.merge(Bounds::point(x, y))
                });
            let area = exterior
                .windows(2)
                .map(|points| {
                    (points[0].0 - x) * (points[1].1 - y) - (points[1].0 - x) * (points[0].1 - y)
                })
                .sum::<f64>()
                .abs();
            let edges = std::iter::once(exterior)
                .chain(rings)
                .flat_map(|ring| {
                    ring.windows(2)
                        .filter_map(|edge| {
                            let (x1, y1) = edge[0];
                            let (x2, y2) = edge[1];
                            (y1 != y2).then(|| Edge {
                                low: y1.min(y2),
                                high: y1.max(y2),
                                x: if y1 < y2 { x1 } else { x2 },
                                slope: (x2 - x1) / (y2 - y1),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            Some(Shape {
                bounds,
                first: (x, y),
                edges,
                area,
            })
        })
        .collect()
}

pub struct Atlas {
    regions: Vec<Vec<Shape>>,
    label_positions: Vec<Option<(f64, f64)>>,
    names: HashMap<String, usize>,
    codes: HashMap<String, usize>,
    surroundings: Vec<Shape>,
    lakes: Vec<Shape>,
    national: Bounds,
    coverage: Bounds,
}

#[derive(Clone, Copy)]
struct Extent {
    west: f64,
    north: f64,
    step: f64,
}

impl Atlas {
    pub fn load() -> Result<Self> {
        let regions: BTreeMap<String, (String, Polygons)> =
            serde_json::from_str(include_str!("../data/regions.json"))?;
        let mut names = HashMap::new();
        let mut codes = HashMap::new();
        let regions: Vec<_> = regions
            .into_iter()
            .enumerate()
            .map(|(index, (code, (name, polygons)))| {
                names.insert(name, index);
                codes.insert(code, index);
                shapes(polygons)
            })
            .collect();
        let label_positions = regions
            .iter()
            .map(|shapes| {
                shapes
                    .iter()
                    .max_by(|a, b| a.area.total_cmp(&b.area))
                    .and_then(Shape::label_position)
            })
            .collect();
        let surroundings = shapes(serde_json::from_str(include_str!(
            "../data/surroundings.json"
        ))?);
        let lakes = shapes(serde_json::from_str(include_str!("../data/lakes.json"))?);
        let (x1, y1) = project(Location {
            lon: 119.7,
            lat: 21.5,
        });
        let (x2, y2) = project(Location {
            lon: 146.2,
            lat: 46.0,
        });
        let national = Bounds::point(x1, y1).merge(Bounds::point(x2, y2));
        let coverage = surroundings
            .iter()
            .chain(regions.iter().flatten())
            .map(|shape| shape.bounds)
            .fold(national, Bounds::merge);
        Ok(Self {
            regions,
            label_positions,
            names,
            codes,
            surroundings,
            lakes,
            national,
            coverage,
        })
    }

    fn area(&self, code: &str, name: &str) -> Option<usize> {
        self.codes
            .get(code)
            .or_else(|| self.names.get(name))
            .copied()
    }

    pub fn has_area(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    fn event_bounds(&self, bulletins: &[Bulletin], layer: EewLayer) -> Option<Bounds> {
        let positions = bulletins
            .iter()
            .flat_map(|v| {
                v.epicenter
                    .into_iter()
                    .chain(v.stations.iter().filter_map(|p| p.location))
            })
            .map(|point| {
                let (x, y) = project(point);
                Bounds::point(x, y)
            });
        let areas = bulletins
            .iter()
            .flat_map(|v| {
                v.warning_areas
                    .iter()
                    .filter_map(|name| self.area("", name))
                    .chain(
                        v.regions
                            .iter()
                            .filter_map(|area| self.area(&area.area_code, &area.name)),
                    )
                    .chain(
                        v.forecast_areas
                            .iter()
                            .filter(move |area| {
                                layer == EewLayer::Forecast && area.display_level().is_some()
                            })
                            .filter_map(|area| self.area("", &area.name)),
                    )
            })
            .flat_map(|index| self.regions[index].iter().map(|shape| shape.bounds));
        positions.chain(areas).reduce(Bounds::merge)
    }

    fn extent(
        &self,
        bulletin: &Bulletin,
        sequence: &[Bulletin],
        view: &mut View,
        width: usize,
        height: usize,
    ) -> Extent {
        let current = std::slice::from_ref(bulletin);
        let sequence = if sequence.is_empty() {
            current
        } else {
            sequence
        };
        let context = self
            .event_bounds(sequence, view.eew_layer)
            .unwrap_or(self.national);
        let focus = if view.national {
            self.national
        } else if view.manual_zoom.is_some() {
            self.event_bounds(current, view.eew_layer)
                .unwrap_or(context)
        } else {
            context
        };
        let max_steps = if bulletin.eew {
            EEW_AUTO_ZOOM_STEPS
        } else {
            u16::MAX
        };
        self.fit_view(context, focus, max_steps, view, (width, height))
    }

    fn fit_view(
        &self,
        context: Bounds,
        focus: Bounds,
        max_steps: u16,
        view: &mut View,
        (width, height): (usize, usize),
    ) -> Extent {
        let width = width as f64;
        let height = height as f64;
        let fit = |bounds: Bounds| {
            (1.10 * (bounds.east - bounds.west) / width)
                .max(1.10 * (bounds.north - bounds.south) / height)
        };
        // Use the same zoom ladder for automatic framing and manual zoom.
        // Keep offshore event data within reach if it extends beyond the coastline data.
        let coverage = self.coverage.merge(context.padded(0.5));
        let widest = fit(coverage);
        let zoom_steps = view.manual_zoom.unwrap_or_else(|| {
            let desired = fit(focus).max(3.5 / height);
            let steps = ((widest / desired).ln() / ZOOM_STEP.ln()).floor() as u16;
            steps.min(max_steps)
        });
        let step = widest / ZOOM_STEP.powi(i32::from(zoom_steps));
        let span = (step * width, step * height);
        let (x, y) = view.center.unwrap_or_else(|| {
            (
                (focus.west + focus.east) / 2.0,
                (focus.south + focus.north) / 2.0,
            )
        });
        let center = coverage.clamp_center(x, y, span.0 / 2.0, span.1 / 2.0);
        if view.center.is_some() {
            view.center = Some(center);
        }
        view.rendered = Some(MapViewport {
            center,
            span,
            zoom_steps,
        });
        Extent {
            west: center.0 - span.0 / 2.0,
            north: center.1 + span.1 / 2.0,
            step,
        }
    }

    pub fn draw(
        &self,
        raster: &mut Raster,
        bulletin: &Bulletin,
        sequence: &[Bulletin],
        layer: Layer,
        mut view: View,
        image_cell: Option<(usize, usize)>,
    ) -> View {
        let image = image_cell.is_some();
        let extent = self.extent(bulletin, sequence, &mut view, raster.width, raster.height);
        let mut colors = vec![LAND; self.regions.len()];
        let mut labels = Vec::new();
        if layer == Layer::Regions {
            for area in &bulletin.regions {
                if let Some((index, level)) = self
                    .area(&area.area_code, &area.name)
                    .zip(area.level.index())
                {
                    colors[index] = REGION + level;
                    if let Some(position) = self.label_positions[index] {
                        labels.push((position, level));
                    }
                }
            }
        }
        match view.eew_layer {
            EewLayer::Warning => {
                for area in &bulletin.warning_areas {
                    if let Some(index) = self.area("", area) {
                        colors[index] = WARNING;
                    }
                }
            }
            EewLayer::Forecast => {
                for area in &bulletin.forecast_areas {
                    if let Some((index, level)) =
                        self.area("", &area.name).zip(area.display_level())
                    {
                        colors[index] = REGION + level;
                    }
                }
            }
        }
        self.draw_base(raster, extent, colors.into_iter(), image);
        if let Some((wavefront, center)) = view.wavefront.zip(bulletin.epicenter) {
            raster.draw_wavefront(center, wavefront, extent, image);
        }
        if image && layer == Layer::Stations {
            for point in bulletin.stations.iter().rev() {
                if let Some((where_, level)) = point.location.zip(point.level.index()) {
                    raster.mark(where_, STATION + level, extent, image);
                }
            }
        }
        if !labels.is_empty() {
            let cell = image_cell.unwrap_or((TEXT_CELL_WIDTH, TEXT_CELL_HEIGHT));
            labels.sort_by_key(|&(_, level)| std::cmp::Reverse(level));
            raster.place_labels(&labels, bulletin.epicenter, extent, cell, image);
        }
        if let Some(where_) = bulletin.epicenter {
            raster.mark(where_, EPICENTER, extent, image);
        }
        view
    }

    fn draw_base(
        &self,
        raster: &mut Raster,
        extent: Extent,
        colors: impl Iterator<Item = Color>,
        image: bool,
    ) {
        raster.pixels.fill(SEA);
        raster.labels.clear();
        if !image {
            raster.markers.resize(
                (raster.width / TEXT_CELL_WIDTH) * (raster.height / TEXT_CELL_HEIGHT),
                None,
            );
            raster.markers.fill(None);
        }
        raster.islands.clear();
        for shape in &self.surroundings {
            raster.paint(shape, SURROUNDINGS, extent);
        }
        for (shapes, color) in self.regions.iter().zip(colors) {
            for shape in shapes {
                raster.paint(shape, color, extent);
            }
        }
        for &(x, y, color) in &raster.islands {
            let cell = &mut raster.pixels[y * raster.width + x];
            if *cell == SEA {
                *cell = color;
            }
        }
        // Shape area is twice the polygon's area. Skip lakes smaller than two pixels.
        let minimum_lake_area = 4.0 * extent.step.powi(2);
        for lake in &self.lakes {
            if lake.area >= minimum_lake_area {
                raster.paint(lake, SEA, extent);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct LabelCell {
    pub glyph: char,
    pub color: Color,
}

#[derive(Default)]
pub struct Raster {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Color>,
    pub markers: Vec<Option<Color>>,
    pub labels: Vec<Option<LabelCell>>,
    scanlines: Vec<Vec<f64>>,
    islands: Vec<(usize, usize, Color)>,
    bands: Vec<[u8; PALETTE.len()]>,
}

impl Raster {
    fn stroke_line(&mut self, from: [f64; 2], to: [f64; 2], radius: usize, color: Color) {
        let delta = [to[0] - from[0], to[1] - from[1]];
        let mut enter: f64 = 0.0;
        let mut exit: f64 = 1.0;
        // Clip the segment before walking pixels, including when manually zoomed in.
        for (axis, limit) in [self.width - 1, self.height - 1].into_iter().enumerate() {
            let d = delta[axis];
            let position = from[axis];
            if d == 0.0 {
                if !(0.0..=limit as f64).contains(&position) {
                    return;
                }
            } else {
                let first = -position / d;
                let last = (limit as f64 - position) / d;
                enter = enter.max(first.min(last));
                exit = exit.min(first.max(last));
            }
        }
        if enter > exit {
            return;
        }
        let pixels = ((exit - enter) * delta[0].abs().max(delta[1].abs())).ceil() as usize;
        for index in 0..=pixels {
            let at = enter + (exit - enter) * index as f64 / pixels.max(1) as f64;
            let x = ((from[0] + at * delta[0]).round() as usize).min(self.width - 1);
            let y = ((from[1] + at * delta[1]).round() as usize).min(self.height - 1);
            for y in y.saturating_sub(radius)..=(y + radius).min(self.height - 1) {
                let start = y * self.width + x.saturating_sub(radius);
                let end = y * self.width + (x + radius).min(self.width - 1);
                for pixel in &mut self.pixels[start..=end] {
                    *pixel = (*pixel).max(color);
                }
            }
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.pixels.resize(width * height, SEA);
        self.scanlines.resize_with(height, Vec::new);
    }

    fn place_labels(
        &mut self,
        labels: &[((f64, f64), u8)],
        epicenter: Option<Location>,
        extent: Extent,
        cell: (usize, usize),
        image: bool,
    ) {
        let (dx, dy) = cell;
        if image && (dx < 5 || dy < 7) {
            return;
        }
        let (columns, rows) = (self.width / dx, self.height / dy);
        let position = |(x, y): (f64, f64)| {
            let x = ((x - extent.west) / extent.step / dx as f64).floor() as isize;
            let y = ((extent.north - y) / extent.step / dy as f64).floor() as isize;
            ((0..columns as isize).contains(&x) && (0..rows as isize).contains(&y))
                .then_some((x as usize, y as usize))
        };
        let epicenter = epicenter.and_then(|point| position(project(point)));
        self.labels.resize(columns * rows, None);
        for &(point, level) in labels {
            let Some((x, row)) = position(point) else {
                continue;
            };
            let label = SHORT_INTENSITIES[level as usize].as_bytes();
            if label.len() > columns {
                continue;
            }
            let start = x.min(columns - label.len());
            let end = start + label.len();
            let nearby = start.saturating_sub(1)..(end + 1).min(columns);
            let offset = row * columns;
            let covers_epicenter = epicenter.is_some_and(|(x, y)| {
                if image {
                    row.abs_diff(y) <= 1 && nearby.contains(&x)
                } else {
                    row == y && (start..end).contains(&x)
                }
            });
            let crowds_label = (row.saturating_sub(1)..=(row + 1).min(rows - 1)).any(|other| {
                let offset = other * columns;
                self.labels[offset + nearby.start..offset + nearby.end]
                    .iter()
                    .any(Option::is_some)
            });
            if covers_epicenter || crowds_label {
                continue;
            }
            let color = STATION + level;
            for (i, &byte) in label.iter().enumerate() {
                self.labels[offset + start + i] = Some(LabelCell {
                    glyph: char::from(byte),
                    color,
                });
            }
            if image {
                self.draw_label(start * dx, row * dy, (dx * label.len(), dy), label, color);
            }
        }
    }

    fn draw_label(&mut self, x: usize, y: usize, size: (usize, usize), label: &[u8], color: Color) {
        const DIGITS: [[u8; 5]; 7] = [
            [2, 6, 2, 2, 7],
            [7, 1, 7, 4, 7],
            [7, 1, 7, 1, 7],
            [5, 5, 7, 1, 1],
            [7, 4, 7, 1, 7],
            [7, 4, 7, 5, 7],
            [7, 1, 2, 2, 2],
        ];
        let text_width = label.len() * 4 - 1;
        let scale = ((size.0 - 2) / text_width).min((size.1 - 2) / 5);
        let x = x + (size.0 - text_width * scale) / 2;
        let y = y + (size.1 - 5 * scale) / 2;
        for row in y - 1..y + 5 * scale + 1 {
            let offset = row * self.width + x;
            self.pixels[offset - 1..offset + text_width * scale + 1].fill(color);
        }
        for (index, &byte) in label.iter().enumerate() {
            let glyph = match byte {
                b'+' => [0, 2, 7, 2, 0],
                b'-' => [0, 0, 7, 0, 0],
                _ => DIGITS[(byte - b'1') as usize],
            };
            for (row, mask) in glyph.into_iter().enumerate() {
                for col in 0..3 {
                    if mask & (4 >> col) != 0 {
                        for dy in 0..scale {
                            let offset =
                                (y + row * scale + dy) * self.width + x + (4 * index + col) * scale;
                            self.pixels[offset..offset + scale].fill(SEA);
                        }
                    }
                }
            }
        }
    }

    fn paint(&mut self, shape: &Shape, color: Color, extent: Extent) {
        let Extent { west, north, step } = extent;
        let Bounds {
            west: left,
            south: bottom,
            east: right,
            north: top,
        } = shape.bounds;
        if right < west
            || left > west + step * self.width as f64
            || top < north - step * self.height as f64
            || bottom > north
        {
            return;
        }
        let row =
            |latitude: f64| (((north - latitude) / step - 0.5).ceil() as usize).min(self.height);
        let start = row(top);
        let end = row(bottom);
        for scan in &mut self.scanlines[start..end] {
            scan.clear();
        }
        for edge in &shape.edges {
            let first = row(edge.high).max(start);
            let last = row(edge.low).min(end);
            for (i, scan) in self.scanlines[first..last].iter_mut().enumerate() {
                let latitude = north - ((first + i) as f64 + 0.5) * step;
                scan.push((edge.x + (latitude - edge.low) * edge.slope - west) / step);
            }
        }
        let mut drawn = false;
        for (i, cuts) in self.scanlines[start..end].iter_mut().enumerate() {
            cuts.sort_unstable_by(f64::total_cmp);
            let offset = (start + i) * self.width;
            let col = |x: f64| ((x - 0.5).ceil() as usize).min(self.width);
            for pair in cuts.as_chunks::<2>().0 {
                let first = col(pair[0]);
                let last = col(pair[1]);
                if first < last {
                    self.pixels[offset + first..offset + last].fill(color);
                    drawn = true;
                }
            }
        }
        if !drawn && (right - left).max(top - bottom) >= step * 0.55 {
            let x = ((shape.first.0 - west) / step).floor() as isize;
            let y = ((north - shape.first.1) / step).floor() as isize;
            if (0..self.width as isize).contains(&x) && (0..self.height as isize).contains(&y) {
                self.islands.push((x as usize, y as usize, color));
            }
        }
    }

    fn mark(&mut self, where_: Location, color: Color, extent: Extent, image: bool) {
        let (lon, lat) = project(where_);
        let x = ((lon - extent.west) / extent.step).floor() as isize;
        let y = ((extent.north - lat) / extent.step).floor() as isize;
        let width = self.width as isize;
        let height = self.height as isize;
        if !(0..width).contains(&x) || !(0..height).contains(&y) {
            return;
        }
        if !image {
            let col = x as usize / TEXT_CELL_WIDTH;
            let row = y as usize / TEXT_CELL_HEIGHT;
            let marker = &mut self.markers[row * (self.width / TEXT_CELL_WIDTH) + col];
            if marker.is_none_or(|previous| color == EPICENTER || color > previous) {
                *marker = Some(color);
            }
            return;
        }
        let radius = if color == EPICENTER { 5 } else { 4 };
        for row in (y - radius).max(0)..=(y + radius).min(height - 1) {
            for col in (x - radius).max(0)..=(x + radius).min(width - 1) {
                let dx = (col - x).abs();
                let dy = (row - y).abs();
                let distance = dx * dx + dy * dy;
                let fill = if color == EPICENTER && dx.min(dy) <= 2 {
                    if dx.min(dy) == 2 || dx.max(dy) == radius {
                        SEA
                    } else {
                        color
                    }
                } else if color != EPICENTER && distance <= radius * radius {
                    if distance <= (radius - 1) * (radius - 1) {
                        color
                    } else {
                        SEA
                    }
                } else {
                    continue;
                };
                self.pixels[row as usize * self.width + col as usize] = fill;
            }
        }
    }

    pub fn sixel(&mut self, out: &mut String) {
        out.clear();
        write!(out, "\x1bP7;1q\"1;1;{};{}", self.width, self.height).unwrap();
        let percent = |channel| (u16::from(channel) * 100 + 127) / 255;
        for (index, (r, g, b)) in PALETTE.into_iter().enumerate() {
            write!(
                out,
                "#{index};2;{};{};{}",
                percent(r),
                percent(g),
                percent(b)
            )
            .unwrap();
        }
        self.bands.resize(self.width, [0; PALETTE.len()]);
        for (band, pixels) in self.pixels.chunks(self.width * 6).enumerate() {
            if band > 0 {
                out.push('-');
            }
            self.bands.fill([0; PALETTE.len()]);
            let mut last = [0; PALETTE.len()];
            for (row, line) in pixels.chunks(self.width).enumerate() {
                for (x, (&color, masks)) in line.iter().zip(&mut self.bands).enumerate() {
                    masks[color as usize] |= 1 << row;
                    last[color as usize] = last[color as usize].max(x + 1);
                }
            }
            let mut first = true;
            for (color, end) in last.into_iter().enumerate().filter(|(_, end)| *end != 0) {
                if !first {
                    out.push('$');
                }
                first = false;
                write!(out, "#{color}").unwrap();
                let mut start = 0;
                while start < end {
                    let value = self.bands[start][color];
                    let mut next = start + 1;
                    while next < end && self.bands[next][color] == value {
                        next += 1;
                    }
                    let count = next - start;
                    let glyph = char::from(b'?' + value);
                    if count > 3 {
                        write!(out, "!{count}{glyph}").unwrap();
                    } else {
                        out.extend(std::iter::repeat_n(glyph, count));
                    }
                    start = next;
                }
            }
        }
        out.push_str("\x1b\\");
    }
}

pub fn rgb(out: &mut String, (r, g, b): Rgb, background: bool) {
    let channel = if background { 48 } else { 38 };
    write!(out, "\x1b[{channel};2;{r};{g};{b}m").unwrap();
}

pub fn plain(color: Color) -> char {
    match color {
        SEA | S_WAVE_OUTLINE => ' ',
        LAND => '░',
        SURROUNDINGS => '·',
        WARNING => '█',
        EPICENTER => '+',
        TSUNAMI_MAJOR_WARNING => 'M',
        TSUNAMI_WARNING => 'W',
        TSUNAMI_ADVISORY => 'A',
        TSUNAMI_FORECAST => 'F',
        P_WAVE => 'P',
        S_WAVE => 'S',
        value if value >= STATION => char::from(b"1234abcde"[(value - STATION) as usize]),
        value => char::from(b"1234abcde"[(value - REGION) as usize]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epicenter_keeps_clear_of_labels_on_adjacent_rows() {
        let mut raster = Raster::default();
        raster.resize(80, 50);
        let extent = Extent {
            west: -25.0,
            north: 25.0,
            step: 1.0,
        };
        let epicenter = Location { lon: 0.0, lat: 0.0 };
        let labels = [((0.0, 10.0), 8), ((30.0, 10.0), 4)];
        raster.place_labels(&labels, Some(epicenter), extent, (10, 10), true);
        assert!(
            !raster
                .labels
                .iter()
                .flatten()
                .any(|cell| cell.color == STATION + 8)
        );
        assert!(
            raster
                .labels
                .iter()
                .flatten()
                .any(|cell| cell.color == STATION + 4)
        );
    }

    #[test]
    fn crowded_labels_keep_the_strongest_intensity() {
        let mut raster = Raster::default();
        raster.resize(80, 50);
        let extent = Extent {
            west: 0.0,
            north: 50.0,
            step: 1.0,
        };
        let labels = [((25.0, 35.0), 8), ((25.0, 25.0), 7), ((65.0, 25.0), 4)];
        raster.place_labels(&labels, None, extent, (10, 10), true);
        assert!(
            raster
                .labels
                .iter()
                .flatten()
                .any(|cell| cell.color == STATION + 8)
        );
        assert!(
            !raster
                .labels
                .iter()
                .flatten()
                .any(|cell| cell.color == STATION + 7)
        );
        assert!(
            raster
                .labels
                .iter()
                .flatten()
                .any(|cell| cell.color == STATION + 4)
        );
    }

    #[test]
    fn shared_region_boundaries_do_not_leave_water_pixels_inland() {
        let atlas = Atlas::load().unwrap();
        let (x, y) = project(Location {
            lon: 139.978675,
            lat: 37.1456,
        });
        let (width, height) = (160, 120);
        let step = 0.001;
        let extent = Extent {
            west: x - (width as f64 / 2.0 + 0.5) * step,
            north: y + (height as f64 / 2.0 + 0.5) * step,
            step,
        };
        let mut raster = Raster::default();
        raster.resize(width, height);
        for shape in atlas.regions.iter().flatten() {
            raster.paint(shape, LAND, extent);
        }
        assert_eq!(
            raster.pixels.iter().filter(|&&pixel| pixel == SEA).count(),
            0
        );
    }

    #[test]
    fn lakes_remain_visible_inside_highlighted_regions() {
        let atlas = Atlas::load().unwrap();
        let mut raster = Raster::default();
        let (width, height) = (320, 240);
        raster.resize(width, height);
        for (name, lake, area) in [
            (
                "琵琶湖",
                Location {
                    lat: 35.29,
                    lon: 136.09,
                },
                "滋賀県北部",
            ),
            (
                "霞ヶ浦",
                Location {
                    lat: 36.03,
                    lon: 140.41,
                },
                "茨城県南部",
            ),
        ] {
            let bulletin = Bulletin {
                warning_areas: vec![area.into()],
                ..Bulletin::default()
            };
            let view = View {
                manual_zoom: Some(13),
                center: Some(project(lake)),
                ..View::new(false)
            };
            atlas.draw(
                &mut raster,
                &bulletin,
                &[],
                Layer::Regions,
                view,
                Some((10, 20)),
            );
            assert_eq!(
                raster.pixels[(height / 2) * width + width / 2],
                SEA,
                "{name}"
            );
            assert!(raster.pixels.contains(&WARNING), "{area}");
        }
    }

    #[test]
    fn eew_uses_the_same_zoom_steps_for_automatic_and_manual_framing() {
        let atlas = Atlas::load().unwrap();
        let mut bulletin = crate::data::demo().remove(1);
        bulletin.warning_areas = vec!["宮城県中部".into()];
        let mut view = View::new(false);
        let draw = |view: &mut View| atlas.extent(&bulletin, &[], view, 240, 320);
        let initial = draw(&mut view);
        let center = view.rendered.unwrap().center;
        assert_eq!(view.rendered.unwrap().zoom_steps, EEW_AUTO_ZOOM_STEPS);
        let target = atlas
            .event_bounds(std::slice::from_ref(&bulletin), view.eew_layer)
            .unwrap();
        assert_eq!(
            center,
            (
                (target.west + target.east) / 2.0,
                (target.south + target.north) / 2.0
            )
        );
        assert_ne!(center, project(bulletin.epicenter.unwrap()));

        assert!(view.pan(-0.18, 0.0));
        draw(&mut view);
        assert_ne!(view.rendered.unwrap().center, center);
        assert!(view.zoom_by(1));
        let zoomed = draw(&mut view);
        assert_eq!(view.rendered.unwrap().zoom_steps, EEW_AUTO_ZOOM_STEPS + 1);
        assert_eq!(view.rendered.unwrap().center, center);
        assert!((initial.step / zoomed.step - ZOOM_STEP).abs() < 1e-9);
        assert!(view.zoom_by(-1));
        assert!((draw(&mut view).step - initial.step).abs() < 1e-9);
        for _ in 0..EEW_AUTO_ZOOM_STEPS {
            assert!(view.zoom_by(-1));
            draw(&mut view);
        }
        assert_eq!(view.zoom_factor(), 1.0);
        assert!(!view.zoom_by(-1));

        let mut national = View::new(true);
        draw(&mut national);
        assert!(national.zoom_by(1));
        assert!(!national.national);
        draw(&mut national);
        assert_eq!(national.rendered.unwrap().center, center);
    }

    #[test]
    fn panning_stops_at_coverage_and_can_immediately_reverse() {
        let atlas = Atlas::load().unwrap();
        let bulletin = Bulletin::default();
        let mut view = View {
            center: Some((1000.0, -1000.0)),
            manual_zoom: Some(3),
            ..View::new(true)
        };
        let (width, height) = (240, 320);
        let extent = atlas.extent(&bulletin, &[], &mut view, width, height);
        let epsilon = 1e-8;
        assert!((extent.west + width as f64 * extent.step - atlas.coverage.east).abs() < epsilon);
        assert!(
            (extent.north - height as f64 * extent.step - atlas.coverage.south).abs() < epsilon
        );

        assert!(view.pan(-0.18, 0.18));
        let reversed = atlas.extent(&bulletin, &[], &mut view, width, height);
        assert!(reversed.west < extent.west);
        assert!(reversed.north > extent.north);

        view.manual_zoom = Some(0);
        view.center = Some((1000.0, 0.0));
        let wide_east = atlas.extent(&bulletin, &[], &mut view, 1000, height);
        view.center = Some((-1000.0, 0.0));
        let wide_west = atlas.extent(&bulletin, &[], &mut view, 1000, height);
        assert!((wide_east.west - wide_west.west).abs() < epsilon);
    }
}
