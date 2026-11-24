use std::{borrow::Cow, path::PathBuf};

use super::{App, Capabilities, KeyCode, Map, MapMode, Settings, Tab, View};
use crate::{
    map::Layer,
    screen::{SettingChoice, SettingRow},
    settings::{AUTO_HISTORY_ROWS, StationGrouping},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Map,
    MapRange,
    Intensity,
    Grouping,
    History,
    Bell,
    Demo,
}

impl Field {
    const ALL: [Self; 7] = [
        Self::Map,
        Self::MapRange,
        Self::Intensity,
        Self::Grouping,
        Self::History,
        Self::Bell,
        Self::Demo,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Map => "地図の描画方式",
            Self::MapRange => "起動時の地図範囲",
            Self::Intensity => "起動時の震度表示",
            Self::Grouping => "観測点のまとめ方",
            Self::History => "地震履歴の行数",
            Self::Bell => "EEWの端末ベル",
            Self::Demo => "EEWデモ",
        }
    }

    fn description(self, current: Settings, capabilities: &Capabilities) -> &'static str {
        match self {
            Self::Map if current.map == MapMode::Auto => {
                "次回起動時に描画方式を選びます。×は端末非対応です。"
            }
            Self::Map if current.map.supported(capabilities).is_none() => {
                "保存済みの描画方式はこの端末では非対応です。"
            }
            Self::Map => "すぐに切り替え、次回起動時もこの方式を使います。×は端末非対応です。",
            Self::MapRange => "現在の地図と次回起動時の範囲を切り替えます。",
            Self::Intensity if current.layer == Layer::Stations => {
                "観測点ごとの実測震度を表示します。"
            }
            Self::Intensity => "地域ごとの最大観測震度を表示します。",
            Self::Grouping => "観測点の一覧を震度別または都道府県別にまとめます。",
            Self::History if current.history_rows == AUTO_HISTORY_ROWS => {
                "端末の高さに合わせて表示行数を調整します。"
            }
            Self::History => "履歴を指定した行数まで表示します。",
            Self::Bell => "Enterで試し鳴らし。ベルの音は端末の設定に従います。",
            Self::Demo => "架空の緊急地震速報を再生できます。",
        }
    }

    fn choices(self, current: Settings, capabilities: &Capabilities) -> Vec<Choice> {
        match self {
            Self::Map => MapMode::ALL
                .into_iter()
                .map(|mode| {
                    let label = match mode {
                        MapMode::Auto => "自動",
                        MapMode::Braille => "Braille 非推奨",
                        MapMode::Text => "文字 非推奨",
                        _ => mode.label(),
                    };
                    Choice {
                        label: label.into(),
                        value: Settings {
                            map: mode,
                            ..current
                        },
                        supported: mode == MapMode::Auto || mode.supported(capabilities).is_some(),
                    }
                })
                .collect(),
            Self::MapRange => [false, true]
                .into_iter()
                .map(|national| {
                    let label = if national { "全国" } else { "震源周辺" };
                    Choice::new(
                        label,
                        Settings {
                            national,
                            ..current
                        },
                    )
                })
                .collect(),
            Self::Intensity => [Layer::Regions, Layer::Stations]
                .into_iter()
                .map(|layer| Choice::new(layer.label(), Settings { layer, ..current }))
                .collect(),
            Self::Grouping => [StationGrouping::Intensity, StationGrouping::Prefecture]
                .into_iter()
                .map(|station_grouping| {
                    Choice::new(
                        station_grouping.label(),
                        Settings {
                            station_grouping,
                            ..current
                        },
                    )
                })
                .collect(),
            Self::History => {
                let mut presets = vec![AUTO_HISTORY_ROWS, 5, 10, 20, 30];
                if !presets.contains(&current.history_rows) {
                    presets.push(current.history_rows);
                    presets.sort_unstable();
                }
                presets
                    .into_iter()
                    .map(|history_rows| {
                        let label: Cow<'static, str> = if history_rows == AUTO_HISTORY_ROWS {
                            "自動".into()
                        } else {
                            history_rows.to_string().into()
                        };
                        Choice::new(
                            label,
                            Settings {
                                history_rows,
                                ..current
                            },
                        )
                    })
                    .collect()
            }
            Self::Demo => Vec::new(),
            Self::Bell => [true, false]
                .into_iter()
                .map(|eew_bell| {
                    let label = if eew_bell { "オン" } else { "オフ" };
                    Choice::new(
                        label,
                        Settings {
                            eew_bell,
                            ..current
                        },
                    )
                })
                .collect(),
        }
    }
}

