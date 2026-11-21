# TuiQuake

日本の地震情報、緊急地震速報（EEW）、津波情報をターミナルで見る Rust アプリです。

[ソースからビルド](#ソースからビルド)してから、Windows Terminal の PowerShell で実行します。地図データは実行ファイルに入っています。

```powershell
.\TuiQuake.exe
```

TUI の起動時に地図の描画方式を選びます。端末で対応を確認できた Sixel・Kitty と、文字で描く Unicode Braille が候補です。上下キーと Enter、または行番号で選択できます。Braille の表示は端末のフォントに依存するので、候補の横にサンプルを表示します。

`--map sixel`、`--map kitty`、`--map braille` で方式を直接指定できます。`--map text` は4分割ブロックの文字地図です。`latest`、`eew`、`replay`、`watch` は自動で方式を選びます。画像を利用できない場合やファイル・パイプへの出力では、既定で4分割ブロックの地図を使います。

| キー | 操作 |
|---|---|
| `1` / `2` / `3` / `4` / `5` | 地震 / EEW / 津波 / デモ / About |
| `Tab` / `Shift+Tab` | 次のメニュー / 前のメニュー |
| `↑` / `↓` | 地震履歴の選択、または文字詳細・津波情報のスクロール |
| `←` / `→` | 地震タブでは各地の震度をスクロール。デモのEEWでは予測震度を変更 |
| `Enter` | 選択した地震の震源周辺に寄る |
| `I` / `B` | 文字詳細を開く / 地図に戻る。端末幅が狭い場合は `B` で一覧も開けます |
| `V` | 広域の最大観測震度と、各観測点の震度を切り替える |
| `W` / `A` / `S` / `D` | 地図の表示範囲内で上・左・下・右に移動 |
| `+` / `-` | 地図を拡大・縮小。テンキーと全角の `＋` にも対応 |
| `Z` / `0` | 全国と震源周辺を切り替える / 表示位置と倍率を戻す |
| `Space` | デモを再生・停止 |
| `F` | デモのEEWを予報と警報で切り替える |
| `R` / `Q` | 地震・津波情報を再取得、デモを初期化 / 終了 |

地図は最初に震源周辺を表示します。地図枠の倍率は、収録範囲全体が入る表示を 1.0x とした倍率です。`+` と `-` は選択中の地震の震央や警報範囲などを含む範囲の中心を基準に、1段ずつ倍率を変えます。EEWの自動表示は最も引いた状態から3段まで拡大します。

地震タブでは地図の横に、選択した地震の概要、各地の震度、地震履歴が並びます。端末の幅が狭いときは `B` で履歴と地図を切り替えます。ホイールで地震の選択は変わりません。

TUI の EEW は Wolfx の JMA 緊急地震速報 WebSocket に常時接続し、他のタブを開いていても速報を受信します。新しい速報がある間はメニューに「受信」と出ます。接続が切れたら自動で再接続します。津波タブは約30秒ごと、地震タブは約1分ごとに表示中の情報を更新します。

広域表示では地域の最大震度を地図上に表示します。地図上の `5-`・`5+` と `6-`・`6+` は弱・強を表し、数字が重なると強い震度を優先します。観測点表示では地点をマーカーで示し、各地点の震度は `I` の文字詳細で確認できます。

## コマンドから直接表示

```powershell
.\TuiQuake.exe latest
.\TuiQuake.exe eew
.\TuiQuake.exe replay
.\TuiQuake.exe replay --history
.\TuiQuake.exe watch
```

`replay` のデモはオフラインでも動きます。`--delay 4` で切り替えを4秒間隔にできます。`replay --history` は `eew` と同じ最新1報を表示します。`--japan` で全国を表示します。`watch` は約10秒ごとに閲覧用の公開データを取得します。配信や通信の遅延・欠落があるため、防災時は公式の緊急地震速報を利用してください。

## ソースからビルド

Windows では Rust と Visual Studio C++ Build Tools が必要です。Rust 1.94.1 で動作を確認しています。

```powershell
cargo build --release --locked
Copy-Item .\target\release\tuiquake.exe .\TuiQuake.exe
.\TuiQuake.exe
```

## ライセンス

TuiQuake のオリジナルコードは、[GNU General Public License v3.0](LICENSE)（GPL-3.0-only）で公開しています。地図・観測点データなどの利用条件はデータ出典欄に、依存ライブラリのライセンス文は [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) に記載しています。Wolfx Project の API 利用には [Wolfx Project の利用規約](https://wolfx.jp/legal/terms/)が適用されます。

## データ出典

- **データ提供 Wolfx Project**。TuiQuakeは、気象庁発表の地震履歴・震度・EEW・津波情報を [Wolfx Open API](https://wolfx.jp/docs/open-api/) 経由で表示する非公式の閲覧アプリです。詳細APIに初回接続できない場合は、Wolfx Projectの地震一覧から最大震度を表示します。データを使う機能は無償で提供します。[Wolfx Project 利用規約](https://wolfx.jp/legal/terms/)
- 観測点の地図上の位置は気象庁の [震度観測点一覧](https://www.data.jma.go.jp/eqev/data/bulletin/shindo.html) から観測点名と座標を抽出し、`stations.json` として実行ファイルに内蔵しています。位置情報のない観測点は文字詳細に表示します。
- `regions.json` は、気象庁の[予報区等GISデータ「地震情報／細分区域」](https://www.data.jma.go.jp/developer/gis.html)の区域コード・名称付きのポリゴンを、TuiQuakeが簡略化して作成したものです。区域の共有境界をまとめて簡略化し、元データの細い隙間を補正しています。ごく小さな島の輪郭は省略しています。観測点一覧とGISデータには[気象庁ホームページ利用規約](https://www.jma.go.jp/jma/kishou/info/coment.html)が適用されます。
  - 気象庁のGISデータ一覧に記載された国土地理院の元データは、数値地図25000（空間データ基盤）、数値地図（国土基本情報）、電子国土基本図（地図情報）です。同一覧に記載された承認番号は、平30情使 第401号、令2情使 第125号、令3情使 第90号、令3情使 第123号、令3情使 第648号、令4情使 第437号、令5情使 第71号、令6情使 第382号、令7情使 第395号です。
- 湖沼は、国土交通省の[国土数値情報（湖沼データ W09、平成17年版）](https://nlftp.mlit.go.jp/ksj/gml/datalist/KsjTmplt-W09-v2_2.html)から面積10 km²以上の湖沼を抽出し、TuiQuakeが輪郭を簡略化して作成しました。データは整備時点のため、現在の湖岸線と差がある場合があります。[国土数値情報の利用規約（旧国土情報利用約款準拠版）](https://nlftp.mlit.go.jp/ksj/other/agreement_02.html)の「商用可」が適用されます。
- 周辺陸地は [Natural Earth 1:50m Land](https://www.naturalearthdata.com/downloads/50m-physical-vectors/50m-land/) から東経110〜159度・北緯18〜55度の範囲を切り出しました。[パブリックドメイン](https://www.naturalearthdata.com/about/terms-of-use/)のデータです。
- ロゴは [FIGlet Slant](https://www.figlet.org/cgi-bin/fontdb_example.cgi?font=slant.flf)（Glenn Chappell）で生成した文字列を使っています。色付けはTuiQuakeで行っています。
- Windows版で使用するRust依存クレートのライセンス文は [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) に収録しています。

### 地図データの再生成

通常のビルドには生成済みの `regions.json` と `lakes.json` を使用します。元データから作り直す場合は、気象庁の[地震情報／細分区域のZIP](https://www.data.jma.go.jp/developer/gis/20240520_AreaForecastLocalE_GIS.zip)と、国土交通省の[湖沼データのZIP](https://nlftp.mlit.go.jp/ksj/gml/datalist/KsjTmplt-W09-v2_2.html)を取得し、Python 3.11以上とNode.js 20.11以上で次を実行します。

```powershell
npm install --no-save --no-package-lock --ignore-scripts mapshaper@0.8.1
python tools/prepare_regions.py 20240520_AreaForecastLocalE_GIS.zip regions.json
python tools/prepare_lakes.py W09-05_GML.zip lakes.json
```
