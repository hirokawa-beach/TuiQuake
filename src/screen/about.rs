use std::env;

use crate::{
    graphics::Capabilities,
    release::{Browser, Release},
};

use super::*;

const DISCLAIMER: [&str; 2] = [
    "一個人が作った非公式の地震情報閲覧用アプリです。",
    "災害発生時は気象庁・自治体の公式情報も確認してください。",
];

pub struct AboutInfo<'a> {
    pub rendering: Map,
    pub capabilities: &'a Capabilities,
    pub connection: &'a str,
    pub release: Option<&'a Release>,
}

fn terminal(capabilities: &Capabilities) -> String {
    if capabilities.windows_terminal || env::var_os("WT_SESSION").is_some() {
        return "Windows Terminal".into();
    }
    for name in ["TERM_PROGRAM", "TERM"] {
        if let Ok(value) = env::var(name)
            && !value.trim().is_empty()
        {
            return value;
        }
    }
    "不明".into()
}

fn about_rows(rows: &[(&str, &str)], width: usize, colors: bool) -> Vec<String> {
    let label_width = rows
        .iter()
        .map(|(label, _)| label.width())
        .max()
        .unwrap_or_default()
        + 2;
    let mut lines = Vec::new();
    for &(label, value) in rows {
        if width >= label_width + 12 {
            let padding = " ".repeat(label_width);
            let label = color(&pad(label, label_width), MUTED, colors);
            for (index, line) in wrap(value, width - label_width).into_iter().enumerate() {
                let prefix = if index == 0 { &label } else { &padding };
                lines.push(format!("{prefix}{}", color(&line, INK, colors)));
            }
        } else {
            lines.push(color(&fit(label, width), MUTED, colors));
            lines.extend(
                wrap(value, width)
                    .into_iter()
                    .map(|line| color(&line, INK, colors)),
            );
        }
    }
    lines
}

pub fn about(menu: &[String], display: Display, scroll: &mut usize, info: AboutInfo<'_>) -> Frame {
    let width = display.width();
    let inner = width - TEXT_FRAME_MARGIN;
    let capacity = display.rows.saturating_sub(menu.len() + 3);
    let notice: Vec<_> = DISCLAIMER
        .into_iter()
        .flat_map(|sentence| wrap(sentence, inner))
        .collect();
    let logo_width = LOGO.iter().map(|row| row.len()).max().unwrap_or(0);
    let mut pinned = Vec::new();
    if inner >= logo_width && capacity >= LOGO.len() + notice.len() + 6 {
        pinned.extend(logo_lines(display.colors));
    } else if capacity > notice.len() {
        pinned.push(compact_logo(display.colors));
    }
    let spacious = capacity.saturating_sub(pinned.len() + notice.len()) >= 6;
    if !pinned.is_empty() && spacious {
        pinned.push(String::new());
    }
    pinned.extend(
        notice
            .into_iter()
            .take(capacity - pinned.len())
            .map(|line| color(&line, MUTED, display.colors)),
    );
    if spacious {
        pinned.push(String::new());
    }
    let viewport = capacity - pinned.len();
    let mut content = Vec::new();

    let detected = |available| if available { "対応" } else { "未検出" };
    let image = format!(
        "Sixel {}  Kitty {}",
        detected(info.capabilities.sixel.is_some()),
        detected(info.capabilities.kitty)
    );
    let os = format!("{} / {}", env::consts::OS, env::consts::ARCH);
    let repository = env!("CARGO_PKG_REPOSITORY");
    let issues = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");
    let built_at = env!("TUIQUAKE_BUILD_EPOCH")
        .parse()
        .ok()
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map_or_else(
            || "不明".into(),
            |time| time.format("%Y-%m-%d %H:%M UTC").to_string(),
        );
    let availability = info
        .release
        .map(|release| format!("{} が利用できます", release.tag));
    let mut diagnostics = vec![("バージョン", env!("CARGO_PKG_VERSION"))];
    if let Some(availability) = &availability {
        diagnostics.push(("更新", availability));
    }
    let terminal = terminal(info.capabilities);
    diagnostics.extend([
        ("コミット", env!("TUIQUAKE_BUILD_COMMIT")),
        ("ビルド日時", &built_at),
        ("制作者", env!("CARGO_PKG_AUTHORS")),
        ("リポジトリ", repository),
        ("ライセンス", env!("CARGO_PKG_LICENSE")),
        ("不具合報告", issues),
        ("OS", &os),
        ("端末", &terminal),
        ("画像描画の検出", &image),
        ("使用中の描画方式", info.rendering.label()),
        ("接続状態", info.connection),
    ]);
    content.push(color(&fit("アプリと動作環境", inner), INK, display.colors));
    if let Some(release) = info
        .release
        .filter(|release| release.browser == Browser::Failed)
    {
        content.extend(
            ["ブラウザーを開けませんでした。", &release.url()]
                .into_iter()
                .flat_map(|line| wrap(line, inner))
                .map(|line| color(&line, MUTED, display.colors)),
        );
    }
    content.extend(about_rows(&diagnostics, inner, display.colors));
    content.push(String::new());
    let features = [
        ("地震", "地震の履歴と、地域別・観測点別の震度を表示します。"),
        (
            "EEW",
            "速報が重なると、地震ごとに一覧で表示します。地図は警報対象・推定震度を切り替えられます。P波・S波は現在の推定波面を表示します。",
        ),
        (
            "津波",
            "警報・注意報・予報を沿岸線と地域一覧で色分けします。",
        ),
    ];
    content.push(color(&fit("表示できる情報", inner), INK, display.colors));
    content.extend(about_rows(&features, inner, display.colors));
    content.push(String::new());
    content.push(color(
        &fit("データとクレジット", inner),
        INK,
        display.colors,
    ));
    let credits = [
        ("通常配信", "Wolfx Project"),
        ("予備配信", "P2P地震情報"),
        (
            "気象庁",
            "震度観測点一覧・地震情報／細分区域GIS・津波予報区",
        ),
        ("国土交通省", "国土数値情報（湖沼データ）"),
        ("Natural Earth", "1:50m Land · 東アジアの周辺陸地"),
        ("FIGlet Slant", "Glenn Chappell"),
    ];
    content.extend(about_rows(&credits, inner, display.colors));
    content.extend(
        wrap(
            "データの出典と加工内容は README.md を参照してください。",
            inner,
        )
        .into_iter()
        .map(|line| color(&line, MUTED, display.colors)),
    );

    let total = content.len();
    *scroll = (*scroll).min(total.saturating_sub(viewport));
    let end = (*scroll + viewport).min(total);
    let title = if end > *scroll && total > viewport {
        format!("アプリ情報  {}–{end}/{total}", *scroll + 1)
    } else {
        "アプリ情報".into()
    };
    pinned.extend(content.into_iter().skip(*scroll).take(viewport));
    let height = pinned.len() + 2;
    let mut rows = menu.to_vec();
    rows.extend(Border::new(width, display.colors).text(pinned, &title, height));
    let actions: &[&str] = if info.release.is_some() {
        &["Enter リリースページ", "↑↓ スクロール", "Esc 戻る"]
    } else {
        &["↑↓ スクロール", "Esc 戻る"]
    };
    rows.push(footer(actions, display));
    Frame {
        text: rows.join("\n"),
        ..Frame::default()
    }
}
