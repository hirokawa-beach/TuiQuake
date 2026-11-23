use std::{
    fmt::Write,
    io::{self, IsTerminal, Write as _},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::map::{PALETTE, Raster};

type ImageCell = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Image {
    Sixel(ImageCell),
    Kitty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Map {
    Image(Image),
    Braille,
    Text,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MapMode {
    #[default]
    Auto,
    Sixel,
    Kitty,
    Braille,
    Text,
}

impl MapMode {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Sixel,
        Self::Kitty,
        Self::Braille,
        Self::Text,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "起動時に選ぶ",
            Self::Sixel => "Sixel",
            Self::Kitty => "Kitty",
            Self::Braille => Map::Braille.label(),
            Self::Text => Map::Text.label(),
        }
    }

    pub fn supported(self, capabilities: &Capabilities) -> Option<Map> {
        match self {
            Self::Auto => None,
            Self::Sixel => capabilities
                .sixel
                .map(|cell| Map::Image(Image::Sixel(cell))),
            Self::Kitty => capabilities.kitty.then_some(Map::Image(Image::Kitty)),
            Self::Braille => Some(Map::Braille),
            Self::Text => Some(Map::Text),
        }
    }

    pub fn resolve(self, ansi: bool, detect: impl FnOnce() -> Capabilities) -> Map {
        match self {
            Self::Auto if ansi => detect().images().next().map_or(Map::Text, Map::Image),
            Self::Sixel if ansi => Map::Image(Image::Sixel(WINDOWS_TERMINAL_CELL)),
            Self::Kitty if ansi => Map::Image(Image::Kitty),
            Self::Braille => Map::Braille,
            _ => Map::Text,
        }
    }
}

impl Map {
    pub fn image(self) -> Option<Image> {
        match self {
            Self::Image(image) => Some(image),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Image(Image::Sixel(_)) => "Sixel",
            Self::Image(Image::Kitty) => "Kitty",
            Self::Braille => "Unicode Braille  非推奨/表示調節中",
            Self::Text => "文字地図",
        }
    }
}

impl Image {
    pub fn cell(self) -> ImageCell {
        match self {
            Self::Sixel(cell) => cell,
            Self::Kitty => (8, 16),
        }
    }
}

// Windows Terminal reports virtual Sixel cells of 10 × 20 pixels.
pub const WINDOWS_TERMINAL_CELL: ImageCell = (10, 20);
pub const KITTY_COMMAND: &str = "\x1b_G";

#[derive(Default)]
pub struct Capabilities {
    pub sixel: Option<ImageCell>,
    pub kitty: bool,
}

impl Capabilities {
    pub fn images(&self) -> impl Iterator<Item = Image> {
        self.sixel
            .map(Image::Sixel)
            .into_iter()
            .chain(self.kitty.then_some(Image::Kitty))
    }
}

const QUERY: &str = "\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[16t\x1b[c";
const QUERY_TIMEOUT: Duration = Duration::from_millis(160);

pub fn detect() -> Capabilities {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Capabilities::default();
    }
    let replies = platform::query();
    #[cfg(windows)]
    let windows_terminal = {
        let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        (program.is_empty() || program.eq_ignore_ascii_case("Windows_Terminal"))
            && !term.starts_with("screen")
            && !term.starts_with("tmux")
            && platform::known_sixel_host()
    };
    #[cfg(not(windows))]
    let windows_terminal = false;

    let sixel = if windows_terminal {
        Some(WINDOWS_TERMINAL_CELL)
    } else if replies.sixel {
        replies.cell.or_else(terminal_cell)
    } else {
        None
    };
    Capabilities {
        sixel,
        kitty: replies.kitty,
    }
}

fn terminal_cell() -> Option<ImageCell> {
    let size = crossterm::terminal::window_size().ok()?;
    valid_cell(
        usize::from(size.width).checked_div(usize::from(size.columns))?,
        usize::from(size.height).checked_div(usize::from(size.rows))?,
    )
}

fn valid_cell(width: usize, height: usize) -> Option<ImageCell> {
    ((1..=64).contains(&width) && (1..=128).contains(&height)).then_some((width, height))
}

fn send_query() -> io::Result<()> {
    let mut output = io::stdout().lock();
    output.write_all(QUERY.as_bytes())?;
    output.flush()
}

#[derive(Default)]
struct Replies {
    sixel: bool,
    kitty: bool,
    cell: Option<ImageCell>,
}

enum Reply {
    DeviceAttributes(bool),
    Cell(Option<ImageCell>),
    Kitty(bool),
}

