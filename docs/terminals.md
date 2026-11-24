# ターミナルの画像描画方式

[READMEの必要な環境に戻る](../README.md#必要な環境)

Terminal Troveの「Image Support」をもとにした、各ターミナルの画像描画方式の一覧です。各端末の資料も参照しています。表の「—」は、資料で対応を確認できなかった方式です。 [Terminal Troveの比較表](https://terminaltrove.com/compare/terminals/)

🪟 Windows / 🐧 Linux / 🍎 macOS

| ターミナル | OS | Sixel | Kitty画像描画 |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------- |
| [Bobcat](https://github.com/ismail-yilmaz/Bobcat) | 🪟 🐧 🍎 | 対応 | 対応 |
| [cmux](https://cmux.com/) | 🍎 | — | 対応 |
| [Contour Terminal](https://contour-terminal.org/) | 🪟 🐧 🍎 | 対応 | 対応 |
| [foot](https://codeberg.org/dnkl/foot) | 🐧 | 対応 | — |
| [Ghostty](https://ghostty.org/) | 🐧 🍎 | — | 対応 |
| [iTerm2](https://iterm2.com/) | 🍎 | [対応](https://iterm2.com/downloads.html) | 対応 |
| [kitty](https://sw.kovidgoyal.net/kitty/) | 🐧 🍎 | — | 対応 |
| [Konsole](https://apps.kde.org/konsole/) | 🪟（[開発版](https://apps.kde.org/konsole/)） 🐧 🍎（ARM・開発版） | 対応 | 一部対応 |
| [Mintty](https://mintty.github.io/) | 🪟 | 対応 | — |
| [mlterm](https://mlterm.sourceforge.net/) | 🪟 🐧 🍎（[旧macOSで動作確認](https://github.com/arakiken/mlterm)） | 対応 | — |
| [Ratty](https://github.com/orhun/ratty) | 🪟 🐧 🍎 | — | 対応 |
| [Rex](https://www.superlogical.com/) | 🪟（一部のベータ参加者） 🍎（ベータ） | — | [対応（比較表の記載）](https://terminaltrove.com/terminals/rex-terminal/) |
| [Rio Terminal](https://rioterm.com/) | 🪟 🐧 🍎 | 対応 | 対応 |
| [Tabby](https://tabby.sh/) | 🪟 🐧 🍎 | 対応（設定による） | — |
| [Terminology](https://www.enlightenment.org/about-terminology) | 🐧 | — | — |
| [Warp Terminal](https://www.warp.dev/) | 🪟 🐧 🍎 | — | 一部対応 |
| [WezTerm](https://wezterm.org/) | 🪟 🐧 🍎 | 試験的 | 対応 |
| [Windows Terminal](https://learn.microsoft.com/ja-jp/windows/terminal/) | 🪟 | 1.22以降 | — |
| [xterm](https://invisible-island.net/xterm/) | 🪟（[Cygwin/X](https://cygwin.com/packages/summary/xterm.html)） 🐧 🍎（[XQuartz](https://www.xquartz.org/FAQs.html)） | 対応（VT340などの設定による） | — |
| [Extraterm](https://extraterm.org/) | 🪟 🐧 🍎 | — | — |

Terminologyは `tycat`、Extratermは `show` で画像を表示できます。実際の描画方式は、使用中の端末でTuiQuakeを起動して確認してください。
