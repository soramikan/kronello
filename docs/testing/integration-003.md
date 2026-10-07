# INTEGRATION-003 の検証

対象: `soramikan/kronello-m7-lane-e` / `m7-lane-e`、2026-10-07、Darwin arm64 / Rust 1.95.0 / FFmpeg 9.0.2。
worker sandbox で debug binary の `--backend gpu` 実行を完了し、driver の 219 checks は全件合格した。
macOS アプリ画面を使う GUI 経路の受け入れは本 lane では未実行であり、親エージェントの
Computer Use 検証へ委譲する（手順は末尾）。実装完了と受け入れ合格を区別し、
backlog の `INTEGRATION-003` の status は本変更では更新していない。

## 受け入れ条件との対応

| 条件 | driver の check / 証拠 | 結果 |
|---|---|---|
| 1. カット編集・字幕・色補正を含む作品を GUI と CLI の両経路で生成する | CLI 経路: `cli.*` 101 checks。MCP 経路: `mcp.*` 101 checks。両経路で同一の 18 件の公開 Command を同一引数で適用し、`parity.sequence.query` / `parity.project.export` / `parity.frame.*` / `parity.captions.export` / `parity.job.*` で canonical bytes が厳密一致。GUI 経路は後述手順で親エージェントが実施する | CLI/MCP は合格、GUI は未実行 |
| 2. 書き出し結果の映像・音声・字幕を受け入れ基準として検証する | 映像: `*.job.frames.*`（24 frames・metadata・rgba16f/PNG 完全性）と `*.job.movie.*`（ffprobe で prores 320×180）。音声: `*.job.movie.audio_level`（pcm_s24le 48 kHz 2ch、astats RMS ≈ −15.05 dB）。字幕: `*.job.sidecar.*`（job 出力 bytes = `captions.export` の SRT）と `*.captions.burned.*`（焼き込み画素） | 合格 |

CLI と MCP の「両経路」は、受け入れ条件の GUI/CLI 二分に対し MCP を共有 API の第二経路として
扱った driver の検証範囲である。GUI 経路の作品生成は同じ共有 API に乗る macOS アプリで
後述手順を実行し、同じ evidence manifest に照合する想定。

## 再現 driver と入力

`scripts/demo_integration_m7.py` は `demo_integration_m2.py` の `Demo`（実 subprocess の
CLI / stdio MCP、isolated `KRONELLO_STATE_ROOT`、request 記録、check 集約）を継承し、
`demo_integration_m3.py` の canonical JSON 比較を使う。実 `kronello` / `kronello-mcp`
プロセスと公開 Command / Query API のみを使い、SQLite・内部 crate・system font の探索に
触れない。

全 ID は `uuid.uuid5` の固定 namespace から導出し、CLI / MCP の両 channel が
byte-identical な document を作る。ID を配列番号や表示名から導出しない規約に従い、
track / clip / caption / marker / property ID は全て事前生成した UUIDv5。

1 project は `320×180` design extent、24 fps、48 kHz、`linear_rec709` の sequence に
video / audio の 2 track で始まり、以下を 18 revision で構築する。

| 要素 | 公開操作 | 内容 |
|---|---|---|
| クリップ配置 | `clip.place` ×4 | solid generator の A `[0,5)` 赤、B `[5,10)` 緑、C `[10,14)` 青を video track に、`kronello.audio.tone440` を audio track `[0,10)` に |
| カット編集 | `clip.trim` / `clip_split` / `clip_slip` / `clip_stretch` / `ripple_delete` | C を `[10,13)` に trim、B を 7 秒で split、右半 B2 を +1 秒 slip（source_in 2→3）、B 前半を `[4,7)` に stretch、B2 `[7,10)` を ripple delete。結果 A `[0,5)` / B `[4,7)` / C `[7,10)` |
| トランジション | `transition_set`（stretch と同一 transaction） | A→B の wipe left `[4,5)`。overlap 検証に必要な stretch を同じ `edit.apply` に入れる |
| 字幕 | `captions.import_plan` / `captions.import` / `caption_set` / `clip.trim` | SRT 3 cue を `captions.import` で caption track に配置（plan は 1 track_append + 3 clip_place + 3 caption_set の 7 command）。cue1 本文を `caption_set` で「編集済みの最初の字幕」に書換え、cue2 を `[17/4, 11/2)` に trim |
| マーカー | `marker_set` ×2 / `marker_move` | sequence marker を 2→3 秒へ移動、clip marker を A の source 1 秒に |
| ワークエリア | `work_area_set` | In/Out `[1,7)`。content extent `[0,10)` 内の非空区間 |
| 色補正 | `clip_set_effects` | C に `kronello.color.exposure` version 1、exposure=+1（exposure_offset=0） |

