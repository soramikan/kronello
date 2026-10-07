# INSPECT-002 検証

状態: `done`。統合受け入れは [M5 checkpoint](m5-acceptance.md) を参照。

## 契約と対応

[ADR-0097](../adr/0097-read-only-single-graph-render-plans.md) に従い、`RenderPathPlan.transfers` は cold final CPU 出力境界、`native_preview_transfers` は全領域 native texture 一 graph の代替境界を表す。数値は見積もりであり actual counters ではない。control upload / native decode conversion / disk raster persistence と driver allocation は計画から保証しない。

- 通常 GPU: tile の execution region（effect halo を含む）の row-padded RGBA16F 二面 + status 4 byte、3 operations / graph。native preview は画像 readback なし、status 4 byte / 一操作。
- resident BGRA8 / NV12: graph 内 CPU image upload と中間 image copy を加えない。native decoder / import の外部 pool と conversion は対象外。strict resident + temporal は実 renderer と同じ型付き未対応。
- CPU reference: GPU transfer は全ゼロ。未知 backend は全 null。
- temporal / tile: 実 renderer と同じ rational sample / execution tile を列挙して合計する。warm temporal hit のゼロ追加実行は cold 見積もりと区別する。

## テスト

`crates/kronello-render/tests/inspect_002.rs`:

- `backend_free_temporal_plan_matches_actual_cpu_execution_count_and_warm_cache`: planning の backend 呼出ゼロ、cold の実行 3 samples × 2 tiles、warm の追加実行ゼロ、CPU / unknown / resident temporal 拒否。
- `single_graph_final_and_native_preview_estimates_match_actual_gpu_counters`: actual GPU の padded final counters、status-only native preview、旧 duplicate notice の消失。actual adapter が必要な ignored test。
- `cpu_unknown_and_temporal_tile_plans_respect_execution_boundaries`: actual GPU の temporal × tile 合計 counters。actual adapter が必要な ignored test。
- `resident_graph_input_and_final_boundary_match_counters_without_cpu_image_upload`: 同じ device の明示 ResidentImage を持つ graph の counters。これは hardware decoder の保証を追加するテストではない。actual adapter が必要な ignored test。

共有 service の `render_plan_uses_tiling_real_compile_counters_and_labeled_transfer_estimates` と `inspect002_temporal_explain_plans_actual_samples_without_executing_or_mutating` は繰り返し結果一致、isolated compilation counters、raster miss ゼロ、project export / revision 不変を確認する。

```sh
cargo test -p kronello-render --locked --test inspect_002
cargo test -p kronello-service --locked --test inspect
cargo test -p kronello-render --locked --test inspect_002 -- --ignored --nocapture
```

2026-10-06: ordinary CPU counter test と targeted service 二件は成功した。sandbox 内の actual GPU テストは Metal adapter 不可視で開始に失敗した。同日にホスト権限で `cargo test -p kronello-render --locked --test inspect_002 -- --ignored --nocapture` を実行し、上記 actual GPU 三件が成功した（3 passed、0 failed）。全 workspace 796 tests / 0 failures、fmt / clippy / API / schema の成功は [M5 checkpoint](m5-acceptance.md) に記録した。
