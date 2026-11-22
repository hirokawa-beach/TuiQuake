use super::{panels::*, raster::*, *};
use crate::graphics::{self, Image};

#[test]
fn reading_names_start_in_the_same_column_at_every_intensity() {
    let mut bulletin = crate::data::demo().pop().unwrap();
    bulletin.regions[0].level = Level::Known(4);
    bulletin.regions[1].level = Level::Known(3);
    bulletin.regions[2].level = Level::Other("5弱以上・未入電".into());
    let lines = readings(&bulletin, Layer::Regions, StationGrouping::Prefecture);
    assert!(lines[0].0.starts_with("5-  "));
    assert!(lines[1].0.starts_with(" 4  "));
    assert!(lines[2].0.starts_with(" —  "));
    assert!(lines[2].0.ends_with("（5弱以上・未入電）"));
}

#[test]
fn station_lists_group_by_prefecture_and_keep_the_heading_when_scrolling() {
    let bulletin = crate::data::demo().pop().unwrap();
    let rows = readings(&bulletin, Layer::Stations, StationGrouping::Prefecture);
    let headings: Vec<_> = rows
        .iter()
        .filter(|(line, _)| line.starts_with("── "))
        .map(|(line, _)| line.as_str())
        .collect();
    assert_eq!(
        headings,
        [
            "── 宮城県",
            "── 岩手県",
            "── 福島県",
            "── 山形県",
            "── 秋田県"
        ]
    );
    let (title, rows) = station_readings_window(&bulletin, StationGrouping::Prefecture, 4, 2);
    assert_eq!(title, "各地の震度 · 観測点  5–5/14");
    assert_eq!(rows[0].0, "── 岩手県");
    assert!(rows[1].0.contains("大船渡"));
    let (title, rows) = station_readings_window(&bulletin, StationGrouping::Prefecture, 3, 4);
    assert_eq!(title, "各地の震度 · 観測点  4–5/14");
    assert_eq!(rows[0].0, "── 宮城県");
    assert!(rows[1].0.contains("仙台"));
    assert_eq!(rows[2].0, "── 岩手県");
    assert!(rows[3].0.contains("大船渡"));
}

#[test]
fn station_lists_can_group_intensities_across_prefectures() {
    let mut bulletin = crate::data::demo().pop().unwrap();
    bulletin.stations[0].level = Level::Known(5);
    let rows = readings(&bulletin, Layer::Stations, StationGrouping::Intensity);
    assert_eq!(
        rows[0],
        ("── 震度 5+".into(), reading_rgb(&Level::Known(5)))
    );
    assert_eq!(rows[1].0, "石巻");
    assert_eq!(
        rows[2],
        ("── 震度 5-".into(), reading_rgb(&Level::Known(4)))
    );
    let four = rows.iter().position(|row| row.0 == "── 震度 4").unwrap();
    assert!(rows[four + 1].0.contains("気仙沼"));
    assert!(rows[four + 2].0.contains("仙台"));
    assert!(rows[four + 3].0.contains("大船渡"));
    let (title, window) = station_readings_window(&bulletin, StationGrouping::Intensity, 1, 2);
    assert!(title.contains("震度別"));
    assert_eq!(window[0].0, "── 震度 5-");
    assert_eq!(window[1].0, "南三陸");
}