日本語字幕は `target/fixtures/external/NotoSansCJKjp-Regular.otf`（OFL-1.1、
manifest pinned、SHA-256 `68a3fc98…f375b5`）を `font.pin` で pin し、
`caption_set` の style と render input の font locator に使う。
font input は snapshot font lock に含まれる identity が必須であり、
caption import 前に render input へ渡すと `font input is not a snapshot lock` の型付き拒否になる。

`--backend` は `gpu` / `cpu-reference`、既定 `gpu`、暗黙 fallback なし。
`--resolution` は `small`（既定、320×180 raster）と `4k`（3840×2160 raster、同じ design extent）。
`--output-directory` は新規 directory を要求し、`report.json` に全 request / check を残す。
`gui-evidence.json` に全 stable ID・cue 時刻・本文・work area・transition・font lock を保存する。

## 実行記録

```sh
cargo build -p kronello-cli -p kronello-mcp
python3 scripts/demo_integration_m7.py \
  --binary-dir target/debug \
  --output-directory /tmp/m7-demo-out
```

結果: `{"status": "verified", "checks": 219}`、失敗 0、530 requests。
`/tmp/m7-demo-out/` には `m7-integration.cli.kronello` / `m7-integration.mcp.kronello`
（revision 18）、`report.json`、`gui-evidence.json`、`project.export.json`、
`cli-frames` / `mcp-frames`（各 24 frames の PNG/rgba16f/JSON）、
`cli-captions.srt` / `mcp-captions.srt`、`cli-movie.mov` / `mcp-movie.mov`、
`mcp.stderr.log`、隔離 `state/` を保存した。

### 映像・音声・字幕・ワークエリアの判定内容

- ワークエリア: `*.job.frames.work_area_range` は manifest の range が `[1,7)`、
  frame 時刻が `1 + k/4`（rate 4、24 frames）に一致することを確認。
  `*.job.movie.succeeded` は rate 24 で `completed_frames == 144`（6 秒）。
  sequence の `work_area` 保存値も `[1,7)` で照合。
- トランジション: `*.transition.wipe_pixels` は t=9/2 で左半分が B 色・右半分が A 色
  （reveal `min=[0,0]`/`max=[p*320,180]` の方向 left を厳密一致で確認）。
- 色補正: `*.color.exposure_pixels` は effect 適用前後の同一画素で
  linear RGB がちょうど 2 倍（exposure +1、`|after − 2×before| < 1e-3`）、alpha 不変。
  `*.color.effect_stored` は `sequence.query` の `kronello.color.exposure` version 1 /
  property 参照を照合。
- 字幕データ: `*.captions.round_trip` は `captions.export` の SRT が
  編集後 cue 本文・trim 後時刻 `00:00:04,250 --> 00:00:05,500`・CRLF 書式の期待 bytes と一致。
  `*.captions.vtt` は VTT の同内容を確認。`*.captions.plan` は plan の 7 command を検証。
- 字幕焼き込み: `*.captions.burned.*` は cue 区間内と区間外の `render.frame` linear 画素差が
  下部帯（row 159–172、bottom_center inset 1/20）に存在し、白 fill の近白画素が増えることを確認。
- 字幕サイドカー: `*.job.sidecar.content` は job 出力 `.srt` が `captions.export` の bytes と一致。
- 音声: `*.job.movie.streams` は ffprobe で video `prores` 320×180、audio `pcm_s24le` 48 kHz、
  `*.job.movie.audio_level` は ffmpeg `astats` の RMS ≈ −15.05 dB（tone440、> −40 dB を要求）。
  format duration はちょうど 6.000 秒。
- 固定 snapshot: 各 job は `result.validated == true` で公開され、
  `*.job.frames.pixel_match` は job 出力 rgba16f bytes が同一時刻の対話 `render.frame` と一致。
- CLI/MCP 同等性: `parity.sequence.query`（canonical JSON SHA-256 一致）、
  `parity.project.export`（revision 18 と document canonical bytes 一致）、
  `parity.frame.*`（3 probe 時刻の全 linear 画素一致）、`parity.captions.export`、
  `parity.job.frames.hashes`（24 frame の numeric sha256 全一致）、`parity.job.sidecar`。
  さらに `cli-movie.mov` と `mcp-movie.mov` の SHA-256 は `c2bab762…7263` で同一だった
  （driver 外の追加確認。MOV container まで決定的）。
- MCP 健全性: `mcp.scene.query:INVALID_REQUEST`（project 省略の型付き拒否）、
  `mcp.tools.list`（30 tools）、`mcp.clean_exit`。

## 環境・確認済みコマンド

| command | 結果 |
|---|---|
| `python3 scripts/fetch_fixtures.py --offline` | 0、`noto-sans-cjk-jp` の SHA-256 照合 |
| `cargo build -p kronello-cli -p kronello-mcp` | 0 |
| `python3 scripts/demo_integration_m7.py --binary-dir target/debug --output-directory /tmp/m7-demo-out` | 0、219 checks verified |
| `cargo fmt --all --check` | 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 |
| `python3 scripts/fixtures.py generate` | 0、9 素材を生成 / decode（`cargo test` の jobs fixture が要求） |
| `cargo test --workspace --locked` | 0、全 suite 合格（hardware 依存は理由付き ignored） |

