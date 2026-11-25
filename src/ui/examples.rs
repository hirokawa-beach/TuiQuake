use std::{collections::HashMap, sync::mpsc, thread};

use crossterm::event::KeyCode;

use super::map_key;
use crate::{
    data::{
        Client, TsunamiStatus,
        examples::{EXAMPLES, Sample, SampleData},
    },
    map::{Layer, View},
    screen::{self, Display, Frame, NoticePanel, Renderer, Scene},
    settings::StationGrouping,
};

enum Load {
    Pending,
    Ready(Box<Sample>),
    Error(String),
}

pub(super) enum Action {
    None,
    Redraw,
    Replay,
}

pub(super) struct Examples {
    pub(super) open: bool,
    selected: usize,
    preview: bool,
    show_json: bool,
    scroll: usize,
    layer: Layer,
    view: View,
    samples: HashMap<usize, Load>,
    send: mpsc::Sender<(usize, Load)>,
    incoming: mpsc::Receiver<(usize, Load)>,
}

impl Default for Examples {
    fn default() -> Self {
        let (send, incoming) = mpsc::channel();
        Self {
            open: false,
            selected: 0,
            preview: false,
            show_json: false,
            scroll: 0,
            layer: Layer::Stations,
            view: View::new(false),
            samples: HashMap::new(),
            send,
            incoming,
        }
    }
}

impl Examples {
    pub(super) fn open(&mut self) {
        self.open = true;
        self.preview = false;
    }

