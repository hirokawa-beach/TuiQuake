use std::{
    borrow::Cow,
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
    data::{
        self, Bulletin, Client, INTENSITIES, Level, LiveQuery, LiveUpdate, Provider, Report,
        Result, ServerClock, Source, TsunamiStatus,
    },
    eew,
    graphics::{self, Capabilities, Image, KITTY_COMMAND, Map, MapMode},
    map::{self, EewLayer, Layer, View},
    screen::{
        self, AboutInfo, Display, FeedStatus, Frame, HelpOverlay, HelpTopic, History,
        MIN_SPLIT_WIDTH, MenuNotice, QuakeNotices, Renderer, Scene,
    },
    settings::{DEFAULT_HISTORY_ROWS, Settings, StationGrouping},
};

mod demo;
mod settings_menu;
mod tsunami_tab;

use demo::DemoState;
use settings_menu::SettingsMenu;
use tsunami_tab::TsunamiTab;

pub fn tty() -> bool {
    io::stdout().is_terminal() && std::env::var("TERM").as_deref() != Ok("dumb")
}

pub fn ansi() -> bool {
    #[cfg(windows)]
    {
        tty() && crossterm::ansi_support::supports_ansi()
    }
    #[cfg(not(windows))]
    {
        tty()
    }
}

pub fn display(colors: bool, map: Map) -> Display {
    let (columns, rows) = terminal::size().unwrap_or((88, 29));
    // Windows Terminal uses virtual Sixel pixels. Other terminals can change
    // their pixel size per cell when the font size changes.
    #[cfg(unix)]
    let map = match map {
        Map::Image(Image::Sixel(cell)) => {
            Map::Image(Image::Sixel(graphics::terminal_cell().unwrap_or(cell)))
        }
        _ => map,
    };
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

    pub fn ring(&self) -> io::Result<()> {
        let mut output = io::stdout().lock();
        output.write_all(b"\x07")?;
        output.flush()
    }

    pub fn paint(&mut self, frame: &Frame) -> io::Result<()> {
        self.paint_layers(frame, None)
    }

    fn paint_layers(&mut self, frame: &Frame, overlay: Option<&HelpOverlay>) -> io::Result<()> {
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
        if let Some(overlay) = overlay {
            if overlay.colors {
                map::rgb(&mut self.output, (102, 115, 122), false);
            } else {
                self.output.push_str("\x1b[2m");
            }
        }
        for (i, line) in frame.text.split('\n').enumerate() {
            if i > 0 {
                self.output.push_str("\x1b[K\r\n");
            }
            self.output.push_str(line);
        }
        self.output.push_str("\x1b[K\x1b[J");
        if overlay.is_some() {
            self.output.push_str("\x1b[0m");
        }
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
        if let Some(overlay) = overlay {
            for (index, line) in overlay.lines.iter().enumerate() {
                let row = overlay.row + index;
                write!(self.output, "\x1b[{row};{}H{line}", overlay.column).unwrap();
            }
        }
        self.output.push_str("\x1b[?2026l");
        let mut output = io::stdout().lock();
        output.write_all(self.output.as_bytes())?;
        output.flush()
    }
}

fn choose_map(
    console: &mut Console,
    colors: bool,
    capabilities: &Capabilities,
) -> Result<Option<Map>> {
    let options: Vec<_> = capabilities
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
mod tests;

#[cfg(windows)]
fn set_console_title() -> Option<Vec<u16>> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        System::Console::{GetConsoleTitleW, SetConsoleTitleW},
    };

    let mut previous = vec![0; 32768];
    let title: Vec<_> = screen::APP_NAME.encode_utf16().chain([0]).collect();
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
        if let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
            && quit(key)
        {
            return Ok(true);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Quake,
    Eew,
    Tsunami,
    Settings,
    About,
    Demo,
}

impl Tab {
    const MENU: [Self; 5] = [
        Self::Quake,
        Self::Eew,
        Self::Tsunami,
        Self::Settings,
        Self::About,
    ];
    const COUNT: usize = Self::Demo as usize + 1;

    fn menu_tab(self) -> Self {
        if self == Self::Demo {
            Self::Settings
        } else {
            self
        }
    }