`cargo test` の初回実行は生成 fixture 未作成で `nle2_video_generator_fixed_job_survives_…`
が `target/fixtures/generated/media/cfr-24-1.nut` 不在を `unwrap` で失敗した。
`fixtures.py generate` 後の再実行は全件合格であり、初回を合格には数えない。

FFmpeg / ffprobe / ffmpeg-astats は PATH の `/opt/homebrew/bin`（9.0.2）を検査用に使う。
job の encode は build 時に pkg-config で解決された `kronello-media` の MediaRuntime が担い、
配布構成の切替は行っていない。

## GUI 受け入れの再現手順（親エージェント向け）

本 lane では GUI を実行していない。以下は同じ共有 API を使う macOS アプリで
受け入れ条件 1 の GUI 経路を検証する手順である。
`gui-evidence.json` の全 ID と期待値を対照表として使う。

1. 上記 driver を実行し、`<output>/m7-integration.cli.kronello` と
   `<output>/gui-evidence.json` を得る。
2. macOS アプリ（`apps/macos` の Kronello.app）で CLI 生成の project を開く。
   revision が 18、sequence が 3 track（video / audio / caption）であることを確認する。
3. タイムラインで A `[0,5)` 赤・B `[4,7)` 緑・C `[7,10)` 青、tone クリップ `[0,10)`、
   caption cue `[0.5,2)` / `[4.25,5.5)` / `[8,9.5)`、A→B の wipe `[4,5)`、
   sequence marker 3 秒、clip marker、work area `[1,7)` が表示されることを確認する。
4. Viewer で t≈1（cue1「編集済みの最初の字幕」が A 上に表示）、t≈4.5（wipe 中間で
   左緑・右赤 + cue2）、t≈8.75（cue3 が明るくなった C 上に表示）を再生・スクラブする。
   C の明度上昇は `kronello.color.exposure` +1 の結果であり、Inspector で effect と
   exposure 値 1.0 を確認する。
5. GUI の編集操作（trim / split / marker / work area / caption 本文 / effect パラメータ）を
   同一 project 上で行い、Undo で元に戻ることを確認する。GUI-008 の編集 UI が未提供の
   操作は、表示対照のみを GUI 経路の確認範囲とし、編集の同等性は共有 API 経路の
   parity で担保する。
6. GUI の書き出し（image sequence または movie）で新規 directory に `[1,7)` の成果物を出し、
   frame 数・PNG 寸法・codec・字幕サイドカーが driver と同じ判定を満たすことを確認する。
7. GUI 上の project を export し、`project.export` の canonical bytes が
   `<output>/project.export.json` の document と一致することを確認する
   （GUI が新規編集を加えた場合は編集前の revision 18 で比較する）。

GUI 操作は実画面の Computer Use で実行し、失敗は型付きエラーとともに記録する。
本 lane の成果物（driver・manifest・手順）は GUI 実行に必要な全 ID を提供する。

## 未解決・設計上の注意

- `capabilities.get` の `effects` 列挙に `kronello.color.*` が含まれない。
  feature flags には `clip_effects` / `captions_v1` / `audio_generator_v1` 等があり、
  driver は feature flag と model の effect descriptor 定数を直接使う。
  effect registry と capabilities の一致は GUI-008 / COLOR-002 の受け入れ側で扱う。
- `--backend cpu-reference` では字幕を含む frame が debug build で約 43 秒/枚となる
  （glyph raster 経路が未最適化）。字幕なし frame は約 1.6 秒。canonical run は `--backend gpu` とし、
  CI の opt-in small CPU test は今回追加していない。
- MCP 経路の `render.submit` は `kronello-mcp` が `worker --job` 子プロセスを spawn する。
  job は投入時 revision の frozen snapshot を使い、投入後の編集は出力に反映されない
  （`result.validated` で snapshot hash が検証される）。
- movie 出力は `pro_res_mov`（ProRes 422 + PCM24）。H.264 / AAC などの他 profile は
  本デモの検証範囲外。
- `kronello.audio.tone440` は約 0.25 振幅の 440 Hz sine を生成し、外部音声素材を使わない。
  実素材ファイルを使う音声経路は MEDIA-003 等の検証範囲。
- caption track に transition を置けない・work area は content extent 内の非空区間、
  など sequence validation による型付き拒否は既存の NLE / SUB 系検証が担う。
- `INTEGRATION-003` の依存（NLE-003 / COLOR-002 / FX-003 / GUI-008）は本 branch 時点で
  backlog status が `planned` のまま。本 lane は driver と証拠・手順の提供までを範囲とし、
  status 更新は GUI 受け入れを統括する親エージェントの判断に委ねる。