impl Replies {
    // Device attributes are requested last and end the probe.
    fn record(&mut self, reply: Reply) -> bool {
        match reply {
            Reply::Kitty(value) => self.kitty = value,
            Reply::Cell(value) => self.cell = value,
            Reply::DeviceAttributes(value) => {
                self.sixel = value;
                return true;
            }
        }
        false
    }
}

// Ok(None) is a prefix of a possible response. An error is unrelated input.
fn parse_reply(bytes: &[u8]) -> Result<Option<Reply>, ()> {
    if bytes.len() > 96 || !bytes.is_ascii() {
        return Err(());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    if let Some(attributes) = text
        .strip_prefix("\x1b[?")
        .and_then(|s| s.strip_suffix('c'))
    {
        if attributes
            .split(';')
            .all(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
        {
            return Ok(Some(Reply::DeviceAttributes(
                attributes.split(';').skip(1).any(|v| v == "4"),
            )));
        }
        return Err(());
    }
    if let Some(size) = text
        .strip_prefix("\x1b[6;")
        .and_then(|s| s.strip_suffix('t'))
    {
        let (height, width) = size.split_once(';').ok_or(())?;
        let (width, height) = (
            width.parse().map_err(|_| ())?,
            height.parse().map_err(|_| ())?,
        );
        return Ok(Some(Reply::Cell(valid_cell(width, height))));
    }
    const KITTY_REPLY: &str = "\x1b_Gi=31;";
    if let Some(message) = text.strip_prefix(KITTY_REPLY) {
        return Ok(message
            .strip_suffix("\x1b\\")
            .map(|message| Reply::Kitty(message == "OK")));
    }
    for prefix in ["\x1b[?", "\x1b[6;", KITTY_REPLY] {
        if prefix.starts_with(text) {
            return Ok(None);
        }
    }
    if ["\x1b[?", "\x1b[6;"].into_iter().any(|prefix| {
        text.strip_prefix(prefix)
            .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit() || b == b';'))
    }) {
        return Ok(None);
    }
    Err(())
}

#[cfg(unix)]
mod platform {
    use std::time::Instant;

    use crossterm::terminal;

    use super::{QUERY_TIMEOUT, Replies, parse_reply, send_query};

    struct RawMode(bool);

    impl Drop for RawMode {
        fn drop(&mut self) {
            if self.0 {
                let _ = terminal::disable_raw_mode();
            }
        }
    }

    pub(super) fn query() -> Replies {
        let mut replies = Replies::default();
        let restore = !terminal::is_raw_mode_enabled().unwrap_or(false);
        if restore && terminal::enable_raw_mode().is_err() {
            return replies;
        }
        let _raw = RawMode(restore);
        let mut input = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: input is one initialized pollfd. Skip the query if the
        // caller already has pending input.
        if unsafe { libc::poll(&mut input, 1, 0) } != 0 || send_query().is_err() {
            return replies;
        }
        let deadline = Instant::now() + QUERY_TIMEOUT;
        let mut bytes = Vec::new();
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            // SAFETY: input is initialized; the duration is bounded.
            if unsafe { libc::poll(&mut input, 1, remaining.as_millis() as i32 + 1) } <= 0
                || input.revents & libc::POLLIN == 0
            {
                break;
            }
            let mut byte = 0u8;
            // SAFETY: byte is writable. Read one byte to avoid buffering input
            // belonging to the chooser after the final reply.
            if unsafe { libc::read(input.fd, (&mut byte as *mut u8).cast(), 1) } != 1 {
                break;
            }
            bytes.push(byte);
            match parse_reply(&bytes) {
                Ok(Some(reply)) => {
                    if replies.record(reply) {
                        break;
                    }
                    bytes.clear();
                }
                Ok(None) => {}
                Err(()) => break,
            }
        }
        replies
    }
}

