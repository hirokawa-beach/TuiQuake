use std::sync::OnceLock;

use crate::data::{TsunamiArea, TsunamiLevel};

use super::*;

struct Coast {
    paths: Vec<Vec<(f64, f64)>>,
    bounds: Bounds,
}

struct CoastMap {
    districts: HashMap<String, Coast>,
    bounds: Bounds,
}

fn coasts() -> &'static CoastMap {
    static COASTS: OnceLock<CoastMap> = OnceLock::new();
    COASTS.get_or_init(|| {
        let source: HashMap<String, Vec<Vec<[f64; 2]>>> =
            serde_json::from_str(include_str!("../../data/tsunami_coasts.json"))
                .expect("津波予報区");
        let districts: HashMap<_, _> = source
            .into_iter()
            .filter_map(|(code, paths)| {
                let paths: Vec<Vec<_>> = paths
                    .into_iter()
                    .map(|path| {
                        path.into_iter()
                            .map(|[lon, lat]| project(Location { lon, lat }))
                            .collect()
                    })
                    .collect();
                let bounds = paths
                    .iter()
                    .flatten()
                    .map(|&(x, y)| Bounds::point(x, y))
                    .reduce(Bounds::merge)?;
                Some((code, Coast { paths, bounds }))
            })
            .collect();
        let bounds = districts
            .values()
            .map(|coast| coast.bounds)
            .reduce(Bounds::merge)
            .expect("津波予報区の範囲");
        CoastMap { districts, bounds }
    })
}

pub fn has_coast(code: &str) -> bool {
    coasts().districts.contains_key(code)
}

pub fn tsunami_color(level: TsunamiLevel) -> Option<Color> {
    Some(match level {
        TsunamiLevel::MajorWarning => TSUNAMI_MAJOR_WARNING,
        TsunamiLevel::Warning => TSUNAMI_WARNING,
        TsunamiLevel::Advisory => TSUNAMI_ADVISORY,
        TsunamiLevel::Forecast => TSUNAMI_FORECAST,
        TsunamiLevel::Cleared | TsunamiLevel::None | TsunamiLevel::Unknown => return None,
    })
}

impl Atlas {
    pub fn draw_tsunami(
        &self,
        raster: &mut Raster,
        areas: &[TsunamiArea],
        mut view: View,
        image: bool,
    ) -> View {
        let coast_map = coasts();
        let national = self.national.merge(coast_map.bounds);
        let active = || {
            areas.iter().filter_map(|area| {
                coast_map
                    .districts
                    .get(&area.code)
                    .zip(tsunami_color(area.level))
            })
        };
        let focus = if view.national {
            national
        } else {
            active()
                .map(|(coast, _)| coast.bounds)
                .reduce(Bounds::merge)
                .unwrap_or(national)
        };
        let extent = self.fit_view(
            national,
            focus,
            u16::MAX,
            &mut view,
            (raster.width, raster.height),
        );
        self.draw_base(raster, extent, std::iter::repeat(LAND), image);
        let radius = if image { 2 } else { 1 };
        let to_pixel = |(x, y): (f64, f64)| {
            [
                (x - extent.west) / extent.step,
                (extent.north - y) / extent.step,
            ]
        };
        for (coast, color) in active() {
            for path in &coast.paths {
                for segment in path.windows(2) {
                    raster.stroke_line(to_pixel(segment[0]), to_pixel(segment[1]), radius, color);
                }
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsunami_coast_codes_load_from_the_embedded_geography() {
        assert_eq!(coasts().districts.len(), 66);
        for code in ["101", "210", "710", "800", "802"] {
            assert!(has_coast(code), "{code}");
        }
        assert!(!has_coast("999"));
    }
}
