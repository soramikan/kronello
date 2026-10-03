# RENDER-001 の検証

対象: `kronello-render` の snapshot → 純粋 Scene IR / Render DAG → 明示 backend → still / image sequence。作業開始時の HEAD は `bd0a9e3cfd01c2ed706bfb98e606922275cecc31`（branch `m1-motion-core`）。GPU-002 の既存 API / oracle を使い、store への依存は追加しない。

## 受け入れ条件とテスト

| 条件 | `crates/kronello-render/tests/render.rs` の再現テスト |
|---|---|
| 有理数時刻の形状＋日本語 text を描画、順序・領域・解像度に依存して文書を変更しない | `animated_shape_japanese_text_random_access_and_resolution_preserve_document`。固定 Noto Sans CJK JP の「日本語」、animated rectangle、0 / 1/3 / 2/3 / 1 秒。順・逆・シャッフル要求、ROI の crop 一致、倍解像度・非一様出力、snapshot 後の文書編集からの独立性 |
| Scene IR → 明示 DAG、隔離 Group、matte、配置、半開時間 | `dag_is_topological_group_opacity_once_and_matte_is_not_displayed`、`nested_instance_paths_active_range_and_transform_parent_are_preserved`。解析値・input index 順序・mask 非表示・循環拒否・別 InstancePath・world transform を照合 |
| 有理数範囲・fps → 連番、負時刻、端点・overflow | `negative_fractional_range_end_exclusion_empty_and_overflow`、`sequence_rational_grid_files_png16_raw_truth_metadata_and_nonoverwrite`。30000/1001 fps の絶対格子・半開 end・ordinal と絶対 index の区別 |
| 色 / alpha・pixel format・mapping・snapshot hash・意味の版を metadata に記録 | `sequence_rational_grid_files_png16_raw_truth_metadata_and_nonoverwrite`、`snapshot_versions_hash_locks_profile_and_independent_unknown_content`。JSON の各契約、PNG を decode して全 sample、raw binary16 bytes、ファイル hash、manifest round-trip を照合 |
| 未対応・欠落を黙って続行しない、原本・既存成果物を保全 | `expression_missing_font_glyph_hash_and_failed_sequence_are_typed`、`round_stroke_unsupported_style_nonuniform_transform_and_zero_scale`、`later_frame_failure_rolls_back_staged_files_and_hdr_numeric_truth_is_unclipped`。Expression、missing font / glyph、font hash、style / transform、二枚目 backend failure、rollback / 非上書きを確認 |
| 数値正本を SDR 表示 clipping で代用しない | `invalid_regions_pixels_and_numeric_rounding_are_rejected_or_explicit`、`later_frame_failure_rolls_back_staged_files_and_hdr_numeric_truth_is_unclipped`。非有限・alpha・binary16 範囲違反の拒否、alpha round-to-zero、負 RGB / 1 超の raw 保持と PNG 限定 clipping |
| GPU 実行と CPU oracle の一致 | host の `gpu_animated_shape_and_japanese_text_match_cpu_all_pixels_and_order`、`gpu_isolated_alpha_and_luma_mattes_and_sequence_match_reference`。全画素・edge を比較、GPU 逆順一致、ROI、alpha / luminance matte、GPU 連番 raw を確認 |

通常 CPU テストは **10**、GPU adapter 必須テストは `gpu_` prefix の **2**。GPU は adapter 不在で fail し、ignored / skip / fallback を実装しない。sandbox では `--skip gpu_` を明示し、実行可能な CPU テストだけを数える。固定環境 golden の baseline 比較とは別の契約テストである。

CPU oracle は binary16 中間丸めを行わない。GPU 比較は全画素で線形 RGB / alpha を `2^-10 × max(1, |expected|)` 以下、外部 straight sRGB を `2^-9 × max(1, |expected|)` 以下とし、finite / alpha 範囲 / zero alpha RGB も別途確認する。外部 sRGB の閾値は中間 binary16 の量子化と非線形 encode の比較用で、固定環境 golden の閾値変更ではない。

## 再現コマンド

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p kronello-render --locked -- --skip gpu_
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
python3 scripts/backlog.py check
```

GPU host（Apple M1 / Metal）:

```sh
WGPU_BACKEND=metal cargo test -p kronello-render -p kronello-gpu -p kronello-framebridge --locked --no-fail-fast -- --nocapture
```

依存追加後の最初の `cargo check -p kronello-render -p kronello-gpu` と `cargo clippy --workspace --all-targets -- -D warnings` は lock 更新を許して実行した。以降は `--locked` を使う。

## 実行結果

worker が sandbox で実行した結果:

- `cargo fmt --all --check`: 成功。
- `cargo clippy --workspace --all-targets -- -D warnings`（lock 更新後の初回）、続く `cargo clippy --workspace --all-targets --locked -- -D warnings`: 成功。
- `cargo test -p kronello-render --locked -- --skip gpu_`: **10 passed、0 failed、GPU 2 filtered out**（31.50 秒）。
- `cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_`: **241 passed（compile-fail doctest 2 を含む）、0 failed、0 ignored、GPU 2 filtered out**。最新の render suite は **10 passed**（32.60 秒）。
- `python3 scripts/backlog.py render` / `python3 scripts/backlog.py check`: 成功（51 tasks）。

supervisor が host で実行して返した結果（worker による GPU 実測ではない）:

- Apple M1 / IntegratedGpu / Metal / macOS 27.0（build 26A428）。作業開始 HEAD `bd0a9e3cfd01c2ed706bfb98e606922275cecc31` と RENDER-001 の未コミット実装・テスト差分を対象に、上記 host コマンドを実行。
- `kronello-render` の `tests/render.rs`: **12 passed、0 failed**（32.8 秒）。新規 GPU 2 テスト、全画素 oracle 比較、逆順、matte、GPU 連番を含む。
- `kronello-gpu`: contracts **13 passed**、scene **15 passed**、固定環境 golden **1 ignored**。
- `kronello-framebridge`: unit **1 passed**、paths **3 passed**、videotoolbox **1 passed、2 ignored**（既存 optional test）。
- 全対象 **45 passed、0 failed、3 ignored**。ignored test の成功・固定環境 golden の合格は主張しない。

この確認後に RENDER-001 を `done` とし、backlog を再生成した。GPU の固定環境 baseline 登録・M4 golden、Windows / Linux GPU、新 renderer の性能、プロセス強制終了時の復旧は未検証。
