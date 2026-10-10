# RENDER-005: 文字主体フレームのサーフェス予算

検証日: 2026-10-10。設計は [ADR-0141](../adr/0141-peak-live-render-surfaces.md)。

## 変更と受け入れ範囲

`RenderDag::tile_surface_bytes` と通常/streaming の事前検査を同時保持面の上界に変更した。単一参照結果は consumer 後に解放し、複数参照だけを CPU/GPU の pass-local memo に保持する。Group の逐次合成、先行入力を保持する多入力処理、effect の同時 temporary を共通 `SurfaceLifetime` で数える。GPU の検証待ち publication は最新8面までに制限し、sticky validation 成功前には公開しない。union ROI、16 bytes/pixel、512 MiB、型付き拒否、backend 選択、画素演算は維持する。

| 検査 | 証拠 |
|---|---|
| 320字＋図形3個、640×360 / 1920×1080 | `text_heavy_tile_budget_is_peak_residency_at_preview_and_full_hd`: 1920×1080 Compositionに4 Text × 日本語80字（20 px）を画面内配置。各tileに323 CoverageDrawが残り、32面未満。通常 frame / streaming を allocation probe backend で成功。GPU画素の実測ではない |
| 字形ごとの常駐課金を除去 | 同テストで4字と320字の全tile byte推定が一致。`streaming_siblings_do_not_accumulate_but_shared_results_do` で独立兄弟数に非依存、共有400面では400面を加算 |
| CPU 画素の意味 | `text_heavy_cpu_reference_matches_streaming_pixels`: 同じ320字＋図形を64×36で実際にCPU描画。透明一色でないことと通常/streamingのlinear/display全画素完全一致を検証。高解像度2条件は上記純粋予算検査と区別する |
| 本当に保持する共有面と過大拒否 | `surface_budget_streams_hundreds_of_draws_and_retains_shared_results`: backendの400 drawを512×512・8/16 bytesで許可。各leafを二重参照するとfloat32予算で `SurfaceBudgetExceeded`。1×1 CPU実行では共有leafのraster呼出しが400回（800回ではない）で合成値も確認 |
| allocation前の拒否 | `streaming_rejects_cumulative_halo_before_backend_allocation`: 16段sigma20、512×512。通常/streamingとも `UnsupportedFeature`。呼ぶとpanicするbackendに到達せず、sinkは0回。byte乗算overflowも `UNSUPPORTED_FEATURE` |
| 既存小規模4Kの事前予算 | `only_final_singleton_root_is_elided_and_cache_keys_stay_aligned` に3840×2160の純粋budget検査を追加。GPU resident実描画の成功を意味しない |
| 検査表示 | `inspect_path_reports_halos_budgets_and_preserves_the_live_render_cache` の過大ケースを兄弟180個の累積課金から真に過大な16段haloへ変更。inspectionは引き続き非実行でlive cache不変 |

画素比較基準は [golden比較手順](golden-comparison.md) の既存基準を維持する。CPU全画素比較・既存semanticテストと、固定GPU環境のgoldenを区別し、このsandboxではGPU goldenの成功を主張しない。

## 検証結果

最終コマンドの結果を以下に記録する。workspace全体のtestは未実行。要求されたrender crate全体と、変更したGPU crateのCPU/pure検査を実行した。

- `cargo fmt --all --check`: 成功（exit 0）。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: workspace全targetで成功（exit 0）。
- `cargo test -p kronello-render --locked --no-fail-fast`: 145 passed、8 failed、4 ignored（exit 101）。失敗は下記のMetal adapter不在8件のみ。ignoredは既存の8K実機検査1件とGPU inspection検査3件。
- 最後のbackend全結果保持上界の整理後、`cargo test -p kronello-render -p kronello-gpu --locked --lib -- --skip gpu_ --skip perf_tests --skip perf001_basic` を再実行: 22 passed、0 failed、12 filtered out。`cargo test -p kronello-render --locked --test fx008` も8 passed（Generateを含む）。
- `cargo test -p kronello-gpu --locked --lib -- --skip gpu_ --skip perf_tests --skip perf001_basic`: 11 passed、0 failed、12 filtered out。GPU不要のeffect丸め・stroke・共有面・4K admissionを含む。
- `python3 scripts/backlog.py render` / `python3 scripts/backlog.py check`: 成功。`ok: 145 tasks`（exit 0）。`git diff --check` も成功。

renderのGPU必須8件は `AdapterUnavailable("... metal found no adapters ...")`。sandboxでMetal adapterを取得できない環境要因であり、CPU fallbackや自動skipは追加していない。対象は以下。

- `gpu_animated_shape_and_japanese_text_match_cpu_all_pixels_and_order`
- `gpu_fx002_rotation_nonuniform_shear_crop_and_tile_boundaries_match_cpu`
- `gpu_fx_cropped_roi_matches_full_render_with_stacked_effects`
- `gpu_gradient_dag_transform_and_animation_match_cpu_reference`
- `gpu_isolated_alpha_and_luma_mattes_and_sequence_match_reference`
- `gpu_vec004_bbox_transform_animation_match_cpu_reference`
- `gpu_vec005_affine_animation_alignment_and_roi_match_cpu`
- `temporal_gpu_matches_whole_composition_cpu_and_warm_cache`

監督エージェントがMetal利用可能な環境で上記8件、既存ignoredのGPU/8K検査、GPU texture pool/cache/4K preview検査を再実行する。GPUでの面再利用・cache eviction後のpixel parityと実測peakはこの作業環境では未検証。

CPU/pureによる上記受け入れ条件を確認してRENDER-005を `done` とした。GPU実機の未検証範囲は上記のとおり残す。コミットは作成していない。
