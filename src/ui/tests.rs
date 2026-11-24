use super::*;

#[test]
fn update_notification_only_marks_the_about_tab() {
    let now = Instant::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.details.next = now + Duration::from_secs(60);
    app.change_tab(Tab::Eew);
    assert_eq!(app.menu_items()[Tab::About.index()].label, "アプリ情報");
    let release = Release::newer("v1.2.3".into(), "0.1.0").unwrap();
    app.send.send(Reply::UpdateAvailable(release)).ok().unwrap();

    assert!(app.update(now));
    let items = app.menu_items();
    let about = &items[Tab::About.index()];
    assert_eq!(about.label, "アプリ情報 ↑更新");
    assert!(about.notice.is_none());
    assert!(items[Tab::Eew.index()].active);
    assert!(!app.take_eew_bell());
}

#[test]
fn demo_can_change_intensity_and_toggle_forecast_without_losing_warning_areas() {
    let now = Instant::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.change_tab(Tab::Demo);
    let original = app.current().unwrap().0.into_owned();
    assert!(app.demo.next.is_none());

    assert!(app.select(KeyCode::Char(' '), 80, now));
    assert!(app.demo.next.is_some());
    assert!(app.select(KeyCode::Right, 80, now));
    assert!(app.demo.next.is_none());
    assert_eq!(
        app.current().unwrap().0.maximum.as_ref().unwrap().label(),
        "5強"
    );
    for _ in 0..4 {
        assert!(app.select(KeyCode::Right, 80, now));
    }
    assert_eq!(
        app.current().unwrap().0.maximum.as_ref().unwrap().label(),
        "1"
    );

    assert!(app.select(KeyCode::Char('f'), 80, now));
    assert!(!app.current().unwrap().0.warning);
    assert!(app.current().unwrap().0.label.contains("予報"));
    assert!(app.current().unwrap().0.warning_areas.is_empty());
    assert!(app.select(KeyCode::Char('f'), 80, now));
    assert_eq!(
        app.current().unwrap().0.warning_areas,
        original.warning_areas
    );
    assert_eq!(app.current().unwrap().0.label, original.label);

    assert!(app.select(KeyCode::Char('r'), 80, now));
    assert_eq!(
        app.current().unwrap().0.maximum.as_ref().unwrap().label(),
        "5弱"
    );
    assert!(app.demo.next.is_none());
}

#[test]
fn demo_bell_plays_at_start_and_warning_escalation() {
    let now = Instant::now();
    let delay = Duration::from_secs(2);
    let mut app = App::new(Client::new(), delay, false);
    app.details.next = now + Duration::from_secs(60);
    app.change_tab(Tab::Demo);
    assert!(!app.take_eew_bell());
    assert!(app.select(KeyCode::Char(' '), 80, now));
    assert!(app.take_eew_bell());
    assert!(app.update(now + delay));
    assert!(!app.take_eew_bell());
    assert!(app.update(now + delay * 2));
    assert!(!app.take_eew_bell());

    app.select(KeyCode::Char('r'), 80, now);
    app.select(KeyCode::Char('f'), 80, now);
    assert!(!app.take_eew_bell());
    app.select(KeyCode::Char(' '), 80, now);
    assert!(app.take_eew_bell());
    assert!(app.update(now + delay));
    assert!(app.take_eew_bell());
    app.select(KeyCode::Char('f'), 80, now);
    assert!(!app.take_eew_bell());
    app.select(KeyCode::Char('f'), 80, now);
    assert!(app.take_eew_bell());
}

#[test]
fn help_scroll_and_close_preserve_the_underlying_view() {
    let now = Instant::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.page = Page::Text;
    app.scroll = 4;
    app.selection[0] = 2;
    app.view.zoom_by(2);
    let zoom = app.view.zoom_factor();

    assert!(app.select(KeyCode::Char('h'), 80, now));
    assert!(app.select(KeyCode::Down, 80, now));
    assert_eq!(app.help_scroll, Some(1));
    assert!(!app.select(KeyCode::Char('2'), 80, now));
    assert!(app.select(KeyCode::Esc, 80, now));
    assert!(app.help_scroll.is_none());
    assert!(app.tab == Tab::Quake && app.page == Page::Text);
    assert_eq!(app.scroll, 4);
    assert_eq!(app.selection[0], 2);
    assert_eq!(app.view.zoom_factor(), zoom);

    app.change_tab(Tab::Settings);
    assert!(app.select(KeyCode::Char('h'), 80, now));
    assert!(app.select(KeyCode::Char('H'), 80, now));
    assert!(app.help_scroll.is_none() && app.tab == Tab::Settings);
}

