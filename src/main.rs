mod data;
mod graphics;
mod map;
mod screen;
mod settings;
mod ui;

use std::{
    io::{self, IsTerminal, Write},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use data::{Bulletin, Client, Result};
use graphics::{Map, MapMode};
use screen::{APP_NAME, Renderer, Scene};

const HELP: &str = "TuiQuake  日本の地震・緊急地震速報をターミナルで表示

使い方: tuiquake [tui|latest|eew|replay|watch] [オプション]

  tui       地図と地震一覧を操作する（既定）
  latest    最新地震の観測震度
  eew       Wolfx が配信した最新の緊急地震速報
  replay    架空の EEW と観測震度のデモ
  watch     地震情報と緊急地震速報を受信し次第更新

  --history                   replay で Wolfx の最新 EEW を1報表示
  --delay 秒                   replay の画面切り替え間隔（既定 2.25 秒）
  --japan                      全国を表示
  --color <auto|always|never>   テキストの色
  --map <auto|sixel|kitty|braille|text>
                              地図の描画方式（TUI の auto は起動時に選択）
  -h, --help                   ヘルプを表示
  -V, --version                バージョンを表示
";

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Command {
    #[default]
    Tui,
    Latest,
    Eew,
    Replay,
    Watch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Colors {
    Auto,
    Always,
    Never,
}

struct Options {
    command: Command,
    history: bool,
    delay: Duration,
    national: bool,
    colors: Colors,
    map: Option<MapMode>,
}

impl Options {
    fn parse() -> Result<Option<Self>> {
        let mut options = Self {
            command: Command::Tui,
            history: false,
            delay: Duration::from_millis(2250),
            national: false,
            colors: Colors::Auto,
            map: None,
        };
        let mut args = std::env::args().skip(1);
        let mut command_given = false;
        while let Some(arg) = args.next() {
            let (name, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
            let mut argument = || {
                inline
                    .map(str::to_owned)
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{name} に値を指定してください"))
            };
            match name {
                "-h" | "--help" => {
                    print!("{HELP}");
                    return Ok(None);
                }
                "-V" | "--version" => {
                    println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "--history" if inline.is_none() => options.history = true,
                "--japan" if inline.is_none() => options.national = true,
                "--delay" => {
                    let seconds = argument()?.parse::<f64>()?;
                    options.delay = Duration::try_from_secs_f64(seconds)
                        .map_err(|_| "--delay には 0 以上の有限な値を指定してください")?;
                    if Instant::now().checked_add(options.delay).is_none() {
                        return Err("--delay の値が大きすぎます".into());
                    }
                }
                "--color" => {
                    options.colors = match argument()?.as_str() {
                        "auto" => Colors::Auto,
                        "always" => Colors::Always,
                        "never" => Colors::Never,
                        _ => return Err("--color は auto、always、never のいずれかです".into()),
                    }
                }
                "--map" => {
                    options.map = Some(match argument()?.as_str() {
                        "auto" => MapMode::Auto,
                        "sixel" => MapMode::Sixel,
                        "kitty" => MapMode::Kitty,
                        "braille" => MapMode::Braille,
                        "text" => MapMode::Text,
                        _ => {
                            return Err(
                                "--map は auto、sixel、kitty、braille、text のいずれかです".into(),
                            );
                        }
                    });
                }
                "tui" | "latest" | "eew" | "replay" | "watch"
                    if !command_given && inline.is_none() =>
                {
                    options.command = match name {
                        "latest" => Command::Latest,
                        "eew" => Command::Eew,
                        "replay" => Command::Replay,
                        "watch" => Command::Watch,
                        _ => Command::Tui,
                    };
                    command_given = true;
                }
                _ => return Err(format!("不明な引数 {arg}").into()),
            }
        }
        if options.history && options.command != Command::Replay {
            return Err("--history は replay 用です".into());
        }
        Ok(Some(options))
    }
}

fn print_frame(frame: &screen::Frame) -> Result<()> {
    let mut out = io::stdout().lock();
    ui::write_inline(&mut out, frame)?;
    Ok(out.flush()?)
}

fn replay(
    options: &Options,
    renderer: &mut Renderer,
    colors: bool,
    map: Map,
    interactive: bool,
) -> Result<()> {
    let frames = data::demo();
    let banner = "デモ · 架空の地震";
    let mut console = if interactive {
        Some(ui::Console::enter()?)
    } else {
        None
    };
    for (index, bulletin) in frames.iter().enumerate() {
        if index > 0 {
            if console.is_some() {
                if ui::wait_or_quit(options.delay)? {
                    return Ok(());
                }
            } else {
                thread::sleep(options.delay);
            }
        }
        let label = format!(
            "{banner}  {}/{}   {}",
            index + 1,
            frames.len(),
            bulletin.label
        );
        let mut view = Scene::single(bulletin, options.national, &label);
        view.series = &frames;
        // The TUI demo keeps its final frame on screen. Direct replay leaves
        // the final frame on the main screen after the timed transitions.
        let frame = renderer.render(view, ui::display(colors, map));
        if index + 1 == frames.len() {
            drop(console.take());
            print_frame(frame)?;
        } else if let Some(console) = console.as_mut() {
            console.paint(frame)?;
        } else {
            print_frame(frame)?;
        }
    }
    Ok(())
}

fn watch(
    options: &Options,
    client: &Client,
    renderer: &mut Renderer,
    colors: bool,
    map: Map,
    interactive: bool,
) -> Result<()> {
    let mut console = if interactive {
        Some(ui::Console::enter()?)
    } else {
        None
    };
    let mut app = ui::App::start(client.clone(), options.delay, options.national);
    let waiting = Bulletin {
        label: "取得待ち".into(),
        ..Bulletin::default()
    };
    let mut previous = screen::Frame::default();
    let mut resize = ui::ResizeRedraw::default();
    let interval = Duration::from_millis(200);
    loop {
        let now = Instant::now();
        let updated = app.update(now);
        let mut display = ui::display(colors, map);
        let resized = resize.update(&mut display, now);
        if updated || resized {
            let bulletin = app.newest().unwrap_or(&waiting);
            let label = format!("{}  ·  {}", bulletin.label, app.live_status());
            let frame = renderer.render(Scene::single(bulletin, options.national, &label), display);
            if let Some(console) = console.as_mut() {
                console.paint(frame)?;
            } else if frame.text != previous.text || frame.graphic != previous.graphic {
                print_frame(frame)?;
                previous.text.clone_from(&frame.text);
                previous.graphic.clone_from(&frame.graphic);
            }
        }
        if console.is_some() {
            if ui::wait_or_quit(interval)? {
                return Ok(());
            }
        } else {
            thread::sleep(interval);
        }
    }
}

fn run() -> Result<()> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let ansi = ui::ansi();
    let interactive = ansi && io::stdin().is_terminal();
    let colors = match options.colors {
        Colors::Always => true,
        Colors::Never => false,
        Colors::Auto => ansi && std::env::var_os("NO_COLOR").is_none(),
    };
    let client = Client::new();
    let mut renderer = Renderer::new()?;
    let resolve = || {
        options
            .map
            .unwrap_or_default()
            .resolve(ansi, graphics::detect)
    };
    let command = if options.history {
        Command::Eew
    } else {
        options.command
    };
    match command {
        Command::Tui => {
            ui::run(
                client,
                &mut renderer,
                colors,
                options.map,
                options.delay,
                options.national,
            )?;
        }
        Command::Latest => {
            let reports = client.reports()?;
            let report = reports
                .first()
                .ok_or("Wolfx に表示できる地震情報がありません")?;
            print_frame(renderer.render(
                Scene::single(&report.bulletin, options.national, "地震情報 · Wolfx"),
                ui::display(colors, resolve()),
            ))?;
        }
        Command::Eew => {
            let bulletin = client.eew()?.ok_or("Wolfx に表示できる EEW がありません")?;
            let label = format!("{} · Wolfx", bulletin.label);
            print_frame(renderer.render(
                Scene::single(&bulletin, options.national, &label),
                ui::display(colors, resolve()),
            ))?;
        }
        Command::Replay => replay(&options, &mut renderer, colors, resolve(), interactive)?,
        Command::Watch => watch(
            &options,
            &client,
            &mut renderer,
            colors,
            resolve(),
            interactive,
        )?,
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APP_NAME}: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Capabilities, Image};

    #[test]
    fn direct_output_uses_the_detected_image_or_text() {
        assert_eq!(
            MapMode::Auto.resolve(true, || Capabilities {
                kitty: true,
                ..Default::default()
            }),
            Map::Image(Image::Kitty)
        );
        assert_eq!(
            MapMode::Auto.resolve(true, Capabilities::default),
            Map::Text
        );
        assert_eq!(MapMode::Kitty.resolve(false, || unreachable!()), Map::Text);
        assert_eq!(
            MapMode::Braille.resolve(false, || unreachable!()),
            Map::Braille
        );
    }
}
