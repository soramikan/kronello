# INTEGRATION-005 の検証

対象: `soramikan/kronello` `m8` 統合ブランチ、2026-10-09、Darwin arm64 / Rust 1.95.0 / FFmpeg 9.0.2。
実 `kronello` / `kronello-mcp` debug binary を `--backend cpu-reference` と `--backend gpu` の両方で実行し、
driver の 77 checks が両 backend で全件合格した（requests 各 641）。

## 受け入れ条件との対応

| 条件 | driver の check / 証拠 | 結果 |
|---|---|---|
| 1. 可変リタイム・freeze・マスク・アジャストメントクリップ・LUT・音声エフェクト・プロキシ切替を含む作品を CLI と MCP の両経路で生成する | `cli.*` / `mcp.*` それぞれ同一の公開 Command を同一引数で適用し revision 23 の作品を生成。`parity.sequence.query`（canonical JSON 一致）、`parity.project.export`（revision と、locator/job を正規化した document canonical bytes 一致）、`parity.frame.{2,8,10}`（全 linear 画素一致）、`parity.job.frames.hashes`（24 frame の numeric sha256 全一致） | 合格（両 backend） |
| 2. 書き出し結果の映像・音声・型付き拒否を受け入れ基準として検証する | 映像: `*.job.frames.*`（24 frames・manifest range `[1,7)`・時刻列）と `*.job.movie.*`（`completed_frames == 144`、ffprobe で prores 320×180 / 6.000 s）。音声: `*.job.movie.streams`（pcm_s24le 48 kHz 2ch）、`*.job.movie.audio_level`（ffmpeg astats RMS ≈ −25.3 dB）。型付き拒否: `clip.place:INVALID_CLIP`（audio track への adjustment clip）、`clip.place:TRACK_LOCKED`、`render.frame:LUT_INPUT_MISSING`、`render.submit:UNSUPPORTED_FEATURE`（preview proxy mode の書き出し）、`mcp.scene.query:INVALID_REQUEST` | 合格（両 backend） |

## 再現 driver と入力

`scripts/demo_integration_m8.py` は `demo_integration_m2.py` の `Demo`（実 subprocess の
CLI / stdio MCP、isolated `KRONELLO_STATE_ROOT`、request 記録、check 集約）と
`demo_integration_m7.py` の `Channel` を継承し、`demo_integration_m3.py` の
canonical JSON 比較を使う。全 ID は `uuid.uuid5` の固定 namespace から導出する。

作品は `320×180`・24 fps・48 kHz・`linear_rec709` の sequence に video 4 track /
audio 1 track。実メディア `source.mov`（testsrc2 320×180 mpeg4、extent と同寸）は
`ffprobe` で実測した stream metadata（codec / time_base / duration_ts / 寸法 /
pixel_format）を資産メタデータとして authoring し、プロキシ検証が要求する
duration rational と一致させる。

| 要素 | 公開操作 | 内容 |
|---|---|---|
| 実メディア配置 | `clip.place` | V1 に source.mov `[0,6)`・`[6,12)`、V2 に青 solid `[0,12)`、A1 に tone440 `[0,12)` |
| freeze / 可変リタイム | `clip_freeze` / `clip_time_set` | V1 clip を t=3 で freeze（右半は piecewise hold）。V1 後半 clip に `piecewise_linear` の ramp-hold-ramp（parent 0→2 加速・2→4 保持・4→6 再加速） |
| トラック制御 | `track_state_set` / `sequence_targets_set` / `clip_enable_set` | V1 ロック → `clip.place` が `TRACK_LOCKED` で拒否 → 解除。targets を video=V1 / audio=A1 に永続化。mask clip の disable/enable で合成からの除去・復帰を画素で確認 |
| マスク | `clip_masks_set` | V2 青 solid に左半分のベジェ mask（add・closed）。`*.mask.pixels` は左が青色・右が下位 video で厳密に相違 |
| アジャストメントクリップ | `clip.place`（`source_ref.kind = adjustment`） | V3 `[6,12)` に `kronello.color.exposure` −1 EV。`*.adjustment.exposure` は同一画素の linear RGB がちょうど 0.5 倍。audio track への配置は `INVALID_CLIP` で拒否 |
| LUT | `lut.import` / `clip_set_effects` | `.cube` SIZE 2 の反転格子を `AssetKind::Data` として import（SHA-256 記録）。V4 `[10,12)` の magenta solid に `kronello.color.lut` を接続し `*.lut.pixels` で期待色と照合。lattice 未供給の render は `LUT_INPUT_MISSING` |
| 音声 | `clip_set_effects` / `audio.loudness` / `audio.normalize` | tone に `kronello.audio.eq`（peak −6 dB @1 kHz）+ `hpf`（200 Hz・order 2）+ `limiter`（−1 dB ceiling）。loudness 測定 −14.32 LUFS → normalize で −23.0 LUFS に収束（再測定 −23.0000002） |
| トラッキング | `track.analyze` | points mode・seed 1 件・range 4 frame で `tracking_data_assets` に決定的データ資産を永続化 |
| プロキシ | `proxy.generate` / `proxy.status` / `render.frame`（`media_proxies: prefer`） | scale 0.5 の fixed-input job が成功し link が ready。preview 入力では proxy が使われ、preview mode の `render.submit` は `UNSUPPORTED_FEATURE` で拒否 |
| 書き出し | `render.submit`（image_sequence / pro_res_mov） | work area `[1,7)` で 24 PNG フレームと 144 フレーム ProRes+PCM24 MOV |

