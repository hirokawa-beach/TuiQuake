use std::{
    borrow::Cow,
    collections::VecDeque,
    fmt::Write as _,
    io::{self, IsTerminal, Write},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};

use crate::{
    data::{self, Bulletin, Client, EewUpdate, INTENSITIES, Level, Report, Result, Tsunami},
    graphics::{self, Image, KITTY_COMMAND, Map},
    map::{Layer, View},
    screen::{
        self, APP_NAME, Display, Frame, History, MIN_SPLIT_WIDTH, MenuNotice, QuakeNotices,
        Renderer, Scene,
    },
};

pub fn tty() -> bool {
    io::stdout().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

pub fn ansi() -> bool {
    if !tty() {
        return false;
    }
    #[cfg(windows)]
    {
        crossterm::ansi_support::supports_ansi()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub fn display(colors: bool, map: Map) -> Display {
    let (columns, rows) = terminal::size().unwrap_or((88, 29));
    Display {
        columns: usize::from(columns),
        rows: usize::from(rows),
        colors,
        map,
    }
}

pub struct Console {
    output: String,
    sixel_visible: bool,
    kitty_visible: bool,
    #[cfg(windows)]
    previous_title: Option<Vec<u16>>,
}

impl Console {
    pub fn enter() -> Result<Self> {
        if !io::stdin().is_terminal() || !ansi() {
            return Err(
                "TUI は対話端末で起動してください。出力だけなら latest、eew、replay が使えます。"
                    .into(),
            );
        }
        terminal::enable_raw_mode()?;
        let console = Self {
            output: String::with_capacity(32768),
            sixel_visible: false,
            kitty_visible: false,
            #[cfg(windows)]
            previous_title: set_console_title(),
        };
        let mut out = io::stdout().lock();
        // The alternate buffer otherwise turns the wheel into arrow-key presses.
        out.write_all(b"\x1b[?1049h\x1b[?1007l\x1b[?25l\x1b[2J")?;
        out.flush()?;
        Ok(console)
    }

    pub fn paint(&mut self, frame: &Frame) -> io::Result<()> {
        self.output.clear();
        self.output.push_str("\x1b[?2026h");
        let id = std::process::id();
        if self.kitty_visible {
            write!(self.output, "{KITTY_COMMAND}a=d,d=I,i={id},q=2\x1b\\").unwrap();
            self.kitty_visible = false;
        }
        let sixel = !frame.graphic.is_empty() && !frame.graphic.starts_with(KITTY_COMMAND);
        if self.sixel_visible || sixel {
            self.output.push_str("\x1b[2J");
        }
        self.sixel_visible = sixel;
        self.output.push_str("\x1b[H");
        for (i, line) in frame.text.split('\n').enumerate() {
            if i > 0 {
                self.output.push_str("\x1b[K\r\n");
            }
            self.output.push_str(line);
        }
        self.output.push_str("\x1b[K\x1b[J");
        if !frame.graphic.is_empty() {
            write!(self.output, "\x1b[{};{}H", frame.map_row, frame.map_col).unwrap();
            if let Some(command) = frame.graphic.strip_prefix(KITTY_COMMAND) {
                write!(self.output, "{KITTY_COMMAND}i={id},{command}").unwrap();
                self.kitty_visible = true;
            } else {
                self.output.push_str("\x1b[?80l");
                self.output.push_str(&frame.graphic);
            }
        }
        self.output.push_str("\x1b[?2026l");
        let mut output = io::stdout().lock();
        output.write_all(self.output.as_bytes())?;
        output.flush()
    }
}

fn choose_map(console: &mut Console, colors: bool) -> Result<Option<Map>> {
    let options: Vec<_> = graphics::detect()
        .images()
        .map(Map::Image)
        .chain([Map::Braille])
        .collect();
    let mut selected = 0;
    loop {
        let view = display(colors, Map::Braille);
        let frame = screen::map_picker(&options, selected, view);
        console.paint(&frame)?;

        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if quit(key) || key.code == KeyCode::Esc {
                    return Ok(None);
                }
                match key.code {
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(options.len() - 1),
                    KeyCode::Enter => return Ok(Some(options[selected])),
                    KeyCode::Char(digit @ '1'..='3') => {
                        if let Some(&map) = options.get((digit as u8 - b'1') as usize) {
                            return Ok(Some(map));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

pub fn write_inline(out: &mut impl Write, frame: &Frame) -> io::Result<()> {
    if frame.graphic.is_empty() {
        return writeln!(out, "{}", frame.text);
    }

    // Print the rows first so the frame can start anywhere in the scrollback.
    // Paint its map over the placeholder and restore the prompt's position.
    let back = frame.text.split('\n').count() - (frame.map_row - 1);
    out.write_all(b"\x1b[?2026h")?;
    for line in frame.text.split('\n') {
        write!(out, "{line}\r\n")?;
    }
    let sixel_mode = if frame.graphic.starts_with(KITTY_COMMAND) {
        ""
    } else {
        "\x1b[?80l"
    };
    write!(out, "\x1b7\x1b[{back}A\r")?;
    if frame.map_col > 1 {
        write!(out, "\x1b[{}C", frame.map_col - 1)?;
    }
    write!(out, "{sixel_mode}{}\x1b8\x1b[?2026l", frame.graphic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_can_change_intensity_and_toggle_forecast_without_losing_warning_areas() {
        let now = Instant::now();
        let mut app = App::new(Client::new(), Duration::from_secs(2), false);
        assert!(app.select(KeyCode::Char('4'), 80, now));
        let original = app.current().0.into_owned();
        assert!(app.demo_next.is_none());

        assert!(app.select(KeyCode::Char(' '), 80, now));
        assert!(app.demo_next.is_some());
        assert!(app.select(KeyCode::Right, 80, now));
        assert!(app.demo_next.is_none());
        assert_eq!(app.current().0.maximum.as_ref().unwrap().label(), "5強");
        for _ in 0..4 {
            assert!(app.select(KeyCode::Right, 80, now));
        }
        assert_eq!(app.current().0.maximum.as_ref().unwrap().label(), "1");

        assert!(app.select(KeyCode::Char('f'), 80, now));
        assert!(!app.current().0.warning);
        assert!(app.current().0.label.contains("予報"));
        assert!(app.current().0.warning_areas.is_empty());
        assert!(app.select(KeyCode::Char('f'), 80, now));
        assert_eq!(app.current().0.warning_areas, original.warning_areas);
        assert_eq!(app.current().0.label, original.label);

        assert!(app.select(KeyCode::Char('r'), 80, now));
        assert_eq!(app.current().0.maximum.as_ref().unwrap().label(), "5弱");
        assert!(app.demo_next.is_none());
    }

    #[test]
    fn live_eew_alert_changes_refresh_the_quake_menu_and_match_by_id() {
        let now = Instant::now();
        let mut app = App::new(Client::new(), Duration::from_secs(2), false);
        app.next_refresh = [now + Duration::from_secs(60); 3];
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
            .send(Reply::Eew(EewUpdate::Report(Box::new(eew.clone()))))
            .unwrap();
        assert!(app.update(now));
        assert_eq!(app.quake_notices().eew, Some(true));

        eew.warning = true;
        app.send
            .send(Reply::Eew(EewUpdate::Report(Box::new(eew))))
            .unwrap();
        assert!(app.update(now));
        assert!(app.eew.unwrap().warning);
    }

    #[test]
    fn sixel_waits_for_the_end_of_a_resize_burst() {
        let mut resize = ResizeRedraw::default();
        let sixel = Map::Image(Image::Sixel((10, 20)));
        let now = Instant::now();
        let mut draw = |columns, millis| {
            let mut view = Display {
                columns,
                rows: 30,
                colors: true,
                map: sixel,
            };
            let redraw = resize.update(&mut view, now + Duration::from_millis(millis));
            (redraw, view.map)
        };
        assert_eq!(draw(80, 0), (true, sixel));
        assert_eq!(draw(120, 10), (true, Map::Braille));
        assert_eq!(draw(80, 260), (true, Map::Braille));
        assert_eq!(draw(80, 1259), (false, Map::Braille));
        assert_eq!(draw(80, 1260), (true, sixel));
        assert_eq!(draw(80, 1290), (false, sixel));
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
}

#[cfg(windows)]
fn set_console_title() -> Option<Vec<u16>> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        System::Console::{GetConsoleTitleW, SetConsoleTitleW},
    };

    let mut previous = vec![0; 32768];
    let title: Vec<_> = APP_NAME.encode_utf16().chain([0]).collect();
    // Both buffers are writable or null-terminated as required by the console APIs.
    unsafe {
        SetLastError(0);
        let len = GetConsoleTitleW(previous.as_mut_ptr(), previous.len() as u32) as usize;
        if len >= previous.len() || (len == 0 && GetLastError() != 0) {
            return None;
        }
        previous.truncate(len + 1);
        previous[len] = 0;
        (SetConsoleTitleW(title.as_ptr()) != 0).then_some(previous)
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        let mut out = io::stdout().lock();
        if self.kitty_visible {
            let id = std::process::id();
            let _ = write!(out, "{KITTY_COMMAND}a=d,d=I,i={id},q=2\x1b\\");
        }
        let _ = out.write_all(b"\x1b[0m\x1b[?2026l\x1b[?25h\x1b[?1049l\x1b[?1007h");
        let _ = out.flush();
        let _ = terminal::disable_raw_mode();
        #[cfg(windows)]
        if let Some(title) = &self.previous_title {
            // The saved title includes a trailing null.
            unsafe { windows_sys::Win32::System::Console::SetConsoleTitleW(title.as_ptr()) };
        }
    }
}

fn quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q' | 'Q'))
        || matches!(key.code, KeyCode::Char('c' | 'C'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub fn wait_or_quit(delay: Duration) -> Result<bool> {
    let deadline = Instant::now() + delay;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !event::poll(left)? {
            return Ok(false);
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Release && quit(key) {
                return Ok(true);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Quake,
    Eew,
    Tsunami,
    Demo,
    About,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Quake,
        Self::Eew,
        Self::Tsunami,
        Self::Demo,
        Self::About,
    ];

    fn cycle(self, backwards: bool) -> Self {
        let step = if backwards { Self::ALL.len() - 1 } else { 1 };
        Self::ALL[(self.index() + step) % Self::ALL.len()]
    }
    fn index(self) -> usize {
        self as usize
    }
    fn label(self) -> &'static str {
        match self {
            Self::Quake => "地震",
            Self::Eew => "EEW",
            Self::Tsunami => "津波",
            Self::Demo => "デモ",
            Self::About => "About",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Map,
    Text,
    List,
}

enum Reply {
    Reports(Result<Arc<[Report]>>),
    Eew(EewUpdate),
    Tsunami(Result<Tsunami>),
}

pub struct App {
    client: Client,
    send: Sender<Reply>,
    incoming: Receiver<Reply>,
    tab: Tab,
    page: Page,
    selection: [usize; Tab::ALL.len()],
    scroll: usize,
    reading_start: usize,
    pub view: View,
    layer: Layer,
    reports: Arc<[Report]>,
    eew: Option<Bulletin>,
    received_eew_ids: VecDeque<String>,
    tsunami: Option<Tsunami>,
    tsunami_current: bool,
    demo: Vec<Bulletin>,
    demo_hidden_areas: Vec<Vec<String>>,
    demo_delay: Duration,
    demo_next: Option<Instant>,
    next_refresh: [Instant; 3],
    pending: [bool; 3],
    received: [bool; 3],
    source_error: [bool; 3],
}

impl App {
    pub fn new(client: Client, delay: Duration, national: bool) -> Self {
        let (send, incoming) = mpsc::channel();
        let now = Instant::now();
        let demo = data::demo();
        let demo_hidden_areas = vec![Vec::new(); demo.len()];
        Self {
            client,
            send,
            incoming,
            tab: Tab::Quake,
            page: Page::Map,
            selection: [0; Tab::ALL.len()],
            scroll: 0,
            reading_start: 0,
            view: View::new(national),
            layer: Layer::Regions,
            reports: Arc::default(),
            eew: None,
            received_eew_ids: VecDeque::new(),
            tsunami: None,
            tsunami_current: false,
            demo,
            demo_hidden_areas,
            demo_delay: delay.max(Duration::from_millis(150)),
            demo_next: None,
            next_refresh: [now; 3],
            pending: [false; 3],
            received: [false; 3],
            source_error: [false; 3],
        }
    }

    fn len(&self) -> usize {
        match self.tab {
            Tab::Quake => self.reports.len(),
            Tab::Eew => usize::from(self.eew.is_some()),
            Tab::Tsunami | Tab::About => 0,
            Tab::Demo => self.demo.len(),
        }
    }

    fn change_tab(&mut self, next: Tab) {
        if self.tab != next {
            self.view.reset();
            if next == Tab::Demo {
                self.selection[next.index()] = 0;
                self.demo_next = None;
            }
        }
        self.tab = next;
        self.page = Page::Map;
        self.scroll = 0;
    }

    fn select(&mut self, key: KeyCode, width: usize, now: Instant) -> bool {
        // Windows IMEs may commit a full-width plus. Numpad plus arrives as '+'.
        let key = match key {
            KeyCode::Char('＋') => KeyCode::Char('+'),
            KeyCode::Char('－' | '−') => KeyCode::Char('-'),
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        match key {
            KeyCode::Char(digit @ '1'..='5') => {
                self.change_tab(Tab::ALL[(digit as u8 - b'1') as usize]);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.change_tab(self.tab.cycle(key == KeyCode::BackTab));
            }
            KeyCode::Esc if self.tab == Tab::About => self.change_tab(Tab::Quake),
            KeyCode::Up if self.tab == Tab::About => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down if self.tab == Tab::About => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Home if self.tab == Tab::About => self.scroll = 0,
            KeyCode::End if self.tab == Tab::About => self.scroll = usize::MAX,
            _ if self.tab == Tab::About => return false,
            KeyCode::Char('b') | KeyCode::Esc | KeyCode::Backspace => {
                self.page = if self.tab == Tab::Quake
                    && width < MIN_SPLIT_WIDTH
                    && self.page == Page::Map
                {
                    Page::List
                } else {
                    Page::Map
                };
            }
            KeyCode::Enter if self.tab == Tab::Quake && self.len() > 0 => {
                self.page = Page::Map;
                self.view = View::new(false);
            }
            KeyCode::Char('i') if self.len() > 0 => {
                self.page = if self.page == Page::Text {
                    Page::Map
                } else {
                    Page::Text
                };
                self.scroll = 0;
            }
            KeyCode::Char('v')
                if self.page != Page::List && self.current().0.has_observations() =>
            {
                self.layer = match self.layer {
                    Layer::Regions => Layer::Stations,
                    Layer::Stations => Layer::Regions,
                };
                self.scroll = 0;
                self.reading_start = 0;
            }
            KeyCode::Left | KeyCode::Right
                if self.tab == Tab::Quake && self.page == Page::Map && width >= MIN_SPLIT_WIDTH =>
            {
                let count =
                    self.reports
                        .get(self.selection[0])
                        .map_or(0, |report| match self.layer {
                            Layer::Regions => report.bulletin.regions.len(),
                            Layer::Stations => report.bulletin.stations.len(),
                        });
                let direction = if key == KeyCode::Left { -1 } else { 1 };
                self.reading_start = self
                    .reading_start
                    .saturating_add_signed(direction)
                    .min(count.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Down => {
                let direction = if key == KeyCode::Up { -1 } else { 1 };
                if self.page == Page::Text || self.tab == Tab::Tsunami {
                    self.scroll = self.scroll.saturating_add_signed(direction);
                } else {
                    let limit = self.len().saturating_sub(1);
                    let selected = &mut self.selection[self.tab.index()];
                    let next = selected.saturating_add_signed(direction).min(limit);
                    if *selected != next {
                        *selected = next;
                        self.reading_start = 0;
                        if !self.view.national {
                            self.view.reset();
                        }
                    }
                    self.demo_next = None;
                }
            }
            KeyCode::Char(' ') if self.tab == Tab::Demo => {
                self.page = Page::Map;
                if self.demo_next.take().is_none() {
                    let selected = &mut self.selection[self.tab.index()];
                    if *selected + 1 == self.demo.len() {
                        *selected = 0;
                    }
                    self.demo_next = Some(now + self.demo_delay);
                }
            }
            KeyCode::Char('r') if self.tab == Tab::Demo => {
                self.demo = data::demo();
                self.demo_hidden_areas = vec![Vec::new(); self.demo.len()];
                self.selection[self.tab.index()] = 0;
                self.demo_next = None;
                self.page = Page::Map;
                self.scroll = 0;
                self.view.reset();
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('f') if self.tab == Tab::Demo => {
                let index = self.selection[self.tab.index()];
                let bulletin = &mut self.demo[index];
                if !bulletin.eew || self.page != Page::Map {
                    return false;
                }
                self.demo_next = None;
                if key == KeyCode::Char('f') {
                    let previous = if bulletin.warning { "警報" } else { "予報" };
                    let next = if bulletin.warning { "予報" } else { "警報" };
                    bulletin.label = bulletin.label.replacen(previous, next, 1);
                    bulletin.warning = !bulletin.warning;
                    std::mem::swap(
                        &mut bulletin.warning_areas,
                        &mut self.demo_hidden_areas[index],
                    );
                } else {
                    let current = bulletin
                        .maximum
                        .as_ref()
                        .and_then(Level::index)
                        .unwrap_or(0);
                    let direction = if key == KeyCode::Left { -1 } else { 1 };
                    let next =
                        (i16::from(current) + direction).rem_euclid(INTENSITIES.len() as i16);
                    bulletin.maximum = Some(Level::Known(next as u8));
                }
            }
            _ if self.tab == Tab::Tsunami && key != KeyCode::Char('r') => return false,
            KeyCode::Char('z') => self.view = View::new(!self.view.national),
            KeyCode::Char('+' | '=' | ';') => return self.view.zoom_by(1),
            KeyCode::Char('-') => return self.view.zoom_by(-1),
            KeyCode::Char('w') => return self.view.pan(0.0, 0.18),
            KeyCode::Char('a') => return self.view.pan(-0.18, 0.0),
            KeyCode::Char('s') => return self.view.pan(0.0, -0.18),
            KeyCode::Char('d') => return self.view.pan(0.18, 0.0),
            KeyCode::Char('0') => self.view.reset(),
            KeyCode::Char('r') if matches!(self.tab, Tab::Quake | Tab::Tsunami) => {
                self.next_refresh[self.tab.index()] = now;
            }
            _ => return false,
        }
        true
    }

    fn update(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while let Ok(message) = self.incoming.try_recv() {
            match message {
                Reply::Reports(result) => {
                    self.pending[0] = false;
                    let status_changed =
                        !self.received[0] || self.source_error[0] != result.is_err();
                    self.received[0] = true;
                    self.source_error[0] = result.is_err();
                    changed |= self.tab == Tab::Quake && status_changed;
                    if let Ok(reports) = result
                        && !Arc::ptr_eq(&self.reports, &reports)
                    {
                        let previous = self.reports.get(self.selection[0]);
                        self.selection[0] =
                            keep_selection(&self.reports, &reports, self.selection[0], |item| {
                                &item.id
                            });
                        let next = reports.get(self.selection[0]);
                        if previous.map(|item| &item.id) != next.map(|item| &item.id) {
                            self.reading_start = 0;
                            if self.tab == Tab::Quake {
                                self.scroll = 0;
                                if !self.view.national {
                                    self.view.reset();
                                }
                            }
                        }
                        self.reports = reports;
                        changed |= self.tab == Tab::Quake;
                    }
                }
                Reply::Eew(update) => match update {
                    EewUpdate::Connected | EewUpdate::Disconnected => {
                        let disconnected = matches!(update, EewUpdate::Disconnected);
                        changed |= self.tab == Tab::Eew
                            && (!self.received[1] || self.source_error[1] != disconnected);
                        self.received[1] = true;
                        self.source_error[1] = disconnected;
                    }
                    EewUpdate::Report(report) => {
                        if let Some(id) = &report.eew_event_id
                            && !self.received_eew_ids.contains(id)
                        {
                            if self.received_eew_ids.len() >= 256 {
                                self.received_eew_ids.pop_front();
                            }
                            self.received_eew_ids.push_back(id.clone());
                            changed |= self.tab == Tab::Quake;
                        }
                        if self
                            .eew
                            .as_ref()
                            .is_none_or(|current| current.issued <= report.issued)
                        {
                            let tone_changed = self
                                .eew
                                .as_ref()
                                .map(|current| (current.warning, current.eew_cancelled))
                                != Some((report.warning, report.eew_cancelled));
                            changed |= tone_changed || self.tab == Tab::Eew;
                            if self.tab == Tab::Eew
                                && self
                                    .eew
                                    .as_ref()
                                    .is_none_or(|current| current.issued != report.issued)
                            {
                                self.scroll = 0;
                            }
                            self.eew = Some(*report);
                        }
                    }
                },
                Reply::Tsunami(result) => {
                    let previous_notice = self.quake_notices().tsunami;
                    self.pending[2] = false;
                    self.received[2] = true;
                    self.source_error[2] = result.is_err();
                    if let Ok(tsunami) = result {
                        if self
                            .tsunami
                            .as_ref()
                            .map(|previous| (previous.title, previous.issued))
                            != Some((tsunami.title, tsunami.issued))
                            && self.tab == Tab::Tsunami
                        {
                            self.scroll = 0;
                        }
                        self.tsunami = Some(tsunami);
                    }
                    changed |= self.tab == Tab::Tsunami
                        || (self.tab == Tab::Quake
                            && previous_notice != self.quake_notices().tsunami);
                }
            }
        }
        if self.eew.as_ref().is_some_and(|bulletin| !bulletin.fresh()) {
            self.eew = None;
            if self.tab == Tab::Eew {
                self.page = Page::Map;
            }
            changed = true;
        }
        if matches!(self.tab, Tab::Quake | Tab::Tsunami) {
            let current = self
                .tsunami
                .as_ref()
                .is_some_and(|tsunami| tsunami.current(chrono::Utc::now()));
            changed |= self.tsunami_current != current;
            self.tsunami_current = current;
        }
        for tab in [Tab::Quake, Tab::Tsunami] {
            let i = tab.index();
            let visible = self.tab == tab || (tab == Tab::Tsunami && self.tab == Tab::Quake);
            if !visible || self.pending[i] || now < self.next_refresh[i] {
                continue;
            }
            self.pending[i] = true;
            self.next_refresh[i] =
                now + Duration::from_secs(if tab == Tab::Quake { 60 } else { 30 });
            let client = self.client.clone();
            let send = self.send.clone();
            thread::spawn(move || {
                let reply = if tab == Tab::Quake {
                    Reply::Reports(client.reports())
                } else {
                    Reply::Tsunami(client.tsunami())
                };
                let _ = send.send(reply);
            });
        }
        if self.tab == Tab::Demo && self.demo_next.is_some_and(|at| now >= at) {
            let i = &mut self.selection[Tab::Demo.index()];
            *i = (*i + 1).min(self.demo.len() - 1);
            self.demo_next = (*i + 1 < self.demo.len()).then(|| now + self.demo_delay);
            changed = true;
        }
        changed
    }

    fn current(&self) -> (Cow<'_, Bulletin>, String) {
        let i = self.tab.index();
        let selected = self.selection[i];
        let (bulletin, banner) = match self.tab {
            Tab::Quake => self.reports.get(selected).map(|report| {
                (
                    Cow::Borrowed(&report.bulletin),
                    format!("地震 {}/{} · Wolfx", selected + 1, self.reports.len()),
                )
            }),
            Tab::Eew => self.eew.as_ref().map(|bulletin| {
                (
                    Cow::Borrowed(bulletin),
                    format!("{} · Wolfx", bulletin.label),
                )
            }),
            Tab::Tsunami | Tab::About => unreachable!(),
            Tab::Demo => self.demo.get(selected).map(|frame| {
                let mut banner = format!("DEMO 架空 {}/{}", selected + 1, self.demo.len());
                if self.page == Page::Text || !frame.eew {
                    banner.push_str("  ");
                    banner.push_str(&frame.label);
                }
                (Cow::Borrowed(frame), banner)
            }),
        }
        .unwrap_or_else(|| {
            let label = match (self.tab, self.received[i], self.source_error[i]) {
                (Tab::Eew, _, true) => "EEW接続が切れました。再接続中…",
                (Tab::Eew, false, _) => "EEWに接続中…",
                (Tab::Eew, true, false) => "EEWを受信していません",
                (_, _, true) => "取得できませんでした  R 再取得",
                (_, false, _) => "取得中…",
                _ => "地震情報がありません",
            };
            (
                Cow::Owned(Bulletin {
                    label: label.into(),
                    ..Bulletin::default()
                }),
                self.tab.label().into(),
            )
        });
        let banner = if self.len() > 0 && self.tab != Tab::Demo && self.source_error[i] {
            banner
                + if self.tab == Tab::Eew {
                    "  再接続中…"
                } else {
                    "  更新失敗"
                }
        } else {
            banner
        };
        (bulletin, banner)
    }

    pub fn paint(
        &mut self,
        renderer: &mut Renderer,
        console: &mut Console,
        display: Display,
    ) -> Result<()> {
        let items = Tab::ALL.map(|tab| {
            let bulletin = match tab {
                Tab::Eew => self.eew.as_ref(),
                Tab::Demo if tab == self.tab => self.demo.get(self.selection[tab.index()]),
                _ => None,
            };
            let (label, notice) = match (tab, bulletin.filter(|bulletin| bulletin.eew)) {
                (Tab::Eew, Some(bulletin)) if bulletin.eew_cancelled => ("EEW 取消", None),
                (Tab::Eew, Some(bulletin)) if bulletin.warning => {
                    ("EEW 警報", Some(MenuNotice::Warning))
                }
                (Tab::Eew, Some(_)) => ("EEW 予報", Some(MenuNotice::Forecast)),
                (Tab::Demo, Some(bulletin)) if bulletin.warning => {
                    ("デモ 警報", Some(MenuNotice::Warning))
                }
                (Tab::Demo, Some(_)) => ("デモ 予報", Some(MenuNotice::Forecast)),
                _ => (tab.label(), None),
            };
            screen::MenuItem {
                key: (b'1' + tab.index() as u8) as char,
                label,
                active: tab == self.tab,
                notice,
            }
        });
        let menu = screen::menu_lines(&items, display);
        if self.tab == Tab::About {
            return Ok(console.paint(&screen::about(&menu, display, &mut self.scroll))?);
        }
        if self.tab == Tab::Tsunami {
            let (status, alert, lines) = self.tsunami_display();
            let frame = screen::panel(
                &menu,
                "津波情報",
                status,
                alert,
                &lines,
                display,
                &mut self.scroll,
                "R 更新  Q 終了",
            );
            return Ok(console.paint(&frame)?);
        }
        if self.page == Page::List && display.width() >= MIN_SPLIT_WIDTH {
            self.page = Page::Map;
        }
        let (bulletin, banner) = self.current();
        if self.tab == Tab::Eew && self.eew.is_none() {
            let mut scroll = 0;
            let frame = screen::panel(
                &menu,
                "緊急地震速報",
                &bulletin.label,
                false,
                &[],
                display,
                &mut scroll,
                "Q 終了",
            );
            return Ok(console.paint(&frame)?);
        }
        let status = if self.source_error[0] {
            "取得失敗  R 再取得"
        } else {
            ""
        };
        let history = (self.tab == Tab::Quake).then_some(History {
            reports: &self.reports,
            selected: self.selection[0],
            status,
        });
        if self.page == Page::List {
            return Ok(console.paint(&screen::compact(
                history.expect("earthquake list"),
                display,
                &menu,
            ))?);
        }
        if self.page == Page::Text {
            let mut scroll = self.scroll;
            let frame = screen::detail(&bulletin, &banner, self.layer, display, &mut scroll, &menu);
            self.scroll = scroll;
            return Ok(console.paint(&frame)?);
        }
        let mut viewing = Vec::new();
        let (group, navigation) = match self.tab {
            Tab::Demo => {
                let playback = if self.demo_next.is_some() {
                    "停止"
                } else {
                    "再生"
                };
                let mut navigation = vec![("↑↓", "報を選ぶ"), ("Space", playback)];
                if bulletin.eew {
                    navigation.extend([("←→", "震度を変更"), ("F", "予報/警報")]);
                }
                viewing.push(("R", "デモ初期化"));
                ("デモ", navigation)
            }
            Tab::Quake => {
                let mut navigation = vec![("↑↓", "地震を選ぶ")];
                if display.width() < MIN_SPLIT_WIDTH {
                    navigation.push(("B", "履歴を開く"));
                } else if bulletin.has_observations() {
                    navigation.push(("←→", "震度一覧をスクロール"));
                }
                if self.len() > 0 {
                    navigation.push(("Enter", "震源周辺へ"));
                }
                viewing.push(("R", "情報を更新"));
                ("地震", navigation)
            }
            Tab::Eew => ("EEW", Vec::new()),
            Tab::Tsunami | Tab::About => unreachable!(),
        };
        if bulletin.has_observations() {
            viewing.insert(0, ("V", "広域/観測点"));
        }
        if self.len() > 0 {
            viewing.insert(
                usize::from(bulletin.has_observations()),
                ("I", "詳細を開く"),
            );
        }
        viewing.push(("Q", "終了"));
        let map_controls = [
            ("WASD", "移動"),
            ("+/-", "拡大/縮小"),
            ("Z", "全国/周辺"),
            ("0", "位置と倍率を戻す"),
        ];
        let mut controls = Vec::new();
        for (heading, bindings) in [
            (group, navigation.as_slice()),
            ("表示", viewing.as_slice()),
            ("地図", &map_controls),
        ] {
            if !bindings.is_empty() {
                controls.extend(screen::control_lines(heading, bindings, display));
            }
        }
        let series = if self.tab == Tab::Demo {
            &self.demo
        } else {
            &[] as &[Bulletin]
        };
        let live_eew = self.tab == Tab::Eew;
        let scene = Scene {
            bulletin: &bulletin,
            banner: if live_eew {
                if self.source_error[Tab::Eew.index()] {
                    "再接続中…"
                } else {
                    ""
                }
            } else {
                &banner
            },
            eew_source: live_eew.then_some("Wolfx"),
            series,
            view: self.view,
            layer: self.layer,
            controls: &controls,
            history,
            menu: &menu,
            reading_start: self.reading_start,
            quake_notices: if self.tab == Tab::Quake {
                self.quake_notices()
            } else {
                QuakeNotices::default()
            },
        };
        let frame = renderer.render(scene, display);
        if let Some(view) = frame.map_view {
            self.view = view;
        }
        Ok(console.paint(frame)?)
    }

    fn quake_notices(&self) -> QuakeNotices {
        let Some(report) = self.reports.get(self.selection[Tab::Quake.index()]) else {
            return QuakeNotices::default();
        };
        let received = self.received_eew_ids.contains(&report.id);
        let eew = received.then_some(true).or(report.bulletin.eew_notice);
        let tsunami = self
            .tsunami
            .as_ref()
            .filter(|tsunami| tsunami.current(chrono::Utc::now()))
            .and_then(|tsunami| tsunami.notice_for(&report.id))
            .or(report.bulletin.tsunami_notice);
        QuakeNotices { eew, tsunami }
    }

    fn tsunami_display(&self) -> (&'static str, bool, Vec<String>) {
        let failed = self.source_error[2];
        let Some(tsunami) = &self.tsunami else {
            return (
                if failed {
                    "津波情報を取得できませんでした"
                } else {
                    "津波情報を取得中…"
                },
                false,
                Vec::new(),
            );
        };
        let mut lines = Vec::new();
        let status = if self.tsunami_current {
            if let Some(issued) = tsunami.issued {
                lines.push(format!("気象庁 発表 {} JST", issued.format("%m/%d %H:%M")));
            }
            lines.extend(tsunami.areas.iter().cloned());
            tsunami.title
        } else {
            "現在の津波の状況を確認できません"
        };
        if failed {
            lines.insert(0, "Wolfx の更新に失敗しました".into());
        }
        (status, self.tsunami_current && tsunami.alert, lines)
    }
}

fn keep_selection<T>(old: &[T], next: &[T], selected: usize, key: impl Fn(&T) -> &str) -> usize {
    if selected == 0 || next.is_empty() {
        return 0;
    }
    old.get(selected)
        .and_then(|item| next.iter().position(|other| key(other) == key(item)))
        .unwrap_or(selected.min(next.len() - 1))
}

const SIXEL_RESIZE_QUIET: Duration = Duration::from_secs(1);

#[derive(Default)]
struct ResizeRedraw {
    dimensions: Option<(usize, usize)>,
    sixel_at: Option<Instant>,
}

impl ResizeRedraw {
    fn update(&mut self, view: &mut Display, now: Instant) -> bool {
        let dimensions = (view.columns, view.rows);
        let previous = self.dimensions.replace(dimensions);
        let changed = previous != Some(dimensions);
        if changed && previous.is_some() && matches!(view.map, Map::Image(Image::Sixel(_))) {
            self.sixel_at = Some(now + SIXEL_RESIZE_QUIET);
        }
        if self.sixel_at.is_some_and(|deadline| now >= deadline) {
            self.sixel_at = None;
            return true;
        }
        if self.sixel_at.is_some() {
            view.map = Map::Braille;
        }
        changed
    }
}

pub fn run(
    client: Client,
    renderer: &mut Renderer,
    colors: bool,
    map: Option<Map>,
    delay: Duration,
    national: bool,
) -> Result<()> {
    let mut console = Console::enter()?;
    let map = if let Some(map) = map {
        map
    } else {
        let Some(map) = choose_map(&mut console, colors)? else {
            return Ok(());
        };
        map
    };
    let mut app = App::new(client, delay, national);
    let send = app.send.clone();
    thread::spawn(move || data::stream_eew(|update| send.send(Reply::Eew(update)).is_ok()));
    let mut resize = ResizeRedraw::default();
    loop {
        let mut changed = app.update(Instant::now());
        if event::poll(Duration::from_millis(35))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if quit(key) {
                        return Ok(());
                    }
                    let code = if key.code == KeyCode::Tab
                        && key.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        KeyCode::BackTab
                    } else {
                        key.code
                    };
                    changed |= app.select(code, display(colors, map).width(), Instant::now());
                }
                _ => {}
            }
        }
        let mut view = display(colors, map);
        changed |= resize.update(&mut view, Instant::now());
        if changed {
            if app.tab == Tab::About {
                view.map = map;
            }
            app.paint(renderer, &mut console, view)?;
        }
    }
}
