# MEDIA-005 の検証

対象: カメラ RAW 系フォーマット（RAW スチル / CinemaDNG / ProRes RAW / BRAW / R3D）のデコードと色管理。設計の正本は [ADR-0136](../adr/0136-camera-raw-decoding.md)。

## 依存と build

- LibRaw **0.22.2**（LGPL-2.1 側）。`KRONELLO_LIBRAW_PREFIX` → `KRONELLO_FFMPEG_PREFIX` → pkg-config `libraw_r` / `libraw` の順に検出する（`crates/kronello-media/build.rs`）。見つからない build は全 RAW 経路が型付き `UNSUPPORTED_FEATURE` を返す。
- vendored manifest `scripts/native-dependencies.json` に libraw 0.22.2 の URL・SHA-256・configure（`--disable-openmp --disable-lcms --disable-jpeg --disable-zlib`）を固定。`scripts/build_ffmpeg_lgpl.py` の verify で共有ライブラリの実在と `libraw_version()` の一致を確認し、macOS 配布 inventory（`scripts/release_common.py` の `LIBRARIES`）に `libraw_r.25.dylib` を加えた。
- ProRes RAW は macOS の AVFoundation demux + VideoToolbox hardware decode のみ。FFmpeg の `prores_raw` decoder / BRAW・R3D への暗黙 fallback はしない。

本ホスト検証は Apple M4 / macOS、Homebrew LibRaw 0.22.2（pkg-config `libraw_r`、JPEG/zlib 有効の development 構成）で実施した。`VTIsHardwareDecodeSupported` は `aprn` / `aprh` とも true を返した。vendored prefix の完全 build 自体は本検証で再実行していない（manifest / verify / inventory の静的変更と unit test の範囲）。

## 受け入れ条件と証拠の対応

条件の正本は `docs/backlog/backlog.json` の MEDIA-005（「カメラ RAW 系フォーマットのデコードと色管理を実装する」）。実装範囲を挙動領域ごとに対応付けた完了記録を以下に示す。証拠は `cargo test -p kronello-media --test raw --locked`（8 tests passed）と workspace 全体の `cargo test --workspace --locked`（exit 0）。

| # | 挙動 | 確認内容と証拠 |
|---|---|---|
| 1 | 検出の排他性 | `detection_claims_camera_raw_formats_only` が `.dng` → `LibRawStill`、`.braw` → `Braw`、`.r3d` → `R3d`、QuickTime sample entry `aprn`/`aprh` → `ProResRaw`、`avc1` は非 RAW を確認。検出は generic decoder より先に走る |
| 2 | RAW スチル（LibRaw） | `raw_still_decode_is_deterministic` が testkit の非圧縮 Bayer DNG を 2 回 decode し pixel 完全一致、opaque・finite・非ゼロの scene-referred linear を確認。固定パイプライン（AHD / 16bit / `output_color=1` / linear gamma / カメラ WB / auto bright なし）は `native/raw.cpp` に固定 |
| 3 | RAW メタデータの pin | `raw_still_metadata_is_pinned` が `probe_raw_still` と `MediaRuntime::probe` で codec `dng`・`StreamKind::Other`・寸法・`bayer16` pixel format を確認。`raw_still_rejects_locked_metadata_mismatch` が locked 寸法不一致の `INVALID_MEDIA_INPUT` と `ASSET_HASH_MISMATCH` を確認 |
| 4 | 圧縮 DNG | `compressed_dng_rejection_follows_capabilities` が Compression=7（JPEG-in-DNG）を `libraw_capabilities()` の JPEG bit で分岐: vendored 構成（bit なし）では常に `UNSUPPORTED_FEATURE`、development LibRaw では decode 失敗が `DECODE_ERROR` / `UNSUPPORTED_FEATURE` に留まることを確認 |
| 5 | CinemaDNG シーケンス | `cinemadng_sequence_selects_and_verifies_frames` が `take0001..0003.dng` の manifest probe（3 members・duration 3/24・manifest hash）、rational 時刻での member 選択（seed 別 pixel の区別）、範囲外の `FRAME_NOT_FOUND`、補間の `UNSUPPORTED_FEATURE`、member 改変時の `ASSET_HASH_MISMATCH` を確認 |
| 6 | ProRes RAW | `prores_raw_is_content_verified_and_hardware_gated` が `MediaRuntime::probe` の codec `prores_raw` / `StreamKind::Video` / pixel format `rgba64h`、fixture 内偽装 sample の型付き失敗（`UNSUPPORTED_FEATURE` または `DECODE_ERROR`）、非 RAW codec container への locked codec の `INVALID_MEDIA_INPUT`、`open_video` が FFmpeg へ渡さない `UNSUPPORTED_FEATURE` を確認。本ホストは hardware decode 対応を報告したため admission check を通過した上での decode 失敗を確認済み |
| 7 | ベンダー境界 | `vendor_formats_are_typed_rejections_everywhere` が BRAW / R3D の `open_video`・`probe` 両入口で `UNSUPPORTED_FEATURE`、DNG の video 入口拒否を確認。proprietary SDK は導入していない |
| 8 | 共有 API 経路 | `decode_image_asset`（image.rs）の RAW still 分岐、`decode_video_image` / sequential render（render.rs）の `cinemadng` / `prores_raw(_hq)` codec dispatch、`MediaRuntime::{open_video,open_video_stream,probe}`（video.rs / export.rs）の RAW 検出、`asset.thumbnail`（service media.rs）の RAW video codec 分岐を workspace テストで確認 |

## 再現コマンド

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p kronello-media --test raw --locked
cargo test --workspace --locked
python3 scripts/backlog.py check
python3 scripts/backlog.py render
git diff --check
```

## 残件・範囲外

- BRAW / R3D の実 decode は proprietary SDK（Blackmagic / RED）が前提であり本タスクの範囲外。SDK 導入までは型付き拒否が仕様。
- ProRes RAW の実フレーム decode 画値は hardware 対応機 + 実素材が必要。本ホストは hardware を報告するが、合成 fixture の sample は偽装のため実 pixel 検証は未実施。
- vendored prefix の完全再 build（`build_ffmpeg_lgpl.py`）は時間がかかるため本検証では未実施。manifest の libraw entry と verify 強化は済んでおり、次回 vendored build 実行時に検証される。
- Linux / Windows は LibRaw stills と CinemaDNG のみ（ProRes RAW は macOS 限定で `UNSUPPORTED_FEATURE`）。