pub fn kitty(raster: &Raster, rgb: &mut Vec<u8>, out: &mut String) {
    rgb.clear();
    rgb.reserve(raster.pixels.len() * 3);
    for &pixel in &raster.pixels {
        let (red, green, blue) = PALETTE[pixel as usize];
        rgb.extend([red, green, blue]);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(rgb, 1);
    let (width, height) = Image::Kitty.cell();
    out.clear();
    // 3072 bytes of RGB payload become a 4096-byte base64 chunk.
    let mut chunks = compressed.chunks(3072).peekable();
    write!(
        out,
        "{KITTY_COMMAND}a=T,f=24,o=z,q=2,C=1,z=1,s={},v={},c={},r={}",
        raster.width,
        raster.height,
        raster.width / width,
        raster.height / height
    )
    .unwrap();
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        write!(out, ",m={more};").unwrap();
        STANDARD.encode_string(chunk, out);
        out.push_str("\x1b\\");
        if more != 0 {
            write!(out, "{KITTY_COMMAND}q=2").unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_kitty_and_sixel_independently() {
        let mut replies = Replies::default();
        for response in [
            b"\x1b_Gi=31;OK\x1b\\".as_slice(),
            b"\x1b[6;18;9t",
            b"\x1b[?61;4;14c",
        ] {
            for end in 1..response.len() {
                assert!(matches!(parse_reply(&response[..end]), Ok(None)));
            }
            let last = replies.record(parse_reply(response).ok().flatten().unwrap());
            assert_eq!(last, response.ends_with(b"c"));
        }
        let images: Vec<_> = Capabilities {
            sixel: replies.cell.filter(|_| replies.sixel),
            kitty: replies.kitty,
        }
        .images()
        .collect();
        assert_eq!(images, [Image::Sixel((9, 18)), Image::Kitty]);
        assert!(matches!(
            parse_reply(b"\x1b[?61;14;24c"),
            Ok(Some(Reply::DeviceAttributes(false)))
        ));
        assert!(matches!(
            parse_reply(b"\x1b_Gi=31;ENOSPC\x1b\\"),
            Ok(Some(Reply::Kitty(false)))
        ));
        assert!(parse_reply(b"\x1b[A").is_err());
    }

    #[test]
    fn kitty_payload_decodes_to_the_original_pixels_across_chunks() {
        let mut raster = Raster::default();
        raster.resize(640, 320);
        let mut seed = 42u32;
        for pixel in &mut raster.pixels {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *pixel = ((seed >> 16) as usize % PALETTE.len()) as u8;
        }
        let mut expected = Vec::new();
        let mut output = String::new();
        kitty(&raster, &mut expected, &mut output);
        let mut compressed = Vec::new();
        let mut chunks = output.split_terminator("\x1b\\").peekable();
        let mut count = 0;
        while let Some(chunk) = chunks.next() {
            let (header, payload) = chunk.split_once(';').unwrap();
            let more = chunks.peek().is_some();
            assert!(header.ends_with(if more { ",m=1" } else { ",m=0" }));
            assert!(payload.len() <= 4096);
            if count == 0 {
                assert!(header.contains("f=24,o=z,q=2,C=1,z=1,s=640,v=320,c=80,r=20"));
            }
            compressed.extend(STANDARD.decode(payload).unwrap());
            count += 1;
        }
        assert!(count > 1);
        assert_eq!(
            miniz_oxide::inflate::decompress_to_vec_zlib(&compressed).unwrap(),
            expected
        );
        assert_eq!(expected.len(), 640 * 320 * 3);
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        collections::HashMap,
        thread,
        time::{Duration, Instant},
    };

    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Console::{
                GetConsoleWindow, GetStdHandle, INPUT_RECORD, KEY_EVENT, PeekConsoleInputW,
                ReadConsoleInputW, STD_INPUT_HANDLE,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
        UI::WindowsAndMessaging::GetWindowThreadProcessId,
    };

    use super::{QUERY_TIMEOUT, Replies, parse_reply, send_query};

    pub fn known_sixel_host() -> bool {
        terminal_host() || terminal_ancestor(std::process::id())
    }

    pub(super) fn query() -> Replies {
        let mut replies = Replies::default();
        // SAFETY: The standard handle is borrowed from Windows.
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE || send_query().is_err() {
            return replies;
        }

        let deadline = Instant::now() + QUERY_TIMEOUT;
        let mut buffer = [INPUT_RECORD::default(); 128];
        while Instant::now() < deadline {
            let mut found = 0;
            // SAFETY: buffer contains writable INPUT_RECORDs. Peeking leaves
            // any input other than the expected replies in the queue.
            if unsafe {
                PeekConsoleInputW(handle, buffer.as_mut_ptr(), buffer.len() as u32, &mut found)
            } == 0
            {
                break;
            }
            let mut bytes = Vec::new();
            let mut complete = None;
            for (index, record) in buffer[..found as usize].iter().enumerate() {
                if record.EventType != KEY_EVENT as u16 {
                    continue;
                }
                // SAFETY: KEY_EVENT selects the KeyEvent union member.
                let key = unsafe { record.Event.KeyEvent };
                if key.bKeyDown == 0 {
                    continue;
                }
                // SAFETY: The Unicode console API sets UnicodeChar.
                let Ok(byte) = u8::try_from(unsafe { key.uChar.UnicodeChar }) else {
                    return replies;
                };
                bytes.push(byte);
                match parse_reply(&bytes) {
                    Ok(Some(reply)) => {
                        complete = Some((index as u32 + 1, reply));
                        break;
                    }
                    Ok(None) => {}
                    Err(()) => return replies,
                }
            }
            if let Some((end, reply)) = complete {
                let mut removed = 0;
                // SAFETY: end covers the exact prefix matched above. This
                // process has not started its keyboard event reader.
                if unsafe { ReadConsoleInputW(handle, buffer.as_mut_ptr(), end, &mut removed) } == 0
                    || removed != end
                {
                    break;
                }
                if replies.record(reply) {
                    break;
                }
            } else {
                thread::sleep(Duration::from_millis(4));
            }
        }
        replies
    }

    fn terminal_host() -> bool {
        let mut pid = 0;
        // SAFETY: The HWND comes from this console; pid is valid writable storage.
        unsafe { GetWindowThreadProcessId(GetConsoleWindow(), &mut pid) };
        pid != 0 && terminal_process(pid)
    }

    fn terminal_ancestor(pid: u32) -> bool {
        // SAFETY: This snapshots process IDs and executable names only.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut parents = HashMap::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: entry is writable with dwSize set, and snapshot is owned here.
        let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) };
        while has_entry != 0 {
            let end = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
            let candidate = exe.eq_ignore_ascii_case("WindowsTerminal.exe")
                || exe.eq_ignore_ascii_case("OpenConsole.exe");
            parents.insert(entry.th32ProcessID, (entry.th32ParentProcessID, candidate));
            has_entry = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) };
        ancestor_matches(pid, &parents, terminal_process)
    }

    fn ancestor_matches(
        mut pid: u32,
        processes: &HashMap<u32, (u32, bool)>,
        terminal: impl Fn(u32) -> bool,
    ) -> bool {
        for _ in 0..8 {
            let Some(&(parent, _)) = processes.get(&pid) else {
                break;
            };
            if parent == 0 || parent == pid {
                break;
            }
            if processes
                .get(&parent)
                .is_some_and(|&(_, candidate)| candidate)
                && terminal(parent)
            {
                return true;
            }
            pid = parent;
        }
        false
    }

    fn terminal_process(pid: u32) -> bool {
        // SAFETY: Query-only access to the console host or an ancestor process.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut path = [0u16; 512];
        let mut size = path.len() as u32;
        // SAFETY: path has `size` writable UTF-16 units; the handle is owned here.
        let success =
            unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(process) };
        success != 0 && packaged_terminal(&String::from_utf16_lossy(&path[..size as usize]))
    }

    fn packaged_terminal(path: &str) -> bool {
        let path = path.to_ascii_lowercase();
        let mut segments = path.rsplit(['\\', '/']);
        if !matches!(
            segments.next(),
            Some("openconsole.exe" | "windowsterminal.exe")
        ) {
            return false;
        }
        segments
            .filter_map(|part| part.split_once('_'))
            .any(|(name, version)| {
                if !matches!(
                    name,
                    "microsoft.windowsterminal"
                        | "microsoft.windowsterminalpreview"
                        | "microsoft.windowsterminalcanary"
                ) {
                    return false;
                }
                let mut version = version
                    .split('.')
                    .filter_map(|part| part.parse::<u16>().ok());
                version
                    .next()
                    .zip(version.next())
                    .is_some_and(|version| version >= (1, 22))
            })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recognizes_the_packaged_host_of_a_delegated_console() {
            let app = r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.25.3131.0_x64__8wekyb3d8bbwe\OpenConsole.exe";
            assert!(packaged_terminal(app));
            assert!(!packaged_terminal(
                &app.replace("1.25.3131.0", "1.21.3131.0")
            ));
            assert!(!packaged_terminal(r"C:\Windows\System32\conhost.exe"));
        }

        #[test]
        fn finds_windows_terminal_behind_a_powershell_ancestor() {
            let processes = HashMap::from([(42, (17, false)), (17, (9, false)), (9, (2, true))]);
            assert!(ancestor_matches(42, &processes, |process| process == 9));
            assert!(!ancestor_matches(42, &processes, |_| false));
            assert!(!ancestor_matches(7, &processes, |_| true));
        }
    }
}
