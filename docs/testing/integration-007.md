# INTEGRATION-007 の検証

対象: `soramikan/kronello` `m10` 統合ブランチ、2026-10-09、Darwin arm64 / Rust 1.95.0 / FFmpeg 9.0.2。
実 `kronello` / `kronello-mcp` debug binary を `--backend cpu-reference` と `--backend gpu` の両方で実行し、
driver の 55 checks が両 backend で全件合格した。

## 受け入れ条件との対応

| 条件 | driver の check / 証拠 | 結果 |
|---|---|---|
| チャプター付きマルチ出力書き出し、新エフェクトのタイムライン適用、RAW インジェスト、capture 経路、外部モニタ出力切替を CLI と MCP の両経路で検証する | `cli.*` / `mcp.*` それぞれ同一の公開 Command を同一引数で適用し revision 10 の作品を生成。`delivery.job`（multi-output `render.submit` 成功）→ `delivery.chapters`（MOV に `Intro` / `後半` の chapter track・ffprobe 確認）→ `delivery.{movie-dnx.mov,movie.gif,sound.mp3,sound.flac}`（各 leg の codec を ffprobe で確認）→ `delivery.chapters_dropped`（chapter 非対応 leg に `CHAPTERS_DROPPED` 警告）。`fx008.pixels` / `fx008.deterministic`（grain/mosaic/invert が画素を変更し決定的）、`fx008.gate`（`audio.gate` で tone が無音化・`audio.delay` が audible chain として elementary leg を駆動）。`raw.dng`（LibRaw DNG が `asset.thumbnail` で実画素を返す）、`io.output.list`（ref_monitor/syphon/sdi/ndi の 4 kind を列挙）、`capture.submitted` / `capture.stop` / `capture.job` / `capture.status`（synthetic capture の記録・停止・`asset_registered` 確認） | 合格（両 backend） |
| プロジェクト export・レンダー出力・型付き拒否の CLI/MCP parity を確認する | `parity.sequence.query`（canonical JSON 一致）、`parity.project.export`（revision 10 と、locator/job/記録長を正規化した document canonical bytes 一致）、`parity.frame.{1,3}`（linear 画素一致）。型付き拒否: `io.output.{enable,disable}:UNSUPPORTED_FEATURE`（headless での活性要求）、`capture.deck_probe:UNSUPPORTED_FEATURE`（vendor adapter 未リンクの deck 境界）、`asset.thumbnail:UNSUPPORTED_FEATURE` ×2（BRAW/R3D の vendor-SDK 境界）、`mcp.scene.query:INVALID_REQUEST` | 合格（両 backend） |

## 再現 driver と入力

`scripts/demo_integration_m10.py` は `demo_integration_m2.py` の `Demo`（実 subprocess の
CLI / stdio MCP、isolated `KRONELLO_STATE_ROOT`、request 記録、check 集約）と
`demo_integration_m7.py` の `Channel` を継承し、`demo_integration_m3.py` の
canonical JSON 比較と `demo_integration_m9.py` の `parity_document` を使う。
全 ID は `uuid.uuid5` の固定 namespace から導出する。`io.output.enable` の
request フィールド `name` が `Demo.tool` の引数と衝突するため、
`M10Channel.call` が `tools/call` を直接組み立てる。

作品は `320×180`・24 fps・48 kHz・`linear_rec709` の sequence に video 1 track /
audio 1 track。素材は `ffprobe` で実測した stream metadata を資産メタデータとして
authoring する。`source.mov` は testsrc2 4 s、`-video_track_timescale 24` 固定、
`tone.mov` は 440 Hz sine の stereo pcm_s24le 8 s。RAW fixture は synthetic DNG
（LibRaw 経路）と BRAW/R3D の拡張子スタブ（vendor 境界検証用）。

| 要素 | 公開操作 | 内容 |
|---|---|---|
| チャプター・マルチ出力 | `marker_add` / `render.submit`（`outputs` legs） | シーケンスマーカー 2 件を `[0,4)` の `pro_res_mov`（`profile_version: 3`・document audio）へ転送し、DNxHR-MOV/GIF/MP3/FLAC の追加 leg を同一レンダーで fan-out。chapter 非対応 leg は `CHAPTERS_DROPPED` 警告を記録し、MOV は ffprobe で chapter track（`Intro`/`後半`）を確認 |
| 新エフェクト | `clip_set_effects` / `render.frame` / `audio.loudness` | `kronello.grain`（film・seed 固定）+ `kronello.mosaic` + `kronello.invert` の versioned スタックが画素を変更し決定的。書き出し前に SDR ガムット内の `mosaic` + `invert` + `kronello.tint` スタックへ置換（grain は SDR エンコードの型付きガムット検査を超過するため）。音声側は `audio.gate`（tone を無音化して LUFS 消失を確認）+ `audio.delay`（audible chain） |
| カメラ RAW | `asset.register` / `asset.thumbnail` | synthetic DNG が LibRaw 経路で 64×48 RGBA を返す。BRAW/R3D スタブは `UNSUPPORTED_FEATURE` の vendor-SDK 境界 |
| 外部モニタ出力 | `io.output.list` / `io.output.enable` / `io.output.disable` | 検出と活性の分離: list が 4 device kind を返し、headless の enable/disable は `UNSUPPORTED_FEATURE` で型付き拒否（ADR-0134） |
| キャプチャ | `capture.start` / `capture.stop` / `capture.status` / `capture.deck_probe` | `max_frames` 上限付き synthetic session を投入し、進行フレームを `job.get` で観測してから stop。partial recording が `<asset-id>.mov` として公開・登録され、`capture.status` が `asset_registered: true` を返す。deck probe は vendor adapter 未リンクで `UNSUPPORTED_FEATURE` |

