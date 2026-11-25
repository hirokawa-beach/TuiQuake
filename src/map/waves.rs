use super::{Extent, Location, P_WAVE, Raster, S_WAVE, S_WAVE_OUTLINE, project, travel_times};

// Constant-speed teaching model for the fictional demo.
const EARTH_RADIUS_KM: f64 = 6371.0;
const P_SPEED: f64 = 7.0;
const S_SPEED: f64 = 4.0;

fn surface_radius(speed: f64, seconds: f64, depth: f64) -> Option<f64> {
    let distance = speed * seconds;
    (distance > depth).then(|| (distance.powi(2) - depth.powi(2)).sqrt())
}

#[derive(Clone, Copy, PartialEq)]
pub struct Wavefront([Option<f64>; 2]);

impl Wavefront {
    pub fn demo(depth: u32, seconds: f64) -> Self {
        Self([P_SPEED, S_SPEED].map(|speed| surface_radius(speed, seconds, f64::from(depth))))
    }

    pub fn live(depth: u32, seconds: f64) -> Option<Self> {
        let radii = travel_times::radii(depth, seconds);
        radii.iter().any(Option::is_some).then_some(Self(radii))
    }
}

impl Raster {
    pub(super) fn draw_wavefront(
        &mut self,
        center: Location,
        wavefront: Wavefront,
        extent: Extent,
        image: bool,
    ) {
        let (sin_lat, cos_lat) = center.lat.to_radians().sin_cos();
        for (radius, color) in wavefront.0.into_iter().zip([P_WAVE, S_WAVE]) {
            let Some(radius) = radius else {
                continue;
            };
            let (sin_dist, cos_dist) = (radius / EARTH_RADIUS_KM).sin_cos();
            let point = |bearing: u16| {
                let (sin_bearing, cos_bearing) = f64::from(bearing).to_radians().sin_cos();
                let lat = (sin_lat * cos_dist + cos_lat * sin_dist * cos_bearing)
                    .clamp(-1.0, 1.0)
                    .asin();
                let lon = (sin_bearing * sin_dist * cos_lat).atan2(cos_dist - sin_lat * lat.sin());
                let (x, y) = project(Location {
                    lon: center.lon + lon.to_degrees(),
                    lat: lat.to_degrees(),
                });
                [
                    (x - extent.west) / extent.step,
                    (extent.north - y) / extent.step,
                ]
            };
            let mut from = point(0);
            for bearing in 1..=360 {
                let to = point(bearing);
                if image && color == S_WAVE {
                    self.stroke_line(from, to, 1, S_WAVE_OUTLINE);
                }
                self.stroke_line(from, to, 0, color);
                from = to;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_reach_the_surface_at_different_times() {
        assert!(surface_radius(P_SPEED, 7.0, 50.0).is_none());
        assert!(surface_radius(P_SPEED, 10.0, 50.0).is_some());
        assert!(surface_radius(S_SPEED, 10.0, 50.0).is_none());
        let mut raster = Raster::default();
        raster.resize(450, 450);
        let center = Location {
            lon: 142.0,
            lat: 38.0,
        };
        let (x, y) = project(center);
        raster.draw_wavefront(
            center,
            Wavefront::demo(45, 18.0),
            Extent {
                west: x - 2.0,
                north: y + 2.0,
                step: 0.01,
            },
            true,
        );
        assert!(raster.pixels.contains(&P_WAVE));
        assert!(raster.pixels.contains(&S_WAVE));
        assert!(raster.pixels.contains(&S_WAVE_OUTLINE));
    }
}