#[test]
fn live_wavefront_tracks_wolfx_time_and_disappears_on_disconnect_or_cancellation() {
    let now = Instant::now();
    let utc = chrono::Utc::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.change_tab(Tab::Eew);
    let report = Bulletin {
        provider: Provider::Wolfx,
        eew: true,
        eew_event_id: Some("wave-test".into()),
        origin: Some((utc - chrono::TimeDelta::seconds(20)).fixed_offset()),
        issued: Some((utc - chrono::TimeDelta::seconds(4)).fixed_offset()),
        epicenter: data::Location::new(142.5, 38.2),
        depth: Some(34),
        ..Bulletin::default()
    };
    for update in [
        LiveUpdate::Connection(Provider::Wolfx, None),
        LiveUpdate::Eew(Box::new(report.clone())),
    ] {
        app.send.send(Reply::Live(update)).unwrap();
    }
    app.update(now);
    assert!(!app.refresh_live_wavefront(now));
    app.send
        .send(Reply::Live(LiveUpdate::Clock(
            Provider::Wolfx,
            ServerClock { utc, received: now },
        )))
        .unwrap();
    app.update(now);
    assert!(app.refresh_live_wavefront(now));
    assert!(app.live_wavefront.is_some());
    app.send
        .send(Reply::Live(LiveUpdate::Clock(
            Provider::P2p,
            ServerClock {
                utc: utc + chrono::TimeDelta::seconds(60),
                received: now,
            },
        )))
        .unwrap();
    app.update(now);
    assert!(!app.refresh_live_wavefront(now + Duration::from_millis(250)));
    assert!(app.refresh_live_wavefront(now + Duration::from_millis(500)));

    let mut scene = Scene::single(&report, false, "");
    scene.view.wavefront = app.live_wavefront;
    let mut renderer = Renderer::new().unwrap();
    let frame = renderer.render(
        scene,
        Display {
            columns: 100,
            rows: 34,
            colors: true,
            map: Map::Image(Image::Sixel((10, 20))),
        },
    );
    assert!(frame.text.contains("P波面（推定）") && frame.text.contains("S波面（推定）"));
    assert!(frame.graphic.starts_with("\x1bP"));

    let mut cancelled = report.clone();
    cancelled.eew_cancelled = true;
    assert!(app.eew.wavefront(&cancelled, now).is_none());
    let mut assumed = report;
    assumed.plum_assumed_hypocenter = true;
    assert!(app.eew.wavefront(&assumed, now).is_none());
    app.send
        .send(Reply::Live(LiveUpdate::Connection(
            Provider::Wolfx,
            Some("offline".into()),
        )))
        .unwrap();
    app.update(now + Duration::from_secs(1));
    assert!(app.refresh_live_wavefront(now + Duration::from_secs(1)));
    assert!(app.live_wavefront.is_none());
}

#[test]
fn live_eew_alert_changes_refresh_the_quake_menu_and_match_by_id() {
    let now = Instant::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.details.next = now + Duration::from_secs(60);
    app.reports = vec![Report {
        id: "demo-event".into(),
        bulletin: Bulletin::default(),
    }]
    .into();
    let mut eew = Bulletin {
        eew: true,
        eew_event_id: Some("demo-event".into()),
        issued: Some(chrono::Utc::now().fixed_offset()),
        ..Bulletin::default()
    };
    app.send
        .send(Reply::Live(LiveUpdate::Clock(
            Provider::Wolfx,
            data::ServerClock {
                utc: eew.issued.unwrap().to_utc(),
                received: now,
            },
        )))
        .unwrap();
    app.send
        .send(Reply::Live(LiveUpdate::Eew(Box::new(eew.clone()))))
        .unwrap();
    assert!(app.update(now));
    assert_eq!(app.quake_notices().eew, Some(true));

    eew.warning = true;
    app.send
        .send(Reply::Live(LiveUpdate::Eew(Box::new(eew))))
        .unwrap();
    assert!(app.update(now));
    assert!(app.eew.active().unwrap().warning);
    assert!(app.select(KeyCode::Char('2'), 100, now));
    assert_eq!(app.eew_layer, EewLayer::Warning);
    assert!(app.select(KeyCode::Char('v'), 100, now));
    assert_eq!(app.eew_layer, EewLayer::Forecast);
    assert!(app.select(KeyCode::Char('v'), 100, now));
    assert_eq!(app.eew_layer, EewLayer::Warning);
}