## 実行記録

```sh
cargo build -p kronello-cli -p kronello-mcp --locked
python3 scripts/demo_integration_m8.py \
  --output-directory target/m8-acceptance/integration-005 --backend cpu-reference
python3 scripts/demo_integration_m8.py \
  --output-directory target/m8-acceptance/integration-005-gpu --backend gpu
```

結果: 両実行とも `{"status": "verified", "checks": 77}`、失敗 0、requests 641。
出力は `target/m8-acceptance/integration-005{,-gpu}/` に
`m8-integration.{cli,mcp}.kronello`（各 revision 23）、`report.json`、
`gui-evidence.json`、`project.export.json`、`{cli,mcp}-frames`（24 PNG + manifest）、
`{cli,mcp}-movie.mov`（prores 320×180 / pcm_s24le 48 kHz / 6.000 s）、
`{cli,mcp}-media`（source.mov・invert.cube）、`*.proxies/`（生成 proxy MOV）、
隔離 `state/` を保存した。

### 検証で発見・修正した不具合（本デモが初めて実経路を通した箇所）

- `crates/kronello-media/native/media.c`: エンコーダが packet duration を常に
  1 tick で書いていたため、stream time_base がフレーム間隔より細かい実コンテナ
  （1/12288 の mpeg4 mov 等）でプロキシの公開 duration が `last_pts + 1` に潰れ、
  `proxy.generate` の duration 検証が `OUTPUT_VALIDATION_FAILED` で失敗した。
  `km_encoder_frame` に duration 引数を追加し、caller が実フレーム長を渡すよう変更。
  `encode_video_stream` 側も 1 フレーム先読みで隣接 PTS 差を書く（終端は直前間隔を継承）。
  回帰テスト `encode_proxy_preserves_source_frame_spans` を `tests/proxy.rs` に追加。
- `crates/kronello-service/src/loudness.rs`: `audio.normalize` が gain プロパティ ID を
  `Uuid::new_v4()` で発行し CLI / MCP の document parity を破った。
  clip ID + プロパティ数からの UUID v8 決定的導出に変更（split/freeze の
  owned-ID 導出と同じ規約）。
- `crates/kronello-service/src/proxy.rs`: `proxy.generate` の proxy asset ID も
  `Uuid::new_v4()` で、asset / link / destination 名が両経路で分岐した。
  (asset, stream, scale, 寸法, revision) からの UUID v8 導出に変更。

## 環境・確認済みコマンド

| command | 結果 |
|---|---|
| `cargo build -p kronello-cli -p kronello-mcp --locked` | 0 |
| `python3 scripts/demo_integration_m8.py --output-directory target/m8-acceptance/integration-005 --backend cpu-reference` | 0、77 checks verified |
| `python3 scripts/demo_integration_m8.py --output-directory target/m8-acceptance/integration-005-gpu --backend gpu` | 0、77 checks verified |
| `cargo test -p kronello-media --locked` | 0、11 suite 全件合格 |
| `cargo test -p kronello-service --locked --test proxy_tracking --test loudness --test color003` | 0、全件合格 |
| `cargo fmt --all --check` / `cargo clippy -p kronello-media -p kronello-service --all-targets --locked -- -D warnings` | 0 |

FFmpeg / ffprobe / astats は PATH の `/opt/homebrew/bin`（9.0.2）を検査用に使う。
parity 比較は作品 semantics の一致を扱うため、media locator はファイル名へ、
`proxies[].job` は除外して正規化する（両 channel が別プロジェクト・別素材で
構築する設計上、パスと job ID は一致し得ない）。