#[test]
fn startup_picker_fits_and_keeps_the_selected_renderer_visible() {
    let options = [
        Map::Image(Image::Sixel((10, 20))),
        Map::Image(Image::Kitty),
        Map::Braille,
    ];
    let mut display = Display {
        columns: 120,
        rows: 30,
        colors: false,
        map: Map::Braille,
    };
    let wide = map_picker(&options, 1, display);
    assert!(wide.text.contains(LOGO[0]));
    assert!(wide.text.contains("▸ [2] Kitty"));
    assert!(wide.text.contains("点字で地図を描画"));
    let border = wide.text.lines().find(|line| line.contains("╭")).unwrap();
    assert_eq!(border.find('╭'), Some((display.columns - 1 - 60) / 2));

    for (columns, rows) in [
        (120, 30),
        (70, 14),
        (36, 8),
        (25, 5),
        (19, 4),
        (14, 2),
        (7, 1),
    ] {
        display.columns = columns;
        display.rows = rows;
        display.colors = true;
        let frame = map_picker(&options, 2, display);
        assert!(frame.text.contains("▸ [3]"), "{columns} × {rows}");
        assert!(frame.text.lines().count() <= rows);
        assert!(frame.text.lines().all(|line| styled_width(line) < columns));
    }
}

#[test]
fn about_scrolls_from_the_overview_to_the_credits_in_a_small_terminal() {
    let menu = ["TuiQuake".into()];
    let display = Display {
        columns: 42,
        rows: 21,
        colors: true,
        map: Map::Braille,
    };
    let mut scroll = 0;
    let first = about(&menu, display, &mut scroll);
    assert!(first.text.contains("地震・速報を見る"));
    assert!(first.text.contains("Unicode Braille"));
    assert!(!first.text.contains("Glenn Chappell"));
    assert!(first.text.lines().count() <= display.rows);
    assert!(
        first
            .text
            .lines()
            .all(|line| styled_width(line) <= display.width())
    );
    scroll = usize::MAX;
    let last = about(&menu, display, &mut scroll);
    assert!(last.text.contains("データとクレジット"));
    assert!(last.text.contains("Glenn Chappell"));
    assert!(scroll > 0 && scroll < usize::MAX);
    assert!(last.text.lines().count() <= display.rows);
}

#[test]
fn control_rows_wrap_whole_key_action_pairs() {
    let display = Display {
        columns: 28,
        rows: 24,
        colors: true,
        map: Map::Braille,
    };
    let lines = control_lines(
        "地図",
        &[
            ("WASD", "地図を移動"),
            ("+/-", "拡大/縮小"),
            ("Z", "全国/周辺"),
        ],
        display,
    );
    assert!(
        lines
            .iter()
            .all(|line| styled_width(line) <= display.width())
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[+/-]") && line.contains("拡大/縮小"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[Z]") && line.contains("全国/周辺"))
    );
    assert!(lines[0].contains("\x1b[38;2;125;213;210m[WASD]"));
}