struct Choice {
    label: Cow<'static, str>,
    value: Settings,
    supported: bool,
}

impl Choice {
    fn new(label: impl Into<Cow<'static, str>>, value: Settings) -> Self {
        Self {
            label: label.into(),
            value,
            supported: true,
        }
    }
}

pub(super) struct SettingsMenu {
    path: Option<PathBuf>,
    pub(super) saved: Settings,
    pub(super) selected: usize,
    pub(super) notice: Option<(&'static str, bool)>,
}

impl SettingsMenu {
    pub(super) fn load() -> Self {
        let path = Settings::path();
        let (saved, notice) = match path.as_deref().map(Settings::load) {
            Some(Ok(settings)) => (settings, None),
            Some(Err(_)) => (
                Settings::default(),
                Some(("設定を読み込めないため、初期値で起動しました", true)),
            ),
            None => (
                Settings::default(),
                Some(("設定の保存先が見つかりません", true)),
            ),
        };
        Self {
            path,
            saved,
            selected: 0,
            notice,
        }
    }

    pub(super) fn rows(&self, capabilities: &Capabilities) -> [SettingRow; Field::ALL.len()] {
        Field::ALL.map(|field| SettingRow {
            label: field.label(),
            description: field.description(self.saved, capabilities),
            hint: match field {
                Field::Bell => Some("Enter 試し鳴らし"),
                Field::Demo => Some("Enter 開く"),
                _ => None,
            },
            choices: field
                .choices(self.saved, capabilities)
                .into_iter()
                .map(|choice| SettingChoice {
                    label: choice.label,
                    selected: choice.value == self.saved,
                    supported: choice.supported,
                })
                .collect(),
        })
    }

    pub(super) fn trial_key(&self, key: KeyCode) -> bool {
        matches!(key, KeyCode::Char('t' | 'T'))
            || (key == KeyCode::Enter && Field::ALL[self.selected] == Field::Bell)
    }

    pub(super) fn handle(
        &mut self,
        key: KeyCode,
        app: &mut App,
        map: &mut Map,
        capabilities: &Capabilities,
    ) -> bool {
        let last = Field::ALL.len() - 1;
        match key {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Enter if Field::ALL[self.selected] == Field::Demo => {
                app.change_tab(Tab::Demo);
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(next) = self.next_value(key, capabilities) {
                    self.save(next, app, map, capabilities);
                }
            }
            KeyCode::Esc => app.change_tab(Tab::Quake),
            _ => return false,
        }
        true
    }

    fn next_value(&self, key: KeyCode, capabilities: &Capabilities) -> Option<Settings> {
        let choices = Field::ALL[self.selected].choices(self.saved, capabilities);
        let current = choices
            .iter()
            .position(|choice| choice.value == self.saved)?;
        let before = &choices[..current];
        let after = &choices[current + 1..];
        let next = match key {
            KeyCode::Left => before.iter().rfind(|choice| choice.supported),
            KeyCode::Right => after.iter().find(|choice| choice.supported),
            _ => after.iter().chain(before).find(|choice| choice.supported),
        };
        next.map(|choice| choice.value)
    }

