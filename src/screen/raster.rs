use crate::map::{self, Color, PALETTE, Raster, Rgb, TEXT_CELL_HEIGHT, TEXT_CELL_WIDTH};

use super::RESET;

pub(super) fn map_ink(color: Color) -> Rgb {
    match color {
        map::LAND => (112, 174, 179),
        map::SURROUNDINGS => (34, 51, 66),
        _ => PALETTE[color as usize],
    }
}

pub(super) const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];
pub(super) const SAMPLES_PER_QUADRANT: u16 = (TEXT_CELL_WIDTH * TEXT_CELL_HEIGHT / 4) as u16;

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

pub(super) fn text_cell(
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

pub(super) fn braille_cell(pixels: [[Color; 4]; 8], colors: bool) -> (char, Rgb, Rgb) {
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

pub(super) fn text_row(out: &mut String, raster: &Raster, row: usize, colors: bool, braille: bool) {
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