#[test]
fn interactive_eew_keeps_overview_and_warning_regions_separate() {
    let mut renderer = Renderer::new().unwrap();
    let mut bulletin = crate::data::demo().remove(1);
    let menu = ["TuiQuake".into()];
    for columns in [80, 48] {
        let display = Display {
            columns,
            rows: 28,
            colors: true,
            map: Map::Image(Image::Sixel((10, 20))),
        };
        let scene = || Scene {
            menu: &menu,
            eew_source: Some("Wolfx"),
            ..Scene::single(&bulletin, false, "")
        };
        let frame = renderer.render(scene(), display);
        let headline = frame.text.lines().nth(1).unwrap();
        assert!(headline.contains("緊急地震速報（警報） 第2報（最終報）"));
        assert!(headline.contains("\x1b[48;2;255;97;98m"));
        assert!(!headline.contains("Wolfx"));
        assert_eq!(styled_width(headline), columns - 1);
        let overview = frame.text.find("速報概要 · Wolfx").unwrap();
        let intensity = frame.text.find("推定最大").unwrap();
        let warning_areas = frame.text.find("警報対象地域").unwrap();
        assert!(overview < intensity && intensity < warning_areas);
        assert!(frame.text[intensity..warning_areas].contains(" ▄▄▄▄  ▄  "));
        assert!(frame.text[intensity..warning_areas].contains(" █    ▀█▀ "));
        assert!(frame.text[intensity..warning_areas].contains(" ▀▀▀█     "));
        assert!(frame.text[intensity..warning_areas].contains("\x1b[48;2;255;82;90m"));
        assert!(frame.text[intensity..warning_areas].contains("宮城県沖"));
        assert!(!frame.text[intensity..warning_areas].contains("震源 宮城県沖"));
        assert!(frame.text[intensity..warning_areas].contains("M6.1"));
        assert!(frame.text.lines().all(|line| styled_width(line) < columns));
        assert!(
            frame
                .text
                .lines()
                .nth(frame.map_row - 2)
                .unwrap()
                .contains("地図")
        );
    }

    bulletin.maximum = Some(Level::Known(4));
    bulletin.warning = false;
    bulletin.warning_areas.clear();
    let frame = renderer.render(
        Scene {
            menu: &menu,
            ..Scene::single(&bulletin, false, "DEMO 架空 2/3")
        },
        Display {
            columns: 88,
            rows: 24,
            colors: false,
            map: Map::Braille,
        },
    );
    let headline = frame.text.lines().nth(1).unwrap();
    assert_eq!(headline.trim(), "緊急地震速報（予報） 第2報（最終報）");
    let indent = headline.len() - headline.trim_start().len();
    assert_eq!(indent, (87 - headline.trim().width()) / 2);
    assert!(frame.text.contains("DEMO 架空 2/3"));
    assert!(frame.text.contains("速報概要"));
    assert!(!frame.text.contains("警報対象地域"));
    assert!(frame.text.contains("地域別の予想震度"));
    assert!(frame.text.contains("5-〜5+"));
    assert!(frame.text.contains(" █    ▀▀▀ "));
    assert!(!frame.text.contains(" █    ▀█▀ "));
}

#[test]
fn eew_banner_wraps_narrow_forecasts_and_does_not_color_cancellations() {
    let mut bulletin = crate::data::demo().remove(1);
    bulletin.warning = false;
    let rows = eew_banner(&bulletin, 20, true);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains("緊急地震速報（予報）"));
    assert!(rows[1].contains("第2報（最終報）"));
    assert!(
        rows.iter()
            .all(|row| row.contains("\x1b[48;2;255;222;94m") && styled_width(row) == 20)
    );
    bulletin.eew_cancelled = true;
    let rows = eew_banner(&bulletin, 48, true);
    assert_eq!(rows[0].trim(), "緊急地震速報（取消） 第2報");
}

#[test]
fn earthquake_overview_displays_the_badge_facts_and_event_notices() {
    let mut bulletin = crate::data::demo().pop().unwrap();
    bulletin.maximum = Some(Level::Known(0));
    let lines = earthquake_summary(
        &bulletin,
        QuakeNotices {
            eew: Some(true),
            tsunami: Some("心配なし"),
        },
        38,
        6,
        true,
    );
    assert!(lines[0].contains("最大震度"));
    assert!(lines[1].contains("\x1b[48;2;119;213;255m"));
    assert!(lines[1].contains("宮城県沖"));
    assert!(lines[2].contains("M6.1"));
    assert!(lines[3].contains("深さ 45 km"));
    assert!(lines[4].contains("EEW あり"));
    assert!(lines[5].contains("津波 心配なし"));
    assert!(lines.iter().all(|line| styled_width(line) <= 38));
}

#[test]
fn menu_colors_forecasts_yellow_and_warnings_red() {
    let display = Display {
        columns: 80,
        rows: 24,
        colors: true,
        map: Map::Braille,
    };
    let mut item = MenuItem {
        key: '2',
        label: "EEW 予報",
        active: false,
        notice: Some(MenuNotice::Forecast),
    };
    assert!(
        menu_lines(&[item], display)
            .join("\n")
            .contains("\x1b[38;2;255;222;94m")
    );
    item = MenuItem {
        key: '2',
        label: "EEW 警報",
        active: true,
        notice: Some(MenuNotice::Warning),
    };
    assert!(
        menu_lines(&[item], display)
            .join("\n")
            .contains("\x1b[38;2;255;97;98m")
    );
}