#[test]
fn concurrent_eew_keeps_the_warning_visible_and_the_reports_selectable() {
    let now = Instant::now();
    let utc = chrono::Utc::now();
    let mut app = App::new(Client::new(), Duration::from_secs(2), false);
    app.details.next = now + Duration::from_secs(600);
    app.change_tab(Tab::Eew);
    let send = |update| app.send.send(Reply::Live(update)).unwrap();
    send(LiveUpdate::Clock(
        Provider::Wolfx,
        ServerClock { utc, received: now },
    ));
    for (id, name, age, warning) in [
        ("one", "熊本県熊本地方", 20, true),
        ("two", "宮城県沖", 5, false),
    ] {
        send(LiveUpdate::Eew(Box::new(Bulletin {
            eew: true,
            eew_event_id: Some(id.into()),
            name: name.into(),
            issued: Some((utc - chrono::TimeDelta::seconds(age)).fixed_offset()),
            warning,
            warning_areas: if warning {
                vec!["熊本県熊本".into()]
            } else {
                vec![]
            },
            ..Bulletin::default()
        })));
    }
    assert!(app.update(now));
    assert_eq!(app.current().unwrap().0.name, "宮城県沖");
    assert_eq!(app.menu_items()[Tab::Eew.index()].label, "EEW 警報");
    assert_eq!(app.feed_status().eew.0, "EEW 警報");
    assert_eq!(app.warning_seconds, None);

    let events: Vec<_> = app.eew.reports().collect();
    let mut rows = vec![];
    screen::append_eew_events(
        &mut rows,
        &events,
        app.eew_index(),
        Display {
            columns: 82,
            rows: 24,
            colors: true,
            map: Map::Text,
        },
    );
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains("\x1b[38;2;255;97;98m") && rows[0].contains("対象 熊本県熊本"));
    assert!(rows[1].contains("▸ 予報  宮城県沖"));

    let mut renderer = Renderer::new().unwrap();
    let focused = app.current().unwrap().0;
    for columns in [48, 82] {
        let display = Display {
            columns,
            rows: 24,
            colors: false,
            map: Map::Text,
        };
        let mut menu = screen::menu_lines(&app.menu_items(), display);
        screen::append_feed_status(&mut menu, app.feed_status(), display);
        screen::append_eew_events(&mut menu, &events, app.eew_index(), display);
        let mut scene = Scene::single(&focused, false, "");
        scene.menu = &menu;
        let text = &renderer.render(scene, display).text;
        let older = text.find("警報  熊本県熊本地方").unwrap();
        let newer = text.find("▸ 予報  宮城県沖").unwrap();
        let banner = text.find("緊急地震速報（予報）").unwrap();
        assert!(older < newer && newer < banner);
        assert!(text.lines().count() <= display.rows);
    }

    assert!(app.select(KeyCode::Up, 82, now));
    assert_eq!(app.current().unwrap().0.name, "熊本県熊本地方");
    assert_eq!(app.warning_seconds, Some(20));
    assert!(app.select(KeyCode::Down, 82, now));
    assert_eq!(app.current().unwrap().0.name, "宮城県沖");
    assert!(app.select(KeyCode::Up, 82, now));
    assert!(app.update(now + Duration::from_secs(100)));
    assert_eq!(app.current().unwrap().0.name, "宮城県沖");
    assert_eq!(app.menu_items()[Tab::Eew.index()].label, "EEW 予報");
}

#[test]
fn sixel_waits_for_the_end_of_a_resize_burst() {
    let mut resize = ResizeRedraw::default();
    let sixel = Map::Image(Image::Sixel((10, 20)));
    let now = Instant::now();
    let mut draw = |columns, millis, map| {
        let mut view = Display {
            columns,
            rows: 30,
            colors: true,
            map,
        };
        let redraw = resize.update(&mut view, now + Duration::from_millis(millis));
        (redraw, view.map)
    };
    assert_eq!(draw(80, 0, sixel), (true, sixel));
    assert_eq!(draw(120, 10, sixel), (true, Map::Braille));
    assert_eq!(draw(80, 260, sixel), (true, Map::Braille));
    assert_eq!(draw(80, 1259, sixel), (false, Map::Braille));
    assert_eq!(draw(80, 1260, sixel), (true, sixel));
    assert_eq!(draw(80, 1290, sixel), (false, sixel));
    let larger_cells = Map::Image(Image::Sixel((12, 24)));
    assert_eq!(draw(80, 1300, larger_cells), (true, Map::Braille));
    assert_eq!(draw(80, 2300, larger_cells), (true, larger_cells));
}

#[test]
fn inline_graphic_uses_relative_position_and_leaves_the_cursor_after_the_frame() {
    let frame = Frame {
        text: "title\nevent\ntime\n\n  list\n\nlegend".into(),
        graphic: "\x1bP7;1q~\x1b\\".into(),
        map_row: 5,
        map_col: 1,
        ..Frame::default()
    };
    let mut bytes = Vec::new();
    write_inline(&mut bytes, &frame).unwrap();
    let output = std::str::from_utf8(&bytes).unwrap();
    assert!(output.contains("legend\r\n\x1b7\x1b[3A\r\x1b[?80l\x1bP7;1q~\x1b\\\x1b8"));
}
