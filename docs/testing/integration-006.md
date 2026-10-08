# INTEGRATION-006 の検証

対象: `soramikan/kronello` `m9` 統合ブランチ、2026-10-09、Darwin arm64 / Rust 1.95.0 / FFmpeg 9.0.2。
実 `kronello` / `kronello-mcp` debug binary を `--backend cpu-reference` と `--backend gpu` の両方で実行し、
driver の 56 checks が両 backend で全件合格した（requests 各 1045）。

## 受け入れ条件との対応

| 条件 | driver の check / 証拠 | 結果 |
|---|---|---|
| シーン検出 → マーカー/分割、マルチカム、二画面モニタの insert/overwrite、スタビライズ、書き出しバッチを CLI と MCP の両経路で検証する | `cli.*` / `mcp.*` それぞれ同一の公開 Command を同一引数で適用し revision 22 の作品を生成。`scene.job`（fixed-input job 成功）→ `scene.asset`（`SceneBoundaryAsset` がドキュメントへコミット）→ `scene.cut`（4 s の確定的カット検出）→ `scene.markers` / `scene.split`（`scene.apply` の両モード）。`multicam.create`（manual sync・2 angle）+ `multicam.angle_switch`（A↔B の画素差）。`edit.insert`（境界 insert で ripple）/ `edit.overwrite`（中間 overwrite で split_tail）。`stabilize.render`（`track.analyze` → `kronello.stabilize` 適用 → フレーム出力）。`export.batch`（preset + inline submission の混在 batch） | 合格（両 backend） |
| プロジェクト書き出し・レンダー出力・型付き拒否を CLI/MCP で一致させる | `parity.sequence.query`（canonical JSON 一致）、`parity.project.export`（revision と、locator/job を正規化した document canonical bytes 一致）、`parity.frame.{1,3,11}`（全 linear 画素一致）。型付き拒否: `multicam.create:MULTICAM_SYNC_FAILED`（映像のみの angle に audio sync）、`clip.angle_switch:SOURCE_MISSING`（未登録 angle）、`render.frame:TRACKING_DATA_STALE`（未解析 angle への切替後のスタビライズ）、`edit.insert:TRACK_LOCKED`（ロック済みトラック）、`mcp.scene.query:INVALID_REQUEST` | 合格（両 backend） |

## 再現 driver と入力

`scripts/demo_integration_m9.py` は `demo_integration_m2.py` の `Demo`（実 subprocess の
CLI / stdio MCP、isolated `KRONELLO_STATE_ROOT`、request 記録、check 集約）と
`demo_integration_m7.py` の `Channel` を継承し、`demo_integration_m3.py` の
canonical JSON 比較を使う。全 ID は `uuid.uuid5` の固定 namespace から導出する。
MCP 経路では `Demo.tool` の `name` 引数と `multicam.create` の `name` フィールドが
衝突するため、`M9Channel.call` が `tools/call` を直接組み立てて同一の
text/structuredContent 一致・isError 検査を行う。

作品は `320×180`・24 fps・48 kHz・`linear_rec709` の sequence に video 2 track /
audio 1 track。素材は `ffprobe` で実測した stream metadata を資産メタデータとして
authoring する。`angle-a.mov` は testsrc2 4 s + 単色 4 s の concat（4 s にハードカット、
シーン検出の確定的入力）、`angle-b.mov` は smptebars 8 s、`tone.mov` は
440 Hz sine の 5.1ch pcm_s24le 8 s（`pcm_s24le`・`-ac 6`）。`scene.detect` の
submit 時フレーム上限は `stream.time_base` から推定するため、映像素材は
`-video_track_timescale 24` に固定してある。