    fn cycle(self, backwards: bool) -> Self {
        let step = if backwards { Self::MENU.len() - 1 } else { 1 };
        Self::MENU[(self.menu_tab().index() + step) % Self::MENU.len()]
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
            Self::About => "アプリ情報",
            Self::Settings => "設定",
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
    Details(Result<Arc<[Report]>>),
    Live(LiveUpdate),
}

struct DetailsState {
    reports: Arc<[Report]>,
    next: Instant,
    requested: bool,
    pending: bool,
    error: Option<String>,
}

impl DetailsState {
    fn new(now: Instant) -> Self {
        Self {
            reports: Arc::default(),
            next: now,
            requested: true,
            pending: false,
            error: None,
        }
    }
}

pub struct App {
    client: Arc<dyn Source>,
    send: Sender<Reply>,
    incoming: Receiver<Reply>,
    tab: Tab,
    page: Page,
    help_scroll: Option<usize>,
    selection: [usize; Tab::COUNT],
    scroll: usize,
    reading_start: usize,
    pub view: View,
    eew_layer: EewLayer,
    layer: Layer,
    station_grouping: StationGrouping,
    history_rows: usize,
    reports: Arc<[Report]>,
    list: Arc<[Report]>,
    details: DetailsState,
    queries: Option<Sender<LiveQuery>>,
    provider: Provider,
    connected: Option<bool>,
    last_feed_update: Option<ServerClock>,
    warning_seconds: Option<u64>,
    connection_error: Option<String>,
    history_error: Option<String>,
    eew: eew::State,
    tsunami: TsunamiTab,
    demo: DemoState,
}

impl App {
    fn new(client: impl Source + 'static, delay: Duration, national: bool) -> Self {
        let (send, incoming) = mpsc::channel();
        let now = Instant::now();
        Self {
            client: Arc::new(client),
            send,
            incoming,
            tab: Tab::Quake,
            page: Page::Map,
            help_scroll: None,
            selection: [0; Tab::COUNT],
            scroll: 0,
            reading_start: 0,
            view: View::new(national),
            eew_layer: EewLayer::default(),
            layer: Layer::default(),
            station_grouping: StationGrouping::default(),
            history_rows: DEFAULT_HISTORY_ROWS,
            reports: Arc::default(),
            list: Arc::default(),
            details: DetailsState::new(now),
            queries: None,
            provider: Provider::Wolfx,
            connected: None,
            last_feed_update: None,
            warning_seconds: None,
            connection_error: None,
            history_error: None,
            eew: eew::State::default(),
            tsunami: TsunamiTab::default(),
            demo: DemoState::new(delay),
        }
    }

    pub fn start(client: impl Source + 'static, delay: Duration, national: bool) -> Self {
        let mut app = Self::new(client, delay, national);
        let (send_queries, queries) = mpsc::channel();
        app.queries = Some(send_queries);
        let send = app.send.clone();
        let client = Arc::clone(&app.client);
        thread::spawn(move || {
            client.stream(queries, &mut |update| {
                send.send(Reply::Live(update)).is_ok()
            });
        });
        app
    }

    pub fn newest(&self) -> Option<&Bulletin> {
        self.eew
            .active()
            .or_else(|| self.reports.first().map(|report| &report.bulletin))
    }

    pub fn live_status(&self) -> &str {
        match self.connected {
            Some(false) => self.connection_error.as_deref().unwrap_or("再接続中…"),
            None => "Wolfx 接続中…",
            _ if self.eew.active().is_none() && self.eew.notice().is_some() => {
                self.eew.notice().unwrap()
            }
            _ if self.provider == Provider::P2p => self
                .history_error
                .as_deref()
                .unwrap_or("P2P地震情報（予備）"),
            _ => self.details.error.as_deref().unwrap_or("Wolfx"),
        }
    }

    pub fn take_eew_bell(&mut self) -> bool {
        let live = self.eew.take_bell();
        let demo = std::mem::take(&mut self.demo.bell);
        live || demo
    }

    fn len(&self) -> usize {
        match self.tab {
            Tab::Quake => self.reports.len(),
            Tab::Eew => usize::from(self.eew.active().is_some()),
            Tab::Tsunami | Tab::About | Tab::Settings => 0,
            Tab::Demo => self.demo.frames.len(),
        }
    }

    fn change_tab(&mut self, next: Tab) {
        if self.tab != next {
            self.view.reset();
            if self.tab == Tab::Demo || next == Tab::Demo {
                self.demo.next = None;
            }
            if next == Tab::Demo {
                self.selection[next.index()] = 0;
            }
        }
        self.tab = next;
        self.page = Page::Map;
        self.scroll = 0;
    }

