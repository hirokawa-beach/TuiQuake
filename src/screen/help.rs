use unicode_width::UnicodeWidthStr;

use super::{Border, Display, INK, RESET, fit, map, pad, wrap};

pub enum HelpTopic {
    QuakeMap {
        narrow: bool,
    },
    QuakeList,
    Details {
        tab: &'static str,
        observations: bool,
    },
    Eew {
        active: bool,
    },
    Tsunami,
    Demo {
        eew: bool,
    },
    Settings,
    About,
}

type Binding = (&'static str, &'static str);

const MAP_BINDINGS: [Binding; 4] = [
    ("WASD", "地図を移動"),
    ("+ / -", "拡大・縮小"),
    ("Z", "全国・震源周辺を切り替え"),
    ("0", "位置と倍率を戻す"),
];

impl HelpTopic {
    fn bindings(self) -> (&'static str, Vec<Binding>) {
        match self {
            Self::QuakeMap { narrow } => {
                let reading = if narrow {
                    ("B", "地震履歴を開く")
                } else {
                    ("← / →", "各地の震度をスクロール")
                };
                let mut bindings = vec![
                    ("↑ / ↓", "地震を選ぶ"),
                    ("Enter", "選んだ地震の震源周辺へ"),
                    reading,
                    ("V", "広域・観測点を切り替え"),
                    ("I", "文字詳細を開く"),
                    ("R", "地震情報を更新"),
                ];
                bindings.extend(MAP_BINDINGS);
                ("地震の操作", bindings)
            }
            Self::QuakeList => (
                "地震履歴の操作",
                vec![
                    ("↑ / ↓", "地震を選ぶ"),
                    ("Enter / B", "地図を開く"),
                    ("I", "文字詳細を開く"),
                    ("R", "地震情報を更新"),
                ],
            ),
            Self::Details { tab, observations } => {
                let mut bindings =
                    vec![("↑ / ↓", "文字詳細をスクロール"), ("I / Esc", "地図に戻る")];
                if observations {
                    bindings.push(("V", "広域・観測点を切り替え"));
                }
                (tab, bindings)
            }
            Self::Eew { active } => {
                let mut bindings = vec![("I", "受信した速報の文字詳細を開く")];
                if active {
                    bindings.extend(MAP_BINDINGS);
                }
                ("EEWの操作", bindings)
            }
            Self::Tsunami => (
                "津波の操作",
                vec![("↑ / ↓", "津波情報をスクロール"), ("R", "津波情報を更新")],
            ),
            Self::Demo { eew } => {
                let mut bindings = vec![("↑ / ↓", "デモの報を選ぶ"), ("Space", "再生・停止")];
                if eew {
                    bindings.extend([
                        ("← / →", "推定最大震度を変更"),
                        ("F", "予報・警報を切り替え"),
                    ]);
                }
                bindings.extend([("I", "文字詳細を開く"), ("R", "デモを初期化")]);
                bindings.extend(MAP_BINDINGS);
                ("デモの操作", bindings)
            }
            Self::Settings => (
                "設定の操作",
                vec![
                    ("↑ / ↓", "項目を選ぶ"),
                    ("← / →", "項目の値を変更"),
                    ("Home / End", "先頭の項目・保存欄へ移動"),
                    ("Enter / S", "保存欄でEnter、またはSで保存"),
                    ("Esc", "未保存の変更を破棄して戻る"),
                ],
            ),
            Self::About => (
                "Aboutの操作",
                vec![
                    ("↑ / ↓", "スクロール"),
                    ("Home / End", "先頭・末尾へ移動"),
                    ("Esc", "地震タブへ戻る"),
                ],
            ),
        }
    }
}

pub struct HelpOverlay {
    pub row: usize,
    pub column: usize,
    pub lines: Vec<String>,
    pub colors: bool,
}

pub fn help(topic: HelpTopic, display: Display, scroll: &mut usize) -> HelpOverlay {
    let width = display.width().saturating_sub(4).clamp(8, 54);
    let inner = width - 4;
    let max_height = display.rows.saturating_sub(4).max(4).min(display.rows);
    let (name, bindings) = topic.bindings();
    let mut content = Vec::new();
    let key_width = bindings
        .iter()
        .map(|(key, _)| key.width())
        .max()
        .unwrap_or(0)
        + 2;
    let indent = " ".repeat(key_width);
    for (key, action) in bindings {
        if inner < key_width + 10 {
            content.extend(wrap(&format!("{key}  {action}"), inner));
        } else {
            let key = pad(key, key_width);
            for (index, line) in wrap(action, inner - key_width).into_iter().enumerate() {
                let prefix = if index == 0 { &key } else { &indent };
                content.push(format!("{prefix}{line}"));
            }
        }
    }
    let capacity = max_height.saturating_sub(3);
    *scroll = (*scroll).min(content.len().saturating_sub(capacity));
    let available = (content.len() - *scroll).min(capacity);
    let overflow = content.len() > capacity;
    let mut lines = if max_height >= 3 {
        let mut visible: Vec<_> = content.into_iter().skip(*scroll).take(available).collect();
        let footer = if overflow && inner >= "↑↓ スクロール  H / Esc 閉じる".width() {
            "↑↓ スクロール  H / Esc 閉じる"
        } else if overflow && inner >= "↑↓  H 閉じる".width() {
            "↑↓  H 閉じる"
        } else if inner >= "H / Esc 閉じる".width() {
            "H / Esc 閉じる"
        } else {
            "H 閉じる"
        };
        visible.push(fit(footer, inner));
        Border::new(width, false).text(visible, name, available + 3)
    } else {
        vec![pad("H / Esc", width)]
    };
    if display.colors {
        let mut style = String::new();
        map::rgb(&mut style, (20, 32, 40), true);
        map::rgb(&mut style, INK, false);
        for line in &mut lines {
            *line = format!("{style}{line}{RESET}");
        }
    }
    HelpOverlay {
        row: display.rows.saturating_sub(lines.len()) / 2 + 1,
        column: (display.width() - width) / 2 + 1,
        lines,
        colors: display.colors,
    }
}