## 実行記録

```sh
cargo build -p kronello-cli -p kronello-mcp --locked
python3 scripts/demo_integration_m10.py \
  --output-directory target/m10-acceptance/integration-007 --backend cpu-reference
python3 scripts/demo_integration_m10.py \
  --output-directory target/m10-acceptance/integration-007-gpu --backend gpu
```

結果: 両実行とも `{"status": "verified", "checks": 55}`、失敗 0。
出力は `target/m10-acceptance/integration-007{,-gpu}/` に
`m10-integration.{cli,mcp}.kronello`（各 revision 10）、`report.json`、
`project.export.json`、`gui-evidence.json`、
`{cli,mcp}-delivery/`（movie.mov / movie-dnx.mov / movie.gif / sound.mp3 / sound.flac）、
`{cli,mcp}-media`（source.mov・tone.mov・DNG/BRAW/R3D スタブ）、
`m10-integration.{cli,mcp}.capture/`（公開済み partial recording）、
隔離 `state/` を保存した。

## parity の正規化

`parity_document`（locator をファイル名へ・proxy job id を除去）に加えて、
中途停止した capture の記録長は環境依存のため `parity_query` /
`parity_capture` が `asset_status.size_bytes`・`assets[].content_hash`・
`streams[].duration` を capture asset のみ無効化する。登録・availability・
job 状態の parity はそのまま比較する。

## 検証で発見・修正した不具合・決定

- 書き出し leg の FX-008 スタック: grain は SDR ガムット検査
  （`bt709_rgba` の `SDR export cannot encode out-of-gamut/HDR samples`）を
  超過するため、デモは frame 検証後にガムット内スタックへ置換する。
  型付き拒否そのものは ADR-0133 の意図どおり動作した。
- `io.output.*` / `capture.deck_probe` は projectless operation で、
  `project` フィールドは `deny_unknown_fields` で `INVALID_REQUEST`。
  driver は両経路とも project を付けない呼び出しに修正した。

## GUI 監査（Computer Use）

`target/macos/Kronello.app`（`d55fdbf` 時点ビルド・署名済み）を
open-computer-use で実機監査した。起動画面・最近のプロジェクト
（欠落パスは `PROJECT_NOT_FOUND` で型付き表示）、編集・モーション・
テンプレート・メディア・書き出しの全ページ、Source/Program 両モニタの
GPU プレビュー（CPU 参照バッジなし）、外部出力 UI、ジョブシート、
スコープパネル、ミキサー、マーカータブ、書き出しプリフライト
（`INVALID_MEDIA_INPUT` ×2 で書き出し不可を正しく表示）を確認した。

監査で発見・修正した不具合:

- GUI-012 で追加された永続化キー（`playbackScrub`・`mixerVisible`・
  `scopesVisible`・`editScale`・`editSnap`・`sourceTrack`）を持たない
  旧 `ui-state` / preferences JSON が `keyNotFound` でプロジェクト全体を
  開けなくしていた。合成 `Decodable` を `decodeIfPresent` + 既定値の
  手書き init に置き換え、旧ファイルの後方互換を回復
  （`ccd6bef`・回帰テスト `verifyLegacyStateDecoding` 追加）。
- `EditorModel.request` は全リクエストに `project` を注入するが、
  `io.output.*`・`job.*`・`inspect.scopes` は projectless operation で
  `deny_unknown_fields` に抵触し `DecodingError` になっていた。
  各呼び出しを `transport.call` 直送りに修正（`inspect.scopes` は
  `input.project` で RenderInput を満たす）し、ジョブ一覧は
  `project_id` でクライアント側フィルタに変更（`d55fdbf`）。
- 外部出力: `io.output.list` の列挙（ref_monitor / syphon / sdi / ndi）が
  popup に反映され、未検出の Syphon/SDI/NDI は disabled で表示。
  `ref_monitor` 有効化は単一ディスプレイ環境で
  「外部ディスプレイが見つかりません」の型付き拒否を確認。
- スコープ: 波形・ベクトルスコープ・ヒストグラム・RGB パレードが
  合成フレームの実ビンで描画されることを確認。