    fn request(&mut self) {
        let index = self.selected;
        let example = EXAMPLES[index - 1];
        let send = self.send.clone();
        self.samples.insert(index, Load::Pending);
        if let Err(error) = thread::Builder::new().spawn(move || {
            let result = match example.load(&Client::new()) {
                Ok(sample) => Load::Ready(Box::new(sample)),
                Err(error) => Load::Error(error.to_string()),
            };
            let _ = send.send((index, result));
        }) {
            self.samples.insert(index, Load::Error(error.to_string()));
        }
    }

    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((index, result)) = self.incoming.try_recv() {
            self.samples.insert(index, result);
            changed |= self.open && self.preview && self.selected == index;
        }
        changed
    }

    pub(super) fn handle(&mut self, key: KeyCode) -> Action {
        if self.preview {
            match key {
                KeyCode::Esc if self.show_json => {
                    self.show_json = false;
                    self.scroll = 0;
                }
                KeyCode::Esc => self.preview = false,
                KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::Down => self.scroll = self.scroll.saturating_add(1),
                KeyCode::Home => self.scroll = 0,
                KeyCode::End => self.scroll = usize::MAX,
                KeyCode::Char('i')
                    if matches!(self.samples.get(&self.selected), Some(Load::Ready(_))) =>
                {
                    self.show_json = !self.show_json;
                    self.scroll = 0;
                }
                KeyCode::Char('r')
                    if matches!(self.samples.get(&self.selected), Some(Load::Error(_))) =>
                {
                    self.request()
                }
                KeyCode::Char('v') if !self.show_json => match self.samples.get(&self.selected) {
                    Some(Load::Ready(sample)) => match sample.data {
                        SampleData::Eew(_) => {
                            self.view.eew_layer = self.view.eew_layer.other();
                        }
                        SampleData::Quakes(_) => self.layer = self.layer.other(),
                        SampleData::Tsunami { .. } => return Action::None,
                    },
                    _ => return Action::None,
                },
                _ if !self.show_json => {
                    let layer = self.view.eew_layer;
                    if !map_key(&mut self.view, key) {
                        return Action::None;
                    }
                    self.view.eew_layer = layer;
                }
                _ => return Action::None,
            }
            return Action::Redraw;
        }

        match key {
            KeyCode::Esc => self.open = false,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(EXAMPLES.len()),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = EXAMPLES.len(),
            KeyCode::Enter if self.selected == 0 => {
                self.open = false;
                return Action::Replay;
            }
            KeyCode::Enter => {
                self.preview = true;
                self.show_json = false;
                self.scroll = 0;
                self.layer = Layer::Stations;
                self.view = View::new(EXAMPLES[self.selected - 1].category() == "津波");
                if !self.samples.contains_key(&self.selected) {
                    self.request();
                }
            }
            _ => return Action::None,
        }
        Action::Redraw
    }

    pub(super) fn draw(
        &mut self,
        renderer: &mut Renderer,
        display: Display,
        grouping: StationGrouping,
        update_view: bool,
        paint: impl FnOnce(&Frame) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let width = display.width();
        let mut menu = vec![screen::fit("【デモ】 TuiQuake", width)];
        if !self.preview {
            menu.push(screen::fit("↑↓ 選択  Enter 開く  Esc 設定へ", width));
            let titles: Vec<_> = std::iter::once("架空のEEWリプレイ".into())
                .chain(
                    EXAMPLES
                        .iter()
                        .map(|example| format!("{} · {}", example.category(), example.label)),
                )
                .enumerate()
                .map(|(index, title)| {
                    screen::fit(
                        &format!("{} {title}", if index == self.selected { ">" } else { " " }),
                        width.saturating_sub(4),
                    )
                })
                .collect();
            let capacity = display.rows.saturating_sub(menu.len() + 3).max(1);
            let mut first = self
                .selected
                .saturating_sub(capacity / 2)
                .min(titles.len().saturating_sub(capacity));
            let frame = screen::panel(
                &menu,
                NoticePanel {
                    title: "デモを選択",
                    status: "",
                    alert: false,
                    lines: &titles,
                    notes: &[],
                },
                display,
                &mut first,
            );
            return paint(&frame);
        }

        let example = EXAMPLES[self.selected - 1];
        menu[0] = screen::fit("【デモ・過去の資料】 Wolfx", width);
        menu.push(screen::fit(example.label, width));
        let Some(load) = self.samples.get(&self.selected) else {
            return Ok(());
        };
        match load {
            Load::Pending => {
                menu.push(screen::fit("Esc 一覧", width));
                paint(&screen::panel(
                    &menu,
                    NoticePanel {
                        title: "デモ",
                        status: "サンプルを読み込み中…",
                        alert: false,
                        lines: &[],
                        notes: &[],
                    },
                    display,
                    &mut self.scroll,
                ))
            }
            Load::Error(error) => {
                menu.push(screen::fit("R 再試行  Esc 一覧", width));
                paint(&screen::panel(
                    &menu,
                    NoticePanel {
                        title: "デモ",
                        status: "サンプルを読み込めませんでした",
                        alert: false,
                        lines: &[],
                        notes: std::slice::from_ref(error),
                    },
                    display,
                    &mut self.scroll,
                ))
            }
            Load::Ready(sample) => {
                if self.show_json {
                    menu.push(screen::fit("I / Esc 地図へ", width));
                    let lines: Vec<_> = serde_json::to_string_pretty(&sample.json)
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_owned)
                        .collect();
                    return paint(&screen::panel(
                        &menu,
                        NoticePanel {
                            title: "応答JSON",
                            status: "",
                            alert: false,
                            lines: &lines,
                            notes: &[],
                        },
                        display,
                        &mut self.scroll,
                    ));
                }
                menu.push(screen::fit("I 応答JSON  Esc 一覧", width));
                let frame = match &sample.data {
                    SampleData::Eew(report) => {
                        let controls = [screen::footer(
                            &["V 地図表示", "WASD 移動", "+/- 拡縮"],
                            display,
                        )];
                        let mut scene = Scene::single(report, false, "");
                        scene.view = self.view;
                        scene.menu = &menu;
                        scene.eew_source = Some("Wolfx API例");
                        scene.controls = &controls;
                        renderer.render(scene, display)
                    }
                    SampleData::Quakes(reports) => {
                        self.scroll = self.scroll.min(reports.len().saturating_sub(1));
                        let selected = self.scroll;
                        let Some(report) = reports.get(selected) else {
                            return Ok(());
                        };
                        let controls = [screen::footer(&["↑↓ 地震を選択", "V 震度表示"], display)];
                        let mut scene = Scene::single(&report.bulletin, false, "");
                        scene.view = self.view;
                        scene.menu = &menu;
                        scene.controls = &controls;
                        scene.layer = self.layer;
                        scene.station_grouping = grouping;
                        scene.history = Some(screen::History {
                            reports,
                            selected,
                            status: "",
                            row_limit: 10,
                        });
                        renderer.render(scene, display)
                    }
                    SampleData::Tsunami {
                        report,
                        observations,
                    } => {
                        let active = matches!(
                            report.status,
                            TsunamiStatus::Alert | TsunamiStatus::Forecast
                        );
                        let areas = if active { &report.areas[..] } else { &[] };
                        let lines: Vec<_> = areas.iter().map(|area| area.line()).collect();
                        let mut notes = Vec::new();
                        if let Some(issued) = report.issued {
                            notes.push(format!("発表 {} JST", issued.format("%Y/%m/%d %H:%M:%S")));
                        }
                        notes.extend_from_slice(observations);
                        renderer.tsunami(
                            &menu,
                            NoticePanel {
                                title: "津波情報の例",
                                status: example.label,
                                alert: report.alert(),
                                lines: &lines,
                                notes: &notes,
                            },
                            areas,
                            display,
                            self.view,
                            &mut self.scroll,
                        )
                    }
                };
                if update_view && let Some(view) = frame.map_view {
                    self.view = view;
                }
                paint(frame)
            }
        }
    }
}