    fn save(&mut self, next: Settings, app: &mut App, map: &mut Map, capabilities: &Capabilities) {
        self.notice = Some(match self.path.as_deref().map(|path| next.save(path)) {
            Some(Ok(())) => {
                if next.map != self.saved.map
                    && let Some(mode) = next.map.supported(capabilities)
                {
                    *map = mode;
                }
                if next.national != self.saved.national {
                    app.view = View::new(next.national);
                }
                if next.layer != self.saved.layer {
                    app.layer = next.layer;
                    app.reading_start = 0;
                }
                if next.station_grouping != self.saved.station_grouping {
                    app.station_grouping = next.station_grouping;
                    app.reading_start = 0;
                }
                app.history_rows = next.history_rows;
                self.saved = next;
                ("保存しました", false)
            }
            Some(Err(_)) => ("設定を保存できませんでした", true),
            None => ("設定の保存先が見つかりません", true),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Client;
    use std::{fs, time::Duration};

    #[test]
    fn choices_fit_wide_and_narrow_settings_screens() {
        let menu = SettingsMenu {
            path: None,
            saved: Settings::default(),
            selected: 0,
            notice: None,
        };
        let capabilities = Capabilities {
            sixel: Some((10, 20)),
            ..Default::default()
        };
        for columns in [46, 84] {
            let display = crate::screen::Display {
                columns,
                rows: 22,
                colors: false,
                map: Map::Braille,
            };
            let frame = crate::screen::settings(
                &[],
                &menu.rows(&capabilities),
                0,
                "Sixel",
                Some(("保存しました", false)),
                display,
            );
            assert!(frame.text.contains("[自動]"));
            assert!(frame.text.contains("Kitty ×"));
            assert!(frame.text.contains("Braille 非推奨"));
            assert!(frame.text.contains("文字 非推奨"));
            assert!(frame.text.contains("使用中  Sixel"));
            assert!(frame.text.contains("保存しました"));
            assert!(frame.text.contains("EEWデモ"));
            assert!(frame.text.contains("Enter 開く"));
            assert!(!frame.text.contains("保存して適用"));
            assert!(frame.text.lines().count() <= display.rows);
        }
    }

    #[test]
    fn commits_changes_only_after_saving_and_keeps_legacy_choices() {
        let path =
            std::env::temp_dir().join(format!("tuiquake-autosave-{}.json", std::process::id()));
        let saved = Settings {
            history_rows: 7,
            ..Settings::default()
        };
        let mut menu = SettingsMenu {
            path: None,
            saved,
            selected: Field::History as usize,
            notice: None,
        };
        let mut app = App::new(Client::new(), Duration::from_secs(1), false);
        app.history_rows = 7;
        let mut map = Map::Braille;
        let capabilities = Capabilities::default();

        assert_eq!(
            menu.next_value(KeyCode::Left, &capabilities)
                .unwrap()
                .history_rows,
            5
        );
        assert!(menu.handle(KeyCode::Right, &mut app, &mut map, &capabilities));
        assert_eq!(app.history_rows, 7);
        assert_eq!(menu.saved.history_rows, 7);
        assert_eq!(menu.notice.map(|(_, error)| error), Some(true));

        menu.path = Some(path.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert!(menu.handle(KeyCode::Right, &mut app, &mut map, &capabilities));
            assert_eq!(app.history_rows, 10);
            assert_eq!(Settings::load(&path).unwrap().history_rows, 10);
            assert!(menu.handle(KeyCode::Left, &mut app, &mut map, &capabilities));
            assert!(menu.handle(KeyCode::Left, &mut app, &mut map, &capabilities));
            assert_eq!(app.history_rows, AUTO_HISTORY_ROWS);
            assert_eq!(
                Settings::load(&path).unwrap().history_rows,
                AUTO_HISTORY_ROWS
            );
            assert_eq!(menu.notice, Some(("保存しました", false)));
        }));
        let _ = fs::remove_file(&path);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    #[test]
    fn demo_opens_from_settings_and_returns_to_the_same_item() {
        let mut menu = SettingsMenu {
            path: None,
            saved: Settings::default(),
            selected: 0,
            notice: None,
        };
        let mut app = App::new(Client::new(), Duration::from_secs(1), false);
        let now = std::time::Instant::now();
        let mut map = Map::Braille;
        let capabilities = Capabilities::default();

        assert!(!app.select(KeyCode::Char('6'), 80, now));
        assert!(app.select(KeyCode::Char('5'), 80, now));
        assert_eq!(app.tab, Tab::About);
        assert!(app.select(KeyCode::Tab, 80, now));
        assert_eq!(app.tab, Tab::Quake);
        assert!(app.select(KeyCode::BackTab, 80, now));
        assert_eq!(app.tab, Tab::About);
        assert!(app.select(KeyCode::Char('4'), 80, now));
        assert_eq!(app.tab, Tab::Settings);

        assert!(menu.handle(KeyCode::End, &mut app, &mut map, &capabilities));
        assert!(!menu.trial_key(KeyCode::Enter));
        assert!(menu.handle(KeyCode::Enter, &mut app, &mut map, &capabilities));
        assert_eq!(app.tab, Tab::Demo);
        assert_eq!(app.tab.menu_tab(), Tab::Settings);
        assert!(app.select(KeyCode::Char(' '), 80, now));
        assert!(app.demo.next.is_some());
        assert!(app.select(KeyCode::Esc, 80, now));
        assert_eq!(app.tab, Tab::Settings);
        assert!(app.demo.next.is_none());
        assert_eq!(Field::ALL[menu.selected].label(), "EEWデモ");
        assert!(menu.notice.is_none());
    }
}