    fn map_view_mut(&mut self) -> &mut View {
        if self.tab == Tab::Tsunami {
            &mut self.tsunami.view
        } else {
            &mut self.view
        }
    }

    fn select(&mut self, key: KeyCode, width: usize, now: Instant) -> bool {
        let help_key = matches!(key, KeyCode::Char('h' | 'H' | 'ｈ' | 'Ｈ' | '?' | '？'));
        if let Some(scroll) = &mut self.help_scroll {
            match key {
                _ if help_key || key == KeyCode::Esc => self.help_scroll = None,
                KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Down => *scroll = scroll.saturating_add(1),
                KeyCode::Home => *scroll = 0,
                KeyCode::End => *scroll = usize::MAX,
                _ => return false,
            }
            return true;
        }
        if help_key {
            self.help_scroll = Some(0);
            return true;
        }
        // Windows IMEs may commit a full-width plus. Numpad plus arrives as '+'.
        let key = match key {
            KeyCode::Char('＋') => KeyCode::Char('+'),
            KeyCode::Char('－' | '−') => KeyCode::Char('-'),
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        match key {
            KeyCode::Char(digit @ '1'..='5') => {
                self.change_tab(Tab::MENU[(digit as u8 - b'1') as usize]);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.change_tab(self.tab.cycle(key == KeyCode::BackTab));
            }
            KeyCode::Esc if self.tab == Tab::Demo && self.page == Page::Map => {
                self.change_tab(Tab::Settings);
            }
            KeyCode::Esc if self.tab == Tab::About => self.change_tab(Tab::Quake),
            KeyCode::Up if self.tab == Tab::About => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down if self.tab == Tab::About => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Home if self.tab == Tab::About => self.scroll = 0,
            KeyCode::End if self.tab == Tab::About => self.scroll = usize::MAX,
            _ if matches!(self.tab, Tab::About | Tab::Settings) => return false,
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
                if self.page == Page::Map
                    && matches!(self.tab, Tab::Eew | Tab::Demo)
                    && self
                        .current()
                        .is_some_and(|(report, _)| report.eew && !report.eew_cancelled) =>
            {
                self.eew_layer = self.eew_layer.other();
            }
            KeyCode::Char('v')
                if self.page != Page::List
                    && self
                        .current()
                        .is_some_and(|(report, _)| report.has_observations()) =>
            {
                self.layer = self.layer.other();
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
                if self.page == Page::Text
                    || self.tab == Tab::Tsunami
                    || (self.tab == Tab::Eew && self.len() == 0)
                {
                    self.scroll = self.scroll.saturating_add_signed(direction);
                } else {
                    let limit = self.len().saturating_sub(1);
                    let selected = &mut self.selection[self.tab.index()];
                    let next = selected.saturating_add_signed(direction).min(limit);
                    if *selected != next {
                        if self.tab == Tab::Demo {
                            self.demo.announce(Some(*selected), next);
                        }
                        *selected = next;
                        self.reading_start = 0;
                        if !self.view.national {
                            self.view.reset();
                        }
                    }
                    self.demo.next = None;
                }
            }
            KeyCode::Char(' ') if self.tab == Tab::Demo => {
                self.page = Page::Map;
                if self.demo.next.take().is_none() {
                    let selected = &mut self.selection[self.tab.index()];
                    if *selected + 1 == self.demo.frames.len() {
                        *selected = 0;
                    }
                    self.demo.announce(None, *selected);
                    self.demo.next = Some(now + self.demo.delay);
                }
            }
            KeyCode::Char('r') if self.tab == Tab::Demo => {
                self.demo = DemoState::new(self.demo.delay);
                self.selection[self.tab.index()] = 0;
                self.page = Page::Map;
                self.scroll = 0;
                self.view.reset();
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char('f') if self.tab == Tab::Demo => {
                let index = self.selection[self.tab.index()];
                let bulletin = &mut self.demo.frames[index];
                if !bulletin.eew || self.page != Page::Map {
                    return false;
                }
                self.demo.next = None;
                if key == KeyCode::Char('f') {
                    let previous = if bulletin.warning { "警報" } else { "予報" };
                    let next = if bulletin.warning { "予報" } else { "警報" };
                    bulletin.label = bulletin.label.replacen(previous, next, 1);
                    bulletin.warning = !bulletin.warning;
                    self.demo.bell |= bulletin.warning && !bulletin.eew_cancelled;
                    std::mem::swap(
                        &mut bulletin.warning_areas,
                        &mut self.demo.hidden_areas[index],
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
                    data::set_demo_intensity(bulletin, next as u8);
                }
            }
            KeyCode::Char('z') => {
                let view = self.map_view_mut();
                *view = View::new(!view.national);
            }
            KeyCode::Char('+' | '=' | ';') => return self.map_view_mut().zoom_by(1),
            KeyCode::Char('-') => return self.map_view_mut().zoom_by(-1),
            KeyCode::Char('w') => return self.map_view_mut().pan(0.0, 0.18),
            KeyCode::Char('a') => return self.map_view_mut().pan(-0.18, 0.0),
            KeyCode::Char('s') => return self.map_view_mut().pan(0.0, -0.18),
            KeyCode::Char('d') => return self.map_view_mut().pan(0.18, 0.0),
            KeyCode::Char('0') => self.map_view_mut().reset(),
            KeyCode::Char('r') if matches!(self.tab, Tab::Quake | Tab::Tsunami) => {
                let query = match self.tab {
                    Tab::Tsunami => LiveQuery::Tsunami,
                    _ => {
                        self.details.next = now;
                        self.details.requested = true;
                        LiveQuery::Quakes
                    }
                };
                if let Some(sender) = &self.queries {
                    let _ = sender.send(query);
                }
            }
            _ => return false,
        }
        true
    }

    fn set_reports(&mut self, reports: Arc<[Report]>) {
        let previous = self.reports.get(self.selection[0]);
        self.selection[0] =
            keep_selection(&self.reports, &reports, self.selection[0], |item| &item.id);
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
    }

    pub fn update(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let mut reports_changed = false;
        let mut eew_changed = false;
        while let Ok(message) = self.incoming.try_recv() {
            match message {
                Reply::Details(result) => {
                    self.details.pending = false;
                    let result = result.map_err(|error| error.to_string());
                    let error = result.as_ref().err();
                    changed |= self.details.error.as_ref() != error;
                    self.details.error = error.cloned();
                    if let Ok(details) = result
                        && !Arc::ptr_eq(&self.details.reports, &details)
                    {
                        self.details.reports = details;
                        reports_changed = true;
                    }
                }
                Reply::Live(LiveUpdate::Connection(provider, error)) => {
                    let connected = error.is_none();
                    if self.provider != provider || self.connected != Some(connected) {
                        self.tsunami.received = false;
                    }
                    if self.provider != provider {
                        self.provider = provider;
                        self.last_feed_update = None;
                        self.list = Arc::default();
                        self.eew.switch_source();
                        self.history_error = None;
                        self.tsunami.error = None;
                        self.details.requested = provider == Provider::Wolfx;
                        self.details.next = now;
                        if provider == Provider::P2p {
                            self.tsunami.state.reference = None;
                        }
                        changed = true;
                    } else if !connected {
                        self.eew.disconnect();
                    }
                    changed |= self.connected != Some(connected) || self.connection_error != error;
                    self.connected = Some(connected);
                    self.connection_error = error;
                }
                Reply::Live(LiveUpdate::Clock(provider, clock)) => {
                    if provider == self.provider {
                        let minute = |sample: ServerClock| sample.utc.timestamp().div_euclid(60);
                        changed |= self.last_feed_update.map(minute) != Some(minute(clock));
                        self.last_feed_update = Some(clock);
                    }
                    eew_changed |= self.eew.sync(clock, now);
                    if provider == Provider::Wolfx {
                        self.tsunami.state.sync_wolfx(clock, now);
                    }
                }
                Reply::Live(LiveUpdate::Eew(report)) => {
                    eew_changed |= self.eew.receive(report, now)
                }
                Reply::Live(LiveUpdate::EewInvalid(reason)) => changed |= self.eew.invalid(&reason),
                Reply::Live(LiveUpdate::Quakes(list)) => {
                    self.list = list;
                    self.history_error = None;
                    reports_changed = true;
                    if self.provider == Provider::Wolfx {
                        self.details.requested = true;
                        self.details.next = self.details.next.min(now + Duration::from_secs(1));
                    }
                }
                Reply::Live(LiveUpdate::HistoryInvalid(error)) => {
                    self.history_error = Some(format!("P2P 履歴取得失敗 · {error}"));
                    changed |= self.tab == Tab::Quake;
                }
                Reply::Live(LiveUpdate::Tsunami(report)) => {
                    self.tsunami.received |= report.provider == self.provider;
                    self.tsunami.state.receive(report, now);
                    self.tsunami.error = None;
                    if self.tab == Tab::Tsunami {
                        self.scroll = 0;
                    }
                    changed = true;
                }
                Reply::Live(LiveUpdate::TsunamiInvalid(error)) => {
                    self.tsunami.error = Some(error);
                    changed = true;
                }
            }
        }
        if reports_changed {
            let reports = if self.provider == Provider::Wolfx {
                data::merge_reports(&self.list, &self.details.reports)
            } else {
                Arc::clone(&self.list)
            };
            if !reports.is_empty() {
                self.set_reports(reports);
            }
            changed |= self.tab == Tab::Quake;
        }
        let expired = self.eew.expire(now);
        if self.tab == Tab::Eew && (eew_changed || expired) {
            self.scroll = 0;
            if expired {
                self.page = Page::Map;
            }
        }
        changed |= eew_changed || expired;
        changed |= self.tsunami.state.update(now);
        let warning_seconds = self.eew.warning_seconds(now);
        changed |= self.tab == Tab::Eew && self.warning_seconds != warning_seconds;
        self.warning_seconds = warning_seconds;

        if self.provider == Provider::Wolfx
            && !self.details.pending
            && (self.details.requested || self.tab == Tab::Quake)
            && now >= self.details.next
        {
            self.details.pending = true;
            self.details.requested = false;
            self.details.next = now + Duration::from_secs(60);
            let client = self.client.clone();
            let send = self.send.clone();
            thread::spawn(move || {
                let _ = send.send(Reply::Details(client.details()));
            });
        }
        if self.tab == Tab::Demo && self.demo.next.is_some_and(|at| now >= at) {
            let i = &mut self.selection[Tab::Demo.index()];
            let next = (*i + 1).min(self.demo.frames.len() - 1);
            self.demo.announce(Some(*i), next);
            *i = next;
            self.demo.next = (*i + 1 < self.demo.frames.len()).then(|| now + self.demo.delay);
            changed = true;
        }
        changed
    }

    fn current(&self) -> Option<(Cow<'_, Bulletin>, String)> {
        let i = self.tab.index();
        let selected = self.selection[i];
        let (bulletin, banner) = match self.tab {
            Tab::Quake => self.reports.get(selected).map(|report| {
                (
                    Cow::Borrowed(&report.bulletin),
                    format!(
                        "地震 {}/{} · {}",
                        selected + 1,
                        self.reports.len(),
                        report.bulletin.provider.label()
                    ),
                )
            }),
            Tab::Eew => self.eew.active().map(|bulletin| {
                (
                    Cow::Borrowed(bulletin),
                    format!("{} · {}", bulletin.label, bulletin.provider.label()),
                )
            }),
            Tab::Tsunami | Tab::About | Tab::Settings => return None,
            Tab::Demo => self.demo.frames.get(selected).map(|frame| {
                let progress = format!("{}/{}", selected + 1, self.demo.frames.len());
                let banner = if self.page == Page::Text || !frame.eew {
                    format!("【デモ・架空】 {progress}  {}", frame.label)
                } else {
                    progress
                };
                (Cow::Borrowed(frame), banner)
            }),
        }
        .unwrap_or_else(|| {
            let label = match (self.tab, self.connected) {
                (_, Some(false)) => "地震情報に再接続中…",
                _ if self.details.error.is_some() => "地震詳細を取得できませんでした",
                _ => "地震情報を取得中…",
            };
            (
                Cow::Owned(Bulletin {
                    label: label.into(),
                    ..Bulletin::default()
                }),
                self.tab.label().into(),
            )
        });
        let banner = if self.tab == Tab::Eew && self.connected == Some(false) {
            banner + "  再接続中…"
        } else {
            banner
        };
        Some((bulletin, banner))
    }

    fn help_topic(&self, width: usize) -> HelpTopic {
        if self.page == Page::Text {
            let observations = self
                .current()
                .is_some_and(|(bulletin, _)| bulletin.has_observations());
            return HelpTopic::Details {
                tab: self.tab.label(),
                observations,
            };
        }
        match self.tab {
            Tab::Quake if self.page == Page::List => HelpTopic::QuakeList,
            Tab::Quake => HelpTopic::QuakeMap {
                narrow: width < MIN_SPLIT_WIDTH,
            },
            Tab::Eew => HelpTopic::Eew {
                active: self.eew.active().is_some(),
            },
            Tab::Tsunami => HelpTopic::Tsunami,
            Tab::Demo => HelpTopic::Demo {
                eew: self.demo.frames[self.selection[Tab::Demo.index()]].eew,
            },
            Tab::Settings => HelpTopic::Settings,
            Tab::About => HelpTopic::About,
        }
    }

    fn paint(
        &mut self,
        renderer: &mut Renderer,
        console: &mut Console,
        mut display: Display,
        settings: &SettingsMenu,
        active_map: Map,
        capabilities: &Capabilities,
    ) -> Result<()> {
        if self.page == Page::List && display.width() >= MIN_SPLIT_WIDTH {
            self.page = Page::Map;
        }
        let overlay = self.help_scroll.map(|mut scroll| {
            let layer = screen::help(self.help_topic(display.width()), display, &mut scroll);
            self.help_scroll = Some(scroll);
            display.colors = false;
            // Image protocols cannot reliably place terminal text over an image.
            if matches!(display.map, Map::Image(_)) {
                display.map = Map::Braille;
            }
            layer
        });
        let mut draw = |frame: &Frame| console.paint_layers(frame, overlay.as_ref());
        let items = Tab::MENU.map(|tab| {
            let bulletin = match tab {
                Tab::Eew => self.eew.active(),
                _ => None,
            };
            let (label, notice) = match (tab, bulletin.filter(|bulletin| bulletin.eew)) {
                (Tab::Eew, Some(bulletin)) if bulletin.eew_cancelled => ("EEW 取消", None),
                (Tab::Eew, Some(bulletin)) if bulletin.warning => {
                    ("EEW 警報", Some(MenuNotice::Warning))
                }
                (Tab::Eew, Some(bulletin)) if bulletin.has_eew_warning() => {
                    ("EEW 警報あり", Some(MenuNotice::Warning))
                }
                (Tab::Eew, Some(_)) => ("EEW 予報", Some(MenuNotice::Forecast)),
                _ => (tab.label(), None),
            };
            screen::MenuItem {
                key: (b'1' + tab.index() as u8) as char,
                label,
                active: tab == self.tab.menu_tab(),
                notice,
            }
        });
        let mut menu = screen::menu_lines(&items, display);
        let feed_status = self.feed_status();
        let connection = feed_status.connection;
        screen::append_feed_status(&mut menu, feed_status, display);
        if self.tab == Tab::Settings {
            return Ok(draw(&screen::settings(
                &menu,
                &settings.rows(capabilities),
                settings.selected,
                active_map.label(),
                settings.notice,
                display,
            ))?);
        }
        if self.tab == Tab::About {
            return Ok(draw(&screen::about(
                &menu,
                display,
                &mut self.scroll,
                AboutInfo {
                    rendering: active_map,
                    capabilities,
                    connection,
                },
            ))?);
        }
        if self.tab == Tab::Tsunami {
            let (status, alert, lines, notes) = self.tsunami_display();
            let mut scroll = self.scroll;
            let frame = renderer.tsunami(
                &menu,
                screen::NoticePanel {
                    title: "津波情報",
                    status,
                    alert,
                    lines: &lines,
                    notes: &notes,
                },
                self.tsunami_map_areas(),
                display,
                self.tsunami.view,
                &mut scroll,
            );
            self.scroll = scroll;
            if overlay.is_none()
                && let Some(view) = frame.map_view
            {
                self.tsunami.view = view;
            }
            return Ok(draw(frame)?);
        }
        if self.tab == Tab::Eew && self.eew.active().is_none() {
            let (status, notes) = self.idle_eew_display();
            let frame = screen::panel(
                &menu,
                screen::NoticePanel {
                    title: "緊急地震速報",
                    status,
                    alert: false,
                    lines: &[],
                    notes: &notes,
                },
                display,
                &mut self.scroll,
            );
            return Ok(draw(&frame)?);
        }
        let Some((bulletin, banner)) = self.current() else {
            return Ok(());
        };
        let cached_history = self
            .reports
            .first()
            .filter(|report| report.bulletin.provider != self.provider)
            .map(|report| format!("前回の履歴 · {}", report.bulletin.provider.label()));
        let status = if let Some(cached) = cached_history.as_deref() {
            cached
        } else if self.provider == Provider::P2p {
            self.history_error
                .as_deref()
                .unwrap_or(if self.connected == Some(true) {
                    "P2P地震情報（予備）"
                } else {
                    "P2P地震情報（予備）に接続中…"
                })
        } else if self.connected == Some(false) {
            "Wolfx 再接続中…"
        } else if let Some(error) = &self.details.error {
            error.as_str()
        } else {
            ""
        };
        let history = (self.tab == Tab::Quake).then_some(History {
            reports: &self.reports,
            selected: self.selection[0],
            status,
            row_limit: self.history_rows,
        });
        if self.page == Page::List {
            return Ok(draw(&screen::compact(
                history.expect("earthquake list"),
                display,
                &menu,
            ))?);
        }
        if self.page == Page::Text {
            let mut scroll = self.scroll;
            let frame = screen::detail(
                &bulletin,
                &banner,
                self.layer,
                self.station_grouping,
                display,
                &mut scroll,
                &menu,
            );
            self.scroll = scroll;
            return Ok(draw(&frame)?);
        }
        let actions: &[&str] = match self.tab {
            Tab::Quake if display.width() < MIN_SPLIT_WIDTH => {
                &["B 履歴", "↑↓ 選択", "V 地域別/観測点"]
            }
            Tab::Quake => &["↑↓ 選択", "←→ 震度一覧", "V 地域別/観測点"],
            Tab::Eew if !bulletin.eew_cancelled => &["V 地図表示"],
            Tab::Demo => {
                let playback = if self.demo.next.is_some() {
                    "Space 停止"
                } else {
                    "Space 再生"
                };
                &["↑↓ 選択", playback, "Esc 設定へ"]
            }
            _ => &[],
        };
        let controls = [screen::footer(actions, display)];
        let series = if self.tab == Tab::Demo {
            &self.demo.frames
        } else {
            &[] as &[Bulletin]
        };
        let live_eew = self.tab == Tab::Eew;
        let eew_source = live_eew.then(|| format!("気象庁発表 · {}", bulletin.provider.label()));
        let mut view = self.view;
        view.eew_layer = if bulletin.eew {
            self.eew_layer
        } else {
            EewLayer::Warning
        };
        let scene = Scene {
            bulletin: &bulletin,
            banner: if live_eew {
                if self.connected == Some(false) {
                    "再接続中…"
                } else {
                    self.eew.notice().unwrap_or_default()
                }
            } else {
                &banner
            },
            eew_source: eew_source.as_deref(),
            warning_seconds: if live_eew { self.warning_seconds } else { None },
            demo: self.tab == Tab::Demo,
            series,
            view,
            layer: self.layer,
            station_grouping: self.station_grouping,
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
        if overlay.is_none()
            && let Some(view) = frame.map_view
        {
            self.view = view;
        }
        Ok(draw(frame)?)
    }

    fn quake_notices(&self) -> QuakeNotices {
        let Some(report) = self.reports.get(self.selection[Tab::Quake.index()]) else {
            return QuakeNotices::default();
        };
        let received = self.eew.matches(&report.id);
        let eew = received.then_some(true).or(report.bulletin.eew_notice);
        let tsunami = self
            .tsunami
            .state
            .verified(self.provider)
            .and_then(|tsunami| tsunami.notice_for(&report.id))
            .or(report.bulletin.tsunami_notice);
        QuakeNotices { eew, tsunami }
    }

    fn feed_status(&self) -> FeedStatus {
        let backup = self.provider == Provider::P2p;
        let connection = match (self.connected, backup) {
            (Some(true), false) => "● Wolfx",
            (Some(true), true) => "● P2P（予備）",
            (Some(false), false) => "○ 再接続中",
            (Some(false), true) => "○ 再接続中",
            (None, _) => "○ 接続中",
        };
        let quiet = self.connected == Some(true) && self.eew.can_confirm_idle();
        let eew = match self.eew.active() {
            Some(report) if report.eew_cancelled => ("EEW 取消", None),
            Some(report) if report.warning => ("EEW 警報", Some(MenuNotice::Warning)),
            Some(report) if report.has_eew_warning() => ("EEW 警報あり", Some(MenuNotice::Warning)),
            Some(_) => ("EEW 予報", Some(MenuNotice::Forecast)),
            None if quiet && backup => ("EEW 警報なし", None),
            None if quiet => ("EEW なし", None),
            None => ("EEW 未確認", None),
        };
        let tsunami = match self.confirmed_tsunami().map(|report| report.status) {
            Some(TsunamiStatus::Alert) => ("津波 警報等", Some(MenuNotice::Warning)),
            Some(TsunamiStatus::Forecast) => ("津波 予報", Some(MenuNotice::Forecast)),
            Some(TsunamiStatus::Clear) => ("津波 なし", None),
            _ => ("津波 未確認", None),
        };
        FeedStatus {
            connection,
            online: self.connected == Some(true),
            updated: self.last_feed_update.map(|clock| {
                clock
                    .utc
                    .with_timezone(&chrono::FixedOffset::east_opt(9 * 3600).unwrap())
            }),
            eew,
            tsunami,
        }
    }

    fn idle_eew_display(&self) -> (&'static str, Vec<String>) {
        let status = match self.connected {
            None => "EEWに接続中…",
            Some(false) => "EEWの発表状況を確認できません",
            Some(true) if !self.eew.can_confirm_idle() => "EEWの発表状況を確認中…",
            Some(true) if self.provider == Provider::P2p => "現在受信中のEEW警報はありません",
            Some(true) => "現在表示中のEEWはありません",
        };
        let mut notes = Vec::new();
        if let Some((issued, expired)) = self.eew.latest() {
            let reason = if expired {
                " · 120秒を過ぎたため非表示"
            } else {
                ""
            };
            notes.push(format!("直近 {} JST{reason}", issued.format("%H:%M:%S")));
        }
        if !self.eew.can_confirm_idle()
            && let Some(notice) = self.eew.notice()
        {
            notes.push(notice.into());
        }
        if let Some(error) = &self.connection_error {
            notes.push(format!("再接続中 · {error}"));
        }
        (status, notes)
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
pub struct ResizeRedraw {
    dimensions: Option<(usize, usize)>,
    sixel_cell: Option<(usize, usize)>,
    sixel_at: Option<Instant>,
}

impl ResizeRedraw {
    pub fn update(&mut self, view: &mut Display, now: Instant) -> bool {
        let dimensions = (view.columns, view.rows);
        let previous = self.dimensions.replace(dimensions);
        let changed = previous != Some(dimensions);
        let Map::Image(Image::Sixel(cell)) = view.map else {
            self.sixel_cell = None;
            self.sixel_at = None;
            return changed;
        };
        let cell_changed = self.sixel_cell.replace(cell).is_some_and(|old| old != cell);
        if (changed && previous.is_some()) || cell_changed {
            self.sixel_at = Some(now + SIXEL_RESIZE_QUIET);
        }
        if self.sixel_at.is_some_and(|deadline| now >= deadline) {
            self.sixel_at = None;
            return true;
        }
        if self.sixel_at.is_some() {
            view.map = Map::Braille;
        }
        changed || cell_changed
    }
}

pub fn run(
    client: Client,
    renderer: &mut Renderer,
    colors: bool,
    map_override: Option<MapMode>,
    delay: Duration,
    national: bool,
) -> Result<()> {
    let mut console = Console::enter()?;
    let capabilities = graphics::detect();
    let mut settings = SettingsMenu::load();
    let mode = map_override.unwrap_or(settings.saved.map);
    let mut map = if mode == MapMode::Auto {
        let Some(map) = choose_map(&mut console, colors, &capabilities)? else {
            return Ok(());
        };
        map
    } else if map_override.is_some() {
        mode.resolve(true, Capabilities::default)
    } else {
        mode.supported(&capabilities).unwrap_or(Map::Braille)
    };
    let mut app = App::start(client, delay, national || settings.saved.national);
    app.layer = settings.saved.layer;
    app.station_grouping = settings.saved.station_grouping;
    app.history_rows = settings.saved.history_rows;
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
                    let in_settings = app.help_scroll.is_none() && app.tab == Tab::Settings;
                    changed |= if in_settings && settings.trial_key(code) {
                        console.ring()?;
                        settings.notice = Some(("端末ベルを送信しました", false));
                        true
                    } else if in_settings
                        && settings.handle(code, &mut app, &mut map, &capabilities)
                    {
                        true
                    } else {
                        app.select(code, display(colors, map).width(), Instant::now())
                    };
                }
                _ => {}
            }
        }
        if app.take_eew_bell() && settings.saved.eew_bell {
            console.ring()?;
        }
        let mut view = display(colors, map);
        changed |= resize.update(&mut view, Instant::now());
        if changed {
            app.paint(renderer, &mut console, view, &settings, map, &capabilities)?;
        }
    }
}
