# TuiQuake

<img src="docs/media/tuiquake-logo.svg" alt="TuiQuake" width="480">

日本の地震情報、緊急地震速報、津波情報をターミナルで確認できます。

[![CI](https://github.com/hirokawa-beach/TuiQuake/actions/workflows/ci.yml/badge.svg)](https://github.com/hirokawa-beach/TuiQuake/actions/workflows/ci.yml)
[![GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-3b769b)](LICENSE)
[![Releases](https://img.shields.io/github/v/release/hirokawa-beach/TuiQuake?display_name=tag)](https://github.com/hirokawa-beach/TuiQuake/releases)

> **非公式の閲覧用アプリです。** 配信や通信による遅延・欠落があります。防災時は気象庁・自治体の公式情報を確認してください。

> [!NOTE]
> Unicode Brailleと文字地図は表示調整中です。地図を見るときは、SixelまたはKittyの画像描画に対応したターミナルでの利用を推奨します。 [対応ターミナル](#必要な環境)

## 目次

- [画面例](#画面例)
- [特長](#特長)
- [クイックスタート](#クイックスタート)
- [必要な環境](#必要な環境)
- [主なキー](#主なキー)
- [設定](#設定)
- [仕組みとデータ](#仕組みとデータ)
- [ソースからビルド](#ソースからビルド)
- [トラブルシュート](#トラブルシュート)
- [English](#english)
- [ライセンスとデータの出典](#ライセンスとデータの出典)

## 画面例

<a href="docs/media/earthquakes-regions.png"><img src="docs/media/earthquakes-regions.png" alt="令和8年熊本地震の地域別表示。各区域の最大震度を色と数字で表示。" width="760"></a>

_表示している地震は過去のものです。_

2026年7月28日の令和8年熊本地震です。 [気象庁の震度データベース](https://www.data.jma.go.jp/eqdb/data/shindo/index.html#20260728162715) [令和8年熊本地震ポータル](https://www.jma.go.jp/jma/menu/20260728_kumamoto_jishin.html) [気象庁の区域一覧](https://www.jma.go.jp/jma/kishou/know/jishin/joho/shindo-name.html)

<details>
<summary>観測点ごとの震度を見る</summary>

<a href="docs/media/earthquakes.png"><img src="docs/media/earthquakes.png" alt="令和8年熊本地震の観測点表示。震度1から7を観測した地点を色で表示。" width="760"></a>

地図の点は本震で震度1〜7を観測した1,252地点です。地震タブで `V` を押すと、地域別と観測点別の表示を切り替えられます。

</details>

<details>
<summary>令和8年熊本地震のEEWを再生（第24報まで）</summary>

[EEWの再生動画を開く（MP4・32.2秒）](docs/media/eew-demo.mp4)

</details>

## 特長

- **地震履歴を見る。** 履歴から選んだ地震の震源と各地の震度を確認できます。地域ごとの最大震度と観測点ごとの記録を切り替えられます。
- **緊急地震速報（EEW）の更新を追う。** 受信したEEWの情報(予報/警報)を表示します。地図は警報の対象地域と地域ごとの推定震度を切り替えられます。
- **津波情報を確認する。** 大津波警報、津波警報、注意報、予報を沿岸線の色で示します。地域ごとの波高や第1波の見込みは、地図の横の一覧で確認できます。

## クイックスタート

公開済みのバイナリがあれば、OSとCPUに合うファイルをダウンロードしてください。 [Releases](https://github.com/hirokawa-beach/TuiQuake/releases)

リリースがまだなければ、ソースからビルドできます。 [ビルド手順](#ソースからビルド)

| 環境                           | ファイル                                           |
| ------------------------------ | -------------------------------------------------- |
| Windows（x86-64） | `tuiquake-windows-x86_64.msi`（インストーラー） / `tuiquake-windows-x86_64.exe`（単体版） |
| Linux（x86-64 / ARM64）        | `tuiquake-linux-x86_64` / `tuiquake-linux-aarch64` |
| macOS（Intel / Apple Silicon） | `tuiquake-macos-x86_64` / `tuiquake-macos-aarch64` |

**Windows**

`tuiquake-windows-x86_64.msi` を開いてインストールしてください。新しく開いたターミナルで `tuiquake` と入力すると起動できます。アンインストールはWindowsの「インストールされているアプリ」から行えます。

インストールせずに使う場合は `tuiquake-windows-x86_64.exe` をダウンロードし、保存先を開いたPowerShellで `.\tuiquake-windows-x86_64.exe` を実行します。

**Linux**

```bash
chmod +x ./tuiquake-linux-x86_64
./tuiquake-linux-x86_64
```

Linuxの例はx86-64です。ARM64やmacOSでは、手順中のファイル名を自分の機種用に置き換えてください。

## 必要な環境

画像地図を表示するには、SixelまたはKittyの画像描画に対応したターミナルエミュレーター(以後ターミナルと呼びます)が必要です。

<details>
<summary>画像表示に対応するターミナル一覧（20件）</summary>

Terminal Troveの「Image Support」に掲載された20件です。表の「—」は、参照した資料で対応を確認できなかった方式です。 [Terminal Troveの比較表](https://terminaltrove.com/compare/terminals/)

凡例: 🪟 Windows ／ 🐧 Linux ／ 🍎 macOS

| ターミナル                                                                                           | OS                                                                                                                     | Sixel                                     | Kitty画像描画                                                             |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------- |
| [Bobcat](https://github.com/ismail-yilmaz/Bobcat)                                                    | 🪟 🐧 🍎                                                                                                               | 対応                                      | 対応                                                                      |
| [cmux](https://cmux.com/)                                                                            | 🍎                                                                                                                     | —                                         | 対応                                                                      |
| [Contour Terminal](https://contour-terminal.org/)                                                    | 🪟 🐧 🍎                                                                                                               | 対応                                      | 対応                                                                      |
| [foot](https://codeberg.org/dnkl/foot)                                                               | 🐧                                                                                                                     | 対応                                      | —                                                                         |
| [Ghostty](https://ghostty.org/)                                                                      | 🐧 🍎                                                                                                                  | —                                         | 対応                                                                      |
| [iTerm2](https://iterm2.com/)                                                                        | 🍎                                                                                                                     | [対応](https://iterm2.com/downloads.html) | 対応                                                                      |
| [kitty](https://sw.kovidgoyal.net/kitty/)                                                            | 🐧 🍎                                                                                                                  | —                                         | 対応                                                                      |
| [Konsole](https://apps.kde.org/konsole/)                                                             | 🪟（[開発版](https://apps.kde.org/konsole/)） 🐧 🍎（ARM・開発版）                                                     | 対応                                      | 一部対応                                                                  |
| [Mintty](https://mintty.github.io/)                                                                  | 🪟                                                                                                                     | 対応                                      | —                                                                         |
| [mlterm](https://mlterm.sourceforge.net/)                                                            | 🪟 🐧 🍎（[旧macOSで動作確認](https://github.com/arakiken/mlterm)）                                                    | 対応                                      | —                                                                         |
| [Ratty](https://github.com/orhun/ratty)                                                              | 🪟 🐧 🍎                                                                                                               | —                                         | 対応                                                                      |
| [Rex](https://www.superlogical.com/)                                                                 | 🪟（一部のベータ参加者） 🍎（ベータ）                                                                                  | —                                         | [対応（比較表の記載）](https://terminaltrove.com/terminals/rex-terminal/) |
| [Rio Terminal](https://rioterm.com/)                                                                 | 🪟 🐧 🍎                                                                                                               | 対応                                      | 対応                                                                      |
| [Tabby](https://tabby.sh/)                                                                           | 🪟 🐧 🍎                                                                                                               | 対応（設定による）                        | —                                                                         |
| [Terminology](https://www.enlightenment.org/about-terminology)                                       | 🐧                                                                                                                     | —                                         | —                                                                         |
| [Warp Terminal](https://www.warp.dev/)                                                               | 🪟 🐧 🍎                                                                                                               | —                                         | 一部対応                                                                  |
| [WezTerm](https://wezterm.org/)                                                                      | 🪟 🐧 🍎                                                                                                               | 試験的                                    | 対応                                                                      |
| [Windows Terminal](https://learn.microsoft.com/ja-jp/windows/terminal/)                              | 🪟                                                                                                                     | 1.22以降                                  | —                                                                         |
| [xterm](https://invisible-island.net/xterm/)                                                         | 🪟（[Cygwin/X](https://cygwin.com/packages/summary/xterm.html)） 🐧 🍎（[XQuartz](https://www.xquartz.org/FAQs.html)） | 対応（VT340などの設定による）             | —                                                                         |
| [Extraterm](https://extraterm.org/)                                                                  | 🪟 🐧 🍎                                                                                                               | —                                         | —                                                                         |

Terminologyは `tycat`、Extratermは `show` で画像を表示できます。実際の描画方式は、使用中の端末でTuiQuakeを起動して確認してください。

</details>

起動時の選択画面に表示された描画方式を選んでください。画像描画方式が検出されない場合は、Unicode Braille（表示調整中）で起動できます。

日本語を表示できる等幅フォントを使ってください。Windows Terminalでは Cascadia Mono を使えます。ライブ配信の受信にはインターネット接続が必要です。 [Windows Terminal の外観設定](https://learn.microsoft.com/ja-jp/windows/terminal/customize-settings/profile-appearance)

## 主なキー

| 操作                                        | キー                                            |
| ------------------------------------------- | ----------------------------------------------- |
| タブ移動                                    | 1 地震 / 2 EEW / 3 津波 / 4 設定 / 5 アプリ情報 |
| 地震履歴を選ぶ / 各地の震度をスクロール     | `↑` `↓` / `←` `→`                               |
| 地図を移動 / 拡大・縮小                     | `W` `A` `S` `D` / `+` `−`                       |
| 地震の震度表示を切り替え（地域別 / 観測点） | `V`                                             |
| EEWの地図を切り替え（警報対象 / 推定震度）  | `V`                                             |
| 津波の地図を切り替え（全国 / 対象沿岸）     | `Z`                                             |
| 文字で詳細を表示                            | `I`                                             |
| 今のタブの操作を確認 / 終了                 | `H` / `Q`                                       |

狭い端末では `B` で地震履歴と地図を切り替えられます。

## 設定

`4` で設定を開き、`↑` `↓` で項目を選びます。`←` `→` または `Space` で値を変更すると、アプリが自動で保存します。

- 描画方式を「自動」にすると、次の起動時に選択画面が開きます。方式を指定するとその場で切り替わります。起動時に地図をどの範囲で開くかも設定できます。
- 震度は初期設定で観測点を震度別に表示します。都道府県別にまとめたり、地域別の表示に切り替えたりできます。
- 地震履歴の初期値は10行です。5 / 10 / 20 / 30行、または端末の高さに合わせる「自動」を選べます。
- EEWの端末ベルを有効にすると、初報を受信したときと警報に切り替わったときに端末へベルを送ります。設定行の `Enter` で試し鳴らしができます。音や通知の出方は端末の設定に従います。

## 仕組みとデータ

右上のステータスで接続先、最後の応答時刻（JST）、EEW・津波の状況が分かります。

### 配信元

地震一覧、EEW、津波情報は Wolfx の WebSocket で受信し、地震詳細は Wolfx の HTTP API から取得します。ライブ配信が途切れると P2P地震情報へ切り替え、Wolfx が復旧すると元の配信へ戻ります。P2P から受け取る EEW は警報です。

### 緊急地震速報（EEW）

配信元のサーバー時刻を基準に、発表から120秒未満の報を表示します。警報が出た地震では、その後に予報が更新されても赤い帯と警報対象地域が残ります。警報の続報で対象地域を追加し、取消報を受けると警報表示を消します。 [気象庁の発表基準](https://www.jma.go.jp/jma/kishou/know/jishin/eew/shikumi/shousai.html)

地図は `V` で「警報対象」と「推定震度」を切り替えられます。推定震度は気象庁の配信値を使います。範囲が示されたときはその上限、下限だけが示されたときはその下限の色で描画します。受信した範囲は一覧で確認できます。「震源周辺」にすると、表示対象が収まる範囲で最大約10.5倍まで自動で拡大します。 [地震動の予報業務許可に関する気象庁のFAQ](https://www.jma.go.jp/jma/kishou/minkan/q_a_s.html)

「速報概要」の推定最大震度も、幅がある場合は上限をバッジに使います。

### 津波情報

地図は全国66の津波予報区のうち、確認期限内の発表がある沿岸を色分けします。大津波警報は紫、津波警報は赤、津波注意報は黄、津波予報は水色です。一覧でも同じ色を発表区分の背景と地域名に使います。波高と第1波の見込みもここで確認できます。 [気象庁の津波予報区](https://www.jma.go.jp/jma/kishou/know/jishin/joho/tsunami-yohoku.html)

`Z` で全国と対象沿岸を切り替えます。確認期限切れの情報とP2Pの参考情報では、沿岸線に現況の色を付けません。

## ソースからビルド

[Git](https://git-scm.com/install/) と [Rust](https://rust-lang.org/tools/install/) を用意します。Windows は [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)、Ubuntu は `build-essential`、macOS は Xcode Command Line Tools（`xcode-select --install`）も必要です。

```bash
git clone https://github.com/hirokawa-beach/TuiQuake.git
cd TuiQuake
cargo build --release --locked
```

Windows の PowerShell では `.\target\release\tuiquake.exe`、Linux / macOS では `./target/release/tuiquake` で起動します。

<details>
<summary>コマンドで直接表示する(開発者向け)</summary>

| コマンド                   | 表示内容                                                      |
| -------------------------- | ------------------------------------------------------------- |
| `latest`                   | 最新の地震                                                    |
| `eew` / `replay --history` | 最新の緊急地震速報1報（P2P切り替え時は警報）                  |
| `replay --delay 4`         | 架空の地震と速報のデモを4秒間隔で表示。オフラインでも利用可能 |
| `watch`                    | 地震と緊急地震速報を受信し次第更新                            |

```powershell
.\target\release\tuiquake.exe latest --japan
```

```bash
./target/release/tuiquake latest --japan
```

`--japan` は全国表示です。`--map sixel` / `--map kitty` / `--map braille` / `--map text` で描画方式を指定できます。ファイルやパイプに出力するときは、既定で文字地図（text）を使います。

</details>

## トラブルシュート

### 地図に隙間が出る、文字の桁がずれる

等幅フォントを選び、日本語の代替フォントと端末の文字間隔を確認してください。日本語の等幅フォントには Noto Sans Mono CJK JP も使えます。 [Noto Sans CJK のダウンロード案内](https://github.com/notofonts/noto-cjk/tree/main/Sans#language-specific-otfs)

### 画像地図が表示されない

端末が Sixel または Kitty に対応しているか確認してください。`--map text` で文字地図を選べます。文字地図と Unicode Braille は非推奨です。この2方式では観測点の震度は一覧に表示されます。

### 不具合を報告する

「5 アプリ情報」で確認できるバージョン、端末、描画方式と、問題が出たときの操作を添えてください。 [GitHub Issues](https://github.com/hirokawa-beach/TuiQuake/issues)

## English

TuiQuake is a Japanese-language terminal app for browsing earthquakes, station-level seismic intensity, earthquake early warnings, and tsunami bulletins. It offers Sixel, Kitty, and text maps.  
It is an unofficial viewer. For safety decisions, consult official information from the JMA or local authorities.

## ライセンスとデータの出典

TuiQuake のオリジナルコードは [GNU General Public License v3.0](LICENSE)（GPL-3.0-only）です。配布バイナリの依存ライブラリのライセンスは [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) に記載しております。

- **地震・EEW・津波** — データ提供 Wolfx Project。気象庁発表の情報を [Wolfx WebSocket](https://wolfx.jp/docs/websocket/) と [Wolfx Open API](https://wolfx.jp/docs/open-api/) 経由で取得します。[Wolfx Project の利用規約](https://wolfx.jp/legal/terms/)
- **予備配信(フォールバック)** — P2P地震情報。 [P2P地震情報 API](https://www.p2pquake.net/develop/json_api_v2/) [二次利用ガイドライン](https://www.p2pquake.net/secondary_use/)
- **震度観測点と区域** — 気象庁の [震度観測点一覧](https://www.data.jma.go.jp/eqev/data/bulletin/shindo.html) と [予報区等GISデータ「地震情報／細分区域」](https://www.data.jma.go.jp/developer/gis.html) を加工しました。 [気象庁ホームページの利用規約](https://www.jma.go.jp/jma/kishou/info/coment.html)
- **津波地図** — 気象庁の防災情報サイトの津波予報区データから、区域コードと沿岸線を抽出して描画しています。 [気象庁の津波予報区](https://www.jma.go.jp/jma/kishou/know/jishin/joho/tsunami-yohoku.html) [地図データ（GeoJSON）](https://www.jma.go.jp/bosai/common/const/geojson/tsunami.json) [気象庁ホームページの利用規約](https://www.jma.go.jp/jma/kishou/info/coment.html)
- **湖沼** — 国土交通省の [国土数値情報（湖沼データ W09、平成17年版）](https://nlftp.mlit.go.jp/ksj/gml/datalist/KsjTmplt-W09-v2_2.html) から面積10 km²以上の湖沼を抽出し、輪郭を簡略化しました。整備時点と現在の湖岸線には差がある場合があります。 [国土数値情報の利用規約](https://nlftp.mlit.go.jp/ksj/other/agreement_02.html)
- **周辺陸地** — [Natural Earth 1:50m Land](https://www.naturalearthdata.com/downloads/50m-physical-vectors/50m-land/) のうち、東経110〜159度・北緯18〜55度の範囲を切り出しました。 [Natural Earth の利用条件（パブリックドメイン）](https://www.naturalearthdata.com/about/terms-of-use/)
<details>
<summary>気象庁の地図データの加工内容</summary>

- `stations.json` には震度観測点一覧から抽出した名前と座標を収録しています。位置情報のない観測点は文字詳細に表示します。
- `regions.json` には地震情報の細分区域の名称・コード・ポリゴンを収録しています。共有境界をまとめて簡略化し、細い隙間を補正しています。ごく小さな島の輪郭は省略しています。
- `tsunami_coasts.json` には津波予報区の区域コードと沿岸線を収録しています。

元の地図と利用上の注意は公開元のページに記載されています。 [気象庁の予報区等GISデータ一覧](https://www.data.jma.go.jp/developer/gis.html)

</details>

<details>
<summary>地図データを再生成する（開発者向け）</summary>

通常のビルドではリポジトリに含まれる地図データを使います。再生成するには、気象庁の一覧で「地震情報／細分区域」を選び、湖沼データと津波予報区の `tsunami.json` も取得します。 [気象庁の予報区等GISデータ一覧](https://www.data.jma.go.jp/developer/gis.html) [国土交通省の湖沼データ](https://nlftp.mlit.go.jp/ksj/gml/datalist/KsjTmplt-W09-v2_2.html) [気象庁の津波予報区GeoJSON](https://www.jma.go.jp/bosai/common/const/geojson/tsunami.json)

Python 3.11以上とNode.js 20.11以上で、リポジトリのルートから次のコマンドを実行します。

```powershell
npm install --no-save --no-package-lock --ignore-scripts mapshaper@0.8.1
python tools/prepare_regions.py 20240520_AreaForecastLocalE_GIS.zip regions.json
python tools/prepare_lakes.py W09-05_GML.zip lakes.json
python tools/prepare_tsunami.py tsunami.json tsunami_coasts.json
```

</details>
