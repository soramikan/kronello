# COLOR-002 versioned カラー補正エフェクトの検証

現在の状態（2026-10-07）: M7 Lane C の作業ブランチ `m7-lane-c` で実装・検証を完了した。コードのコミットは `ba4f429`、golden baseline の採用コミットは本書と同じ後続コミット。`docs/backlog/backlog.json` の状態更新はこの文書の範囲外。

## 契約

[ADR-0108](../adr/0108-versioned-color-correction-effects.md) に従う versioned エフェクト 4 件を追加した。effect id と意味版はそれぞれ `kronello.color.exposure` / `kronello.color.levels` / `kronello.color.curves` / `kronello.color.hsl` の version **1**。pixel kernel は `color002-pointwise-f16-v1` で、CPU oracle（`kronello_gpu::color::apply_color`）と WGSL（`crates/kronello-gpu/src/effect.wgsl`）が同じ f32 式を共有する。

- パラメータは `PropertyId` 参照で結び付け、`EffectDefinition::validate` / `resolve` が所有・型・単位・値域を検査する。exposure / offset / levels の out 値は有限 scalar、gamma は正の有限 scalar、`in_white > in_black`、curves は `x` / `y` 列を持つ data table で x 狭義単調・両列 `[0,1]`・最大 64 点。
- 演算は sequence の作業空間（`LinearRec709` / `LinearRec2020`）のシーン線形 premultiplied RGB に対し `rgb = rgb * 2^exposure + offset`、levels の sign-preserving pow、Fritsch–Carlson monotone cubic、HSL の hue / saturation / lightness を適用する。**alpha は変更しない**。負値・HDR は一切 clamp しない。
- 未知 id・未知版は `UNSUPPORTED_FEATURE`、パラメータ不正は `INVALID_INPUT` 系の型付きエラー。`Effect::Opaque` は未知エフェクトをそのまま保持する。エフェクト段の出力は binary16（RNE）の surface 規約で量子化される。
- snapshot の `semantic_versions.effects` に 4 id が version 1 で固定され、pin を超える authored version は評価時に `UNSUPPORTED_FEATURE`。

## 受け入れ条件との対応

| 条件 | テスト・手順 | 状態 |
|---|---|---|
| versioned id・descriptor・既定値・PropertyId 参照 | model `color002_effect_ids_are_distinct_versioned_and_described` | CPU 合格 |
| exposure の式と alpha / HDR 不変条件 | model `color002_exposure_resolves_within_bounds_and_preserves_alpha_contract`、render `color002_exposure_applies_pointwise_and_keeps_extended_range` | CPU 合格 |
| levels の `in_white > in_black`・正 gamma・範囲検査 | model `color002_levels_rejects_degenerate_ranges_at_resolution` | CPU 合格 |
| curves の table 形状・単調性・範囲・64 点上限制 | model `color002_curves_table_shape_and_monotonicity_are_structural` | CPU 合格 |
| HSL の degrees・型・乗算範囲 | model `color002_hsl_uses_degrees_and_bounded_multipliers` | CPU 合格 |
| CPU 点対点式・alpha 保持・extended range | GPU crate `cpu_color002_pointwise_effects_preserve_alpha_and_extended_range` | CPU 合格 |
| stack 順序・scene IR・DAG・キャッシュ同一性 | render `color002_effects_stack_in_authored_order`、`color002_scene_ir_resolves_clip_effects_and_dag_has_pointwise_nodes`、`fx_effect_animation_versions_and_cache_identity` | CPU 合格 |
| 未知版の型付き拒否 | render `color002_unsupported_effect_version_fails_at_evaluation` | CPU 合格 |
| CPU / GPU 一致（両作業空間） | GPU `gpu_color002_pointwise_effects_match_cpu_reference_in_both_spaces` | GPU 実測合格（下記） |
| schema 生成一致 | service `public_schemas_match_rust_generators` | CPU 合格 |
| GPU golden | 4 新規シーン（下記） | 採用・比較済み |

## 実行記録

検証環境: Mac16,10（Apple M4）、macOS 27.0.1、Metal、rustc 1.95.0（`rust-toolchain.toml`）、`Cargo.lock` 固定。この worktree では `python3 scripts/fixtures.py generate` で 9 個の software media fixture を `target/fixtures/generated` に生成した（未追跡の検証入力のみ）。

- `cargo test -p kronello-model --test color002`: exit 0、5 passed。
- `cargo test -p kronello-render --test color002`: exit 0、6 passed。
- `cargo test -p kronello-gpu --test scene` の COLOR-002 系: `cpu_color002_pointwise_effects_preserve_alpha_and_extended_range` / `gpu_color002_pointwise_effects_match_cpu_reference_in_both_spaces` を含め合格。GPU 実機で全 4 エフェクト × 3 入力（部分 alpha・HDR・負値・小 alpha）× 2 作業空間が CPU oracle と一致。
- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `cargo test --workspace --locked`: exit 0（ignored は GPU golden 等の既存 ignored のみ）。

## golden シーン

カタログに 4 シーンを追加し、共通 baseline を **49 シーン・49 comparison frames** に更新した（採用詳細は [FX-003](fx-003.md) と [golden 手順](golden-comparison.md) を参照）。

- `color002-exposure-rec709`: mid-alpha solid + exposure -0.75 / offset +0.04。中心画素の実測 `[0.2881, 0.1288, 0.0542, 0.875]` で alpha 保持を確認。
- `color002-levels-rec2020`: Rec.2020 作業空間で in `[0.1,0.9]` / gamma 1.8 / out `[-0.05,1.1]`。実測 RGB に負値 `-0.3193` を含み、clamp されないことを確認。
- `color002-curves-rec709`: control points `(0,0) (0.3,0.6) (0.7,0.75) (1,1)` の monotone cubic。
- `color002-hsl-rec709`: hue +72° / saturation 0.6 / lightness +0.04。

`tests/golden/scenes.json` の解析 contract に `color002-exposure-alpha`（operation `color-exposure`）を追加し、`kronello-testkit` の contract テストで式と alpha 保持を別計算で検証した（全 12 scene、合格）。

## golden 実行結果

1. UPDATE: `target/golden/run.color002-fx003.fxlroW`、revision `ba4f429` の clean tree。49 シーン全て `candidate; CPU oracle validated`、`candidate_may_be_adopted=true`。
2. 採用: `python3 scripts/golden_adopt.py` が全 hash・寸法・有限値・premultiplied 制約を検証し `status=adopted`。`scene_count=49`、candidate 計 536,289 bytes、fixture 合計 1,409,407 bytes（3 MiB 上限内）。
3. 比較: `target/golden/run.compare.won5KX` で `status=pass`、49 シーン・49 frame の mismatched_pixels 0。provenance に dirty baseline の status を含むのは採用直後の既知の状態であり、比較結果には影響しない。
4. 既存シーン `coverage-fill-stroke` の baseline は採用で 1 チャンネルだけ変わった（最大差 2^-11 = binary16 の 1 ulp）。`src/color.rs` の式整理に伴う丸め差で許容誤差 `2^-10` の範囲内。意味の変更ではないため再採用した（VEC-004 の事例と同じ扱い）。

残件: Linux Vulkan / Windows DX12 の環境別 baseline には新シーンが無い。各環境での候補生成・採用は別環境の明示実行で行う（[QA-004](qa-004.md) の手順と同じ）。
