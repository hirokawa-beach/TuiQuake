const DEPTH_COUNT: usize = 106;
const DISTANCE_COUNT: usize = 236;
// Each row holds interleaved P and S travel times in little-endian milliseconds.
const TABLE: &[u8; DEPTH_COUNT * DISTANCE_COUNT * 8] = include_bytes!("../../data/jma2001.bin");

fn axis(index: usize) -> f64 {
    match index {
        0..25 => (index * 2) as f64,
        25..55 => (50 + (index - 25) * 5) as f64,
        _ => (200 + (index - 55) * 10) as f64,
    }
}

fn time(depth: usize, distance: usize, phase: usize) -> f64 {
    let offset = (depth * DISTANCE_COUNT + distance) * 8 + phase * 4;
    f64::from(u32::from_le_bytes(
        TABLE[offset..offset + 4].try_into().unwrap(),
    )) / 1000.0
}

pub(super) fn radii(depth_km: u32, elapsed: f64) -> [Option<f64>; 2] {
    let depth = f64::from(depth_km);
    if depth > axis(DEPTH_COUNT - 1) {
        return [None; 2];
    }
    let top = (0..DEPTH_COUNT - 1)
        .find(|&index| axis(index + 1) >= depth)
        .unwrap();
    let fraction = (depth - axis(top)) / (axis(top + 1) - axis(top));
    std::array::from_fn(|phase| {
        let arrival = |distance| {
            let upper = time(top, distance, phase);
            upper + (time(top + 1, distance, phase) - upper) * fraction
        };
        // Invert the interpolated travel times at the current elapsed time.
        let (mut lower, mut higher) = (0, DISTANCE_COUNT - 1);
        if elapsed <= arrival(lower) || elapsed > arrival(higher) {
            return None;
        }
        while higher - lower > 1 {
            let middle = (lower + higher) / 2;
            if arrival(middle) < elapsed {
                lower = middle;
            } else {
                higher = middle;
            }
        }
        let start = arrival(lower);
        let fraction = (elapsed - start) / (arrival(higher) - start);
        Some(axis(lower) + (axis(higher) - axis(lower)) * fraction)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_table_samples_and_rejects_out_of_range_values() {
        assert_eq!(radii(0, 2.052)[0], Some(10.0));
        assert_eq!(radii(0, 4.135)[1], Some(12.0));
        assert_eq!(radii(0, (2.052 + 2.448) / 2.0)[0], Some(11.0));
        let halfway = (time(0, 5, 0) + time(1, 5, 0)) / 2.0;
        assert_eq!(radii(1, halfway)[0], Some(10.0));
        assert_eq!(radii(700, 0.0), [None; 2]);
        assert_eq!(radii(701, 80.0), [None; 2]);
        assert_eq!(radii(0, 1000.0), [None; 2]);
    }
}