#[test]
fn eew_centers_unsigned_digits_in_a_fixed_width_badge() {
    let mut bulletin = crate::data::demo().remove(1);
    bulletin.maximum = Some(Level::Known(2));
    let three = eew_overview(&bulletin, 44, 16, true, None);
    assert!(three[2].contains("   ▄▄▄▄   \x1b[0m"));
    assert!(three[4].contains("   ▀▀▀█   \x1b[0m"));
    assert!(three[5].contains("   ▄▄▄█   \x1b[0m"));
    assert!(three[6].contains("\x1b[38;2;244;232;113m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));
    assert!(!three[6].contains("\x1b[48;"));

    bulletin.maximum = Some(Level::Known(3));
    let four = eew_overview(&bulletin, 44, 16, true, None);
    assert!(four[4].contains("  ▀▀▀█▀▀  \x1b[0m"));
    assert!(four[6].contains("\x1b[38;2;255;166;72m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));

    bulletin.maximum = Some(Level::Known(0));
    let one = eew_overview(&bulletin, 44, 16, true, None);
    assert!(one[5].contains("   ▄▄█▄   \x1b[0m"));

    bulletin.maximum = Some(Level::Known(5));
    let five_plus = eew_overview(&bulletin, 44, 16, true, None);
    assert!(five_plus[6].contains("\x1b[38;2;255;82;90m▀▀▀▀▀▀▀▀▀▀\x1b[0m"));
}

#[test]
fn monochrome_text_can_have_a_sixel_map() {
    let mut renderer = Renderer::new().unwrap();
    let bulletin = crate::data::demo().remove(0);
    let frame = renderer.render(
        Scene::single(&bulletin, false, "DEMO"),
        Display {
            columns: 80,
            rows: 24,
            colors: false,
            map: Map::Image(Image::Sixel((10, 20))),
        },
    );
    assert!(frame.graphic.starts_with("\x1bP7;1q\"1;1;440;"));
    assert!(frame.text.contains("警報対象地域"));
}

#[test]
fn kitty_keeps_a_text_map_when_the_image_is_filtered() {
    let mut renderer = Renderer::new().unwrap();
    let bulletin = crate::data::demo().remove(0);
    let frame = renderer.render(
        Scene::single(&bulletin, true, "DEMO"),
        Display {
            columns: 80,
            rows: 24,
            colors: true,
            map: Map::Image(Image::Kitty),
        },
    );
    assert!(frame.graphic.starts_with(graphics::KITTY_COMMAND));
    assert!(frame.text.contains(|glyph| QUADRANTS[1..].contains(&glyph)));
}

#[test]
fn quadrant_map_represents_both_sides_of_a_cell() {
    let [land, sea] = [map::LAND, map::SEA].map(|color| [color; SAMPLES_PER_QUADRANT as usize]);
    assert_eq!(text_cell([land, sea, land, sea], None, false).0, '▌');
    assert_eq!(text_cell([sea, land, sea, land], None, false).0, '▐');
    let (glyph, ink, paper) = text_cell([land, sea, land, sea], None, true);
    assert_eq!(
        (glyph, ink, paper),
        ('▌', map_ink(map::LAND), map_ink(map::SEA))
    );
    assert_eq!(glyph.width(), Some(1));
}

#[test]
fn braille_places_dots_in_one_terminal_cell() {
    let [land, sea] = [map::LAND, map::SEA].map(|color| [color; 4]);
    assert_eq!(
        braille_cell([land, sea, land, sea, land, sea, land, sea], true).0,
        '⡇'
    );
    let right = braille_cell([sea, land, sea, land, sea, land, sea, land], false).0;
    assert_eq!(right, '⢸');
    assert_eq!(right.width(), Some(1));
}

#[test]
fn text_map_keeps_observations_and_eew_visible() {
    let land = [[map::LAND; SAMPLES_PER_QUADRANT as usize]; 4];
    assert_eq!(
        text_cell(land, Some(map::EPICENTER), true),
        ('✚', map_ink(map::EPICENTER), PALETTE[map::LAND as usize])
    );
    assert_eq!(text_cell(land, Some(map::STATION + 1), false).0, '●');
    let mut warning = [[map::SEA; SAMPLES_PER_QUADRANT as usize]; 4];
    warning[3][7] = map::WARNING;
    assert_eq!(text_cell(warning, None, false).0, '█');
}

#[test]
fn switching_to_stations_draws_dots_without_previous_region_labels() {
    let mut renderer = Renderer::new().unwrap();
    let bulletin = crate::data::demo().pop().unwrap();
    for map in [Map::Braille, Map::Image(Image::Sixel((10, 20)))] {
        let display = Display {
            columns: 100,
            rows: 30,
            colors: false,
            map,
        };
        let mut scene = Scene::single(&bulletin, false, "TEST");
        scene.layer = Layer::Regions;
        renderer.render(scene, display);
        let raster = if map.image().is_some() {
            &renderer.image_raster
        } else {
            &renderer.raster
        };
        assert!(raster.labels.iter().any(Option::is_some));
        let frame = renderer.render(Scene::single(&bulletin, false, "TEST"), display);
        if map.image().is_some() {
            assert!(frame.graphic.starts_with("\x1bP"));
            assert!(renderer.image_raster.labels.is_empty());
            assert!(
                renderer
                    .image_raster
                    .pixels
                    .iter()
                    .any(|&pixel| pixel >= map::STATION)
            );
        } else {
            assert!(frame.text.contains('●'));
            assert!(renderer.raster.labels.is_empty());
        }
    }
}

#[test]
fn national_region_map_includes_intensity_labels() {
    let mut renderer = Renderer::new().unwrap();
    let bulletin = crate::data::demo().pop().unwrap();
    let mut scene = Scene::single(&bulletin, true, "TEST");
    scene.layer = Layer::Regions;
    renderer.render(
        scene,
        Display {
            columns: 112,
            rows: 34,
            colors: false,
            map: Map::Text,
        },
    );
    assert!(
        renderer
            .raster
            .labels
            .iter()
            .flatten()
            .any(|label| label.glyph.is_ascii_digit())
    );
}

#[test]
fn header_groups_event_facts_and_wraps_timestamps_on_a_narrow_terminal() {
    let bulletin = crate::data::demo().pop().unwrap();
    let wide = event_header(&bulletin, "地震 1/100 · Wolfx", 79, false);
    assert_eq!(wide[0], "TuiQuake  地震 1/100 · Wolfx");
    assert!(wide[1].contains("最大震度 5弱"));
    assert!(wide[2].contains("M6.1  ·  深さ 45 km"));
    assert_eq!(wide[3], "発生 01/01 14:32:00   発表 14:34:00 JST");
    let narrow = event_header(&bulletin, "", 25, false);
    assert_eq!(
        &narrow[narrow.len() - 2..],
        ["発生 01/01 14:32:00 JST", "発表 01/01 14:34:00 JST"]
    );
}

#[test]
fn earthquake_sidebar_shows_observations_and_selected_history_in_a_standard_terminal() {
    let mut renderer = Renderer::new().unwrap();
    let report = Report {
        id: "event".into(),
        bulletin: Bulletin {
            name: "三陸沖".into(),
            maximum: Some(crate::data::Level::Known(2)),
            regions: vec![crate::data::Observation {
                level: crate::data::Level::Known(2),
                name: "宮城県北部".into(),
                location: None,
                area_code: String::new(),
                prefecture: None,
            }],
            ..Bulletin::default()
        },
    };
    let menu = ["TuiQuake  [1 地震]  2 EEW  3 津波  4 デモ".into()];
    let reports = [report];
    let mut scene = Scene::single(&reports[0].bulletin, true, "");
    scene.menu = &menu;
    scene.layer = Layer::Regions;
    scene.history = Some(History {
        reports: &reports,
        selected: 0,
        status: "",
        row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
    });
    let frame = renderer.render(
        scene,
        Display {
            columns: 80,
            rows: 24,
            colors: false,
            map: Map::Braille,
        },
    );
    assert!(
        frame
            .text
            .lines()
            .any(|line| line.contains("╭─ 地震の概要"))
    );
    assert!(frame.text.lines().any(|line| line.contains("宮城県北部")));
    assert!(
        frame
            .text
            .lines()
            .any(|line| line.contains("│ >") && line.contains("三陸沖"))
    );
    assert!(frame.text.lines().all(|line| line.width() < 80));
    assert!(frame.text.lines().count() <= 24);
    let frame = renderer.render(
        Scene {
            menu: &menu,
            history: Some(History {
                reports: &reports,
                selected: 0,
                status: "",
                row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
            }),
            ..Scene::single(&reports[0].bulletin, true, "")
        },
        Display {
            columns: 59,
            rows: 24,
            colors: true,
            map: Map::Braille,
        },
    );
    assert!(frame.text.lines().all(|line| styled_width(line) <= 58));
}

#[test]
fn history_stays_in_place_and_colors_each_known_intensity() {
    use crate::data::{Level, Observation};

    let quiet = Bulletin::default();
    let observed = Bulletin {
        name: "紀伊水道".into(),
        maximum: Some(Level::Known(8)),
        stations: vec![
            Observation {
                name: "観測点".into(),
                level: Level::Known(2),
                location: None,
                area_code: String::new(),
                prefecture: None,
            };
            25
        ],
        ..Bulletin::default()
    };
    let reports = [
        Report {
            id: "1".into(),
            bulletin: observed.clone(),
        },
        Report {
            id: "2".into(),
            bulletin: quiet.clone(),
        },
    ];
    let menu = ["TuiQuake".into()];
    let display = Display {
        columns: 100,
        rows: 38,
        colors: true,
        map: Map::Braille,
    };
    let mut renderer = Renderer::new().unwrap();
    let mut render = |bulletin, selected, reading_start| {
        let scene = Scene {
            menu: &menu,
            history: Some(History {
                reports: &reports,
                selected,
                status: "",
                row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
            }),
            reading_start,
            ..Scene::single(bulletin, true, "")
        };
        renderer.render(scene, display).text.clone()
    };
    let with_readings = render(&observed, 0, 0);
    let at_end = render(&observed, 0, 24);
    let without_readings = render(&quiet, 1, 0);
    let history_row = |text: &str| {
        text.lines()
            .position(|line| line.contains("地震履歴"))
            .unwrap()
    };
    assert_eq!(history_row(&with_readings), history_row(&at_end));
    assert_eq!(history_row(&with_readings), history_row(&without_readings));
    let seven = with_readings
        .lines()
        .find(|line| line.contains("紀伊水道") && line.contains(">"))
        .unwrap();
    assert!(seven.contains("\x1b[48;2;158;95;235m\x1b[38;2;0;0;0m 7  "));
    assert!(seven.matches("\x1b[48;2;37;70;87m").count() >= 2);
    let unknown = without_readings
        .lines()
        .find(|line| line.contains("震源地不明") && line.contains(">"))
        .unwrap();
    assert!(!unknown.contains("\x1b[48;2;158;95;235m"));
    assert_eq!(styled_width(seven), 99);
}

#[test]
fn readings_fill_the_space_above_bottom_aligned_history() {
    let bulletin = crate::data::demo().pop().unwrap();
    let reports = [Report {
        id: "event".into(),
        bulletin: bulletin.clone(),
    }];
    let scene = Scene::single(&bulletin, false, "");
    let render = |row_limit, status| {
        earthquake_sidebar(
            &scene,
            History {
                reports: &reports,
                selected: 0,
                status,
                row_limit,
            },
            60,
            38,
            false,
        )
    };
    let normal = render(10, "");
    let short = render(4, "");
    let retrying = render(10, "Wolfx 再接続中…");
    let tall_history = render(50, "");
    let top = |rows: &[String], name: &str| {
        rows.iter()
            .position(|line| line.starts_with('╭') && line.contains(name))
            .unwrap()
    };
    let readings_bounds = |rows: &[String]| {
        let start = top(rows, "観測点");
        let end = rows
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.starts_with('╰'))
            .unwrap()
            .0;
        (start, end)
    };
    let (normal_top, normal_bottom) = readings_bounds(&normal);
    assert_eq!(readings_bounds(&short), (normal_top, normal_bottom + 6));
    assert_eq!(readings_bounds(&retrying), (normal_top, normal_bottom));
    assert_eq!(readings_bounds(&tall_history), (normal_top, normal_top + 3));
    for rows in [normal, short, retrying, tall_history] {
        assert_eq!(rows.len(), 38);
        assert_eq!(readings_bounds(&rows).1 + 1, top(&rows, "地震履歴"));
        assert!(rows[37].starts_with('╰'));
    }
}

#[test]
fn wide_map_and_earthquake_sidebar_use_the_available_width() {
    let mut renderer = Renderer::new().unwrap();
    let bulletin = crate::data::demo().pop().unwrap();
    let reports: Vec<_> = (0..50)
        .map(|i| Report {
            id: i.to_string(),
            bulletin: Bulletin {
                name: format!("試験地震 {i:02}"),
                ..bulletin.clone()
            },
        })
        .collect();
    let mut scene = Scene::single(&bulletin, true, "地震 1/50");
    let menu = ["TuiQuake  [1 地震]  2 EEW  3 津波  4 デモ".into()];
    scene.menu = &menu;
    scene.station_grouping = StationGrouping::Prefecture;
    scene.history = Some(History {
        reports: &reports,
        selected: 0,
        status: "",
        row_limit: crate::settings::DEFAULT_HISTORY_ROWS,
    });
    let frame = renderer.render(
        scene,
        Display {
            columns: 160,
            rows: 48,
            colors: false,
            map: Map::Image(Image::Sixel((10, 20))),
        },
    );
    assert!(frame.graphic.starts_with("\x1bP7;1q\"1;1;730;840"));
    assert_eq!(frame.map_row, 4);
    assert_eq!(frame.map_col, 2);
    assert!(frame.text.lines().next().unwrap().contains("3 津波"));
    let top = frame.text.lines().nth(frame.map_row - 2).unwrap();
    assert!(top.starts_with("╭─ 地図 · 全国"));
    assert!(top.contains("╭─ 地震の概要"));
    assert_eq!(top.width(), 159);
    assert!(
        frame
            .text
            .lines()
            .any(|line| line.contains("╭─ 各地の震度 · 観測点"))
    );
    assert!(frame.text.lines().any(|line| line.contains("│ ── 宮城県")));
    assert!(frame.text.lines().any(|line| line.contains("╭─ 地震履歴")));
    assert!(
        frame
            .text
            .lines()
            .any(|line| line.starts_with('╰') && line.matches('╰').count() == 2)
    );
    let entries = frame
        .text
        .lines()
        .filter(|line| line.contains("試験地震"))
        .count();
    assert_eq!(entries, 10);
    let compact = compact(
        History {
            reports: &reports,
            selected: 20,
            status: "",
            row_limit: 4,
        },
        Display {
            columns: 50,
            rows: 25,
            colors: false,
            map: Map::Text,
        },
        &menu,
    );
    assert_eq!(
        compact
            .text
            .lines()
            .filter(|line| line.contains("試験地震"))
            .count(),
        4
    );
    assert!(
        compact
            .text
            .lines()
            .any(|line| line.contains("│ >") && line.contains("試験地震 20"))
    );
}