| 要素 | 公開操作 | 内容 |
|---|---|---|
| マルチカム | `multicam.create` / `clip.place` / `clip.angle_switch` | manual sync・offset 0 の 2 angle グループを作成し、`source_ref.kind = multicam` のクリップを V1 `[0,8)` に配置。angle 切替で画素が変わることを確認。video-only angle への `sync: audio` は `MULTICAM_SYNC_FAILED` で拒否 |
| スタビライズ | `track.analyze` / `clip_set_effects` | angle A の stream を `[0,8)` で解析し `kronello.stabilize` を接続（smoothing 1・max_displacement 64・max_crop 0.25）。angle B へ切替えると `TRACKING_DATA_STALE` で拒否、A へ戻すと描画成功 |
| シーン検出 | `scene.detect` / `scene.apply` | fixed-input job が `SceneBoundaryAsset`（`scene_boundary_v1`）をドキュメントへコミット。markers モードでシーケンスマーカーへ、split モードで V2 クリップを 4 s の境界で分割 |
| フレーム補間 | `clip.place`（`time_map.piecewise_linear` + `interpolation.mode = optical_flow`） | V2 `[10,14)` に 0.25 倍速区間を持つクリップを配置し、local 0.3125 s（24 fps フレーム間）の時刻で `render.frame` が光フロー合成を通して出力 |
| 音声リタイム・マルチch | `clip.place`（`audio_retime = pitch_preserve_v1`） / `render.submit`（`audio_layout = 1551`） | 5.1ch 実音声を A1 `[0,4)` に 2 倍速で配置（8 s 分消費・ピッチ保持）。書き出しは `document` 音声を 5.1ch pcm_s24le で出力 |
| insert/overwrite | `edit.insert` / `edit.overwrite` / `track_state_set` | V2 のクリップ境界へ insert（後続が ripple で 1 s 移動）→ 中間 overwrite で対象クリップを head/tail 分割し `[6,8)` を置換。ロック済みトラックへの insert は `TRACK_LOCKED` で拒否 |
| メディア管理 | `bin_create` / `bin_assign` / `media.query` / `asset.thumbnail` | bin の作成と資産割当がドキュメントに永続化され、両経路で availability `present_unverified` とサムネイル画素を返す |
| 書き出し | `export_preset_save` / `export.batch` / `render.submit` | ドキュメント保存 preset（pro_res_mov・5.1ch）と inline image_sequence submission を混在 batch に投入し両方 `submitted`。同一 `idempotency_key` の再投入は `replayed`。MOV は ffprobe で prores + pcm_s24le 48 kHz 6ch を確認 |

## 実行記録

```sh
cargo build -p kronello-cli -p kronello-mcp --locked
python3 scripts/demo_integration_m9.py \
  --output-directory target/m9-acceptance/integration-006 --backend cpu-reference
python3 scripts/demo_integration_m9.py \
  --output-directory target/m9-acceptance/integration-006-gpu --backend gpu
```

結果: 両実行とも `{"status": "verified", "checks": 56}`、失敗 0、requests 1045。
出力は `target/m9-acceptance/integration-006{,-gpu}/` に
`m9-integration.{cli,mcp}.kronello`（各 revision 22）、`report.json`、
`project.export.json`、`{cli,mcp}-frames`（16 PNG + manifest）、
`{cli,mcp}-movie.mov`（prores 320×180 / pcm_s24le 48 kHz 6ch / 4.000 s）、
`{cli,mcp}-media`（angle-a/b.mov・tone.mov・scene-boundary 資産）、
隔離 `state/` を保存した。

### 検証で発見・修正した不具合（本デモが初めて実経路を通した箇所）

- `crates/kronello-model/src/export_presets.rs`: `ExportOutput` は内部タグ
  付き enum のため serde がフィールドを Content としてバッファし、
  arbitrary_precision の数値が private map 表現のまま `background: [f32;3]` /
  `ExportAudioClip.gain: f32` へ渡され、`invalid type: map, expected f32` で
  `export_preset_save` の movie 系 preset が一切デコードできなかった。
  `SourceRef`（model）と `JobOutput`（service `wire.rs`）と同じく、
  variant payload を RawValue 経由で `serde_json::from_str` する手動
  `Deserialize` に変更。回帰テスト
  `export_preset_save_decodes_f32_fields_from_wire_json` を
  `crates/kronello-service/tests/flow003.rs` に追加。

## 環境・確認済みコマンド

| command | 結果 |
|---|---|
| `cargo build -p kronello-cli -p kronello-mcp --locked` | 0 |
| `python3 scripts/demo_integration_m9.py --output-directory target/m9-acceptance/integration-006 --backend cpu-reference` | 0、56 checks verified |
| `python3 scripts/demo_integration_m9.py --output-directory target/m9-acceptance/integration-006-gpu --backend gpu` | 0、56 checks verified |
| `cargo test -p kronello-service --locked --test flow003` | 0、4 件全件合格 |
| `cargo fmt --all --check` / `cargo clippy -p kronello-model -p kronello-service --all-targets --locked -- -D warnings` | 0 |

FFmpeg / ffprobe は PATH の `/opt/homebrew/bin`（9.0.2）を検査用に使う。
parity 比較は作品 semantics の一致を扱うため、media locator はファイル名へ、
`proxies[].job` は除外して正規化する（両 channel が別プロジェクト・別素材で
構築する設計上、パスと job ID は一致し得ない）。
