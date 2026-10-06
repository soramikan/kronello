# INSPECT-001 検証記録

現在の状態（2026-10-06）: INSPECT-001はM3の受け入れ範囲で`done`。最終統合・実機検証と保証外の範囲は [M3統合受け入れ](m3-acceptance.md) と本書の後続記録を参照する。以下の初回worker記録にある「未コミット」「pending host run」は、その記録時点の状態であり、現在の未完了判定ではない。

## 初回実装とその後の検証履歴

基点 `6ddc568bcf33499e642ea0b77cd3ed6cd945556c`、branch `m3-inspect`。
Darwin arm64 sandbox、Rust 1.95.0。worker は GPU / hardware codec を使用していない。
仕様は [ADR-0060](../adr/0060-structured-read-only-inspection.md)。

## 受け入れ条件と通常テスト

| 条件 | test / 内容 |
|---|---|
| 1. opacity / active range / hidden ancestor | service `inspection_separates_own_opacity_half_open_range_and_hidden_ancestors`。自身・祖先の opacity 0、local time `[start,end)` の end、category / subject の区別、同じ query 前後の Project / revision 一致 |
| 1. transform parent | service `transform_parent_activity_and_opacity_do_not_hide_the_child_but_zero_scale_does`。親の非アクティブ・opacity 0 の非継承、transform dependency、collapsed world transform |
| 1. placement / local time | service `instance_paths_map_local_time_and_an_inactive_placement_skips_out_of_domain_map`。piecewise map と instance identity、inactive placement の下で domain 外 TimeMap を呼ばず local_time null |
| 1. alpha / luminance matte | service `mattes_report_consumption_zero_opacity_inactivity_and_unobserved_coverage`、`alpha_and_luminance_mattes_distinguish_proven_zero_from_unobserved_coverage`。matte-only、opacity 0、透明 paint の zero coverage、黒い leaf の zero luminance、inactive matte の blocking、未観測 coverage の indeterminate |
| 1. missing asset / font / unsupported | service `missing_content_font_and_opaque_effect_are_structured_and_do_not_edit`。欠落 Shape、font locator、opaque effect の安定 code / resource details、作品不変 |
| 1. paint / 不確実性 | service `transparent_paint_and_unrelated_compiler_failure_are_not_mistaken_for_visible_pixels`。透明 paint、自身の資産欠落と無関係な text の font failure の区別、pixel_visibility_observed false |
| 1. 式 / 依存 | service `expression_dependencies_and_typed_failures_are_returned_without_fallback`。Expression upstream 辺、式の opacity 0、除算失敗と元の EVALUATION_ERROR、代替 opacity を返さない |
| 1. layout 依存 | service integration_query `node_explain_reports_renderer_layout_dependencies_for_template_band`。実 renderer の band → layout → text Property 宣言辺、typed layout runtime key、query 前後の文書一致 |
| 2. stages / tile / transfer / memory / cache | service `render_plan_uses_tiling_real_compile_counters_and_labeled_transfer_estimates`。1024×512 の2 tiles、DAG stages、GPU image / status readback の推定、CPU の GPU 転送0、二回描画・透明 node の処理 notice、隔離 geometry cache の実 hits / misses、raster 未観測、繰り返し query の完全一致 |
| 2. Sequence / 色 | service `sequence_plan_uses_the_same_lowering_and_sequence_working_space`。既存 Sequence lowering、gap / active clip の DAG 差、Sequence の linear_rec2020 が profile の正本 |
| 2. halo / 安全予算 / cache 不変 | render `inspect_path_reports_halos_budgets_and_preserves_the_live_render_cache`。stage inputs / execution region を実 DAG と照合、shadow halo、512 MiB 安全予算超過 notice。live cache の entries / counters 不変と次の実 CPU render の raster hit・同じ画素 |
| 共通 API / schema | service `public_schema_matches_and_all_registry_schemas_are_safe`、`every_request_payload_and_envelope_matches_schema_and_denies_execution_fields`、`actual_results_for_every_command_match_envelope_and_registry_schemas`。全32操作、Rust schema 一致、要求・応答往復、schema validation、禁止実行 field |
| 実 CLI | `explain_subcommands_and_tagged_requests_share_read_only_diagnostics`。node explain / render explain、tagged stdin、明示 backend、adapter failure 注入が explain に影響しない、revision 不変 |
| 実 MCP | `explain_tools_are_discovered_and_return_shared_results_without_a_device`。tools/list、readOnlyProject、input / output schema、structuredContent、GPU adapter failure 注入下の成功 |
| FFI worker | `explain_queries_cross_the_worker_abi_with_the_shared_schema`。kronello_call / poll_json の結果を service と完全一致比較 |
| 入力境界 | service `explain_rejects_unknown_fields_urls_and_missing_runtime_node_without_creating_project`。未知 field、URI、存在しない node / project、missing Project を作成しない |

## 公開契約

- Request: `node.explain: NodeExplainRequest`、`render.explain: RenderExplainRequest`。read_only registry の2操作。
- ResultData: `node_explanation: NodeExplainResult`、`render_explanation: RenderExplainResult`。Rust 内部では Box、wire shape は object。
- VisibilityCode / category / impact / assessment、typed InspectionPropertyKey、原因・依存・予定 stage / tile / transfer / cache 型を API schema に追加。版は API schema 1 のまま。Project schema / 保存文書の field は変更していない。
- cache counters は隔離 compile の実値、転送・面・メモリは `_estimate`。control upload の量は未推定の null。runtime raster / device availability / timing / occlusion は未測定。
- surface_bytes_estimate は基準 RGBA16F 面の payload。CPU の intermediate_bytes_estimate は float32 面の payload から計算する。
- transient MatteBinding は既存 RenderSnapshot と同じ意味で、Query から通常 render.frame へ暗黙に保存・伝播しない。文書の enabled は追加していない。

## 再現コマンド

全 Cargo command は `CARGO_BUILD_JOBS=3`。注入された共通 `CARGO_HOME` / `CARGO_TARGET_DIR` と管理された `TMPDIR` を使用した。別の Cargo cache は作成していない。
FFI の job subscription は利用者の既定 state directory へ書き込むため、最終 workspace 検証では `KRONELLO_STATE_ROOT="$TMPDIR/inspect-test-state"` を設定した。専用 job test 自身の state は各 fixture の明示設定を使う。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
CARGO_BUILD_JOBS=3 cargo run -p kronello-service --example api_schema --locked > schemas/api-v1.schema.json
python3 scripts/generate_swift_api.py
export KRONELLO_STATE_ROOT="$TMPDIR/inspect-test-state"
CARGO_BUILD_JOBS=3 cargo run -p kronello-cli --locked -- --request-json '{"operation":"job.list"}'
CARGO_BUILD_JOBS=3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
CARGO_BUILD_JOBS=3 cargo test -p kronello-service --test integration_query --locked
python3 scripts/generate_swift_api.py --check
git diff --check
```

## worker の実行結果

最終の管理 state root を指定した workspace command は exit 0、全 test summary の合計474 passed / 0 failed / 1 ignored / 7 filtered out（GPU / framebridge crate を除外）。service inspect は11件、API schema / registry は11件、render は33件（追加 inspection 1件を含む）、CLI machine は18件、CLI jobs は16件、FFI boundary は9件が成功した。MCP の実 explain tool test も成功。integration_query は filter により5件成功 / 1件除外だったため、最終ソースで専用 command を実行し6件すべて成功、exit 0。この専用 command は CPU の layout query だけを実行し、GPU device を初期化しない。GPU / hardware の成功を表す結果ではない。
ignored は既存 `snapshot_policy_evaluation`（明示実行を要する policy measurement）。この測定は今回実行していない。

初回 workspace command は既存 CLI `worker_checks_saved_schema_semantics_features_and_input_hash` が60秒待機で失敗（jobs は15 passed / 1 failed、command exit 101）。無変更での診断用専用実行 `cargo test -p kronello-cli --test jobs worker_checks_saved_schema_semantics_features_and_input_hash --locked -- --exact --test-threads=1` は1 passed、exit 0。この並列実行時のタイムアウトは原因未確定の残件として保持する。

継続用の `cargo test -p kronello-service -p kronello-render -p kronello-cli -p kronello-mcp -p kronello-ffi --locked -- --skip gpu_ --skip worker_checks_saved_schema_semantics_features_and_input_hash` は CLI capabilities の旧 registry count 30 と実 count 32 の不一致で exit 101。期待値を32へ修正した。これは required workspace command の代替ではない。再実行の workspace では jobs 16件と CLI machine 18件は成功したが、FFI `subscription_detects_external_revision_and_job_snapshot` の job_progress が error（boundary は8 passed / 1 failed、command exit 101）。JobConfig の既定先 `~/Library/Application Support/Kronello` は sandbox の書き込み範囲外であるため、上記の明示した管理 state root を使用した。通常の CLI `job.list` による state 初期化は empty jobs の success、exit 0。

全 target Clippy で基点に存在した render unit test の `EvaluationSnapshot` 初期化子に `expressions` が不足していることを検出し、空配列を追加した。これは検査機能ではなく、既存テストのコンパイル修正。最終 Clippy / fmt / Swift generator check は exit 0。

固定 font fixture を hash 検証して取得し、9 media fixtures を生成・decode。fixture check は16 entries / 9 scenes / 28,012 bundled bytes、終了コード0。schema / Swift の生成と Swift generator check は終了コード0。初回 offline fetch は fixture が未配置のため失敗し、通常 fetch 後に解消した。

## pending host run

supervisor が次を実行する。期待結果は終了コード0と全 test 成功。worker は未実行であり、ここに受け入れ成功の判定は記録していない。

```sh
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
swift build --package-path apps/macos
swift test --package-path apps/macos
```

GPU 転送の推定値はコードからの計画であり、host test 成功だけでも実転送・性能の実測にはならない。OQ-14 は未決のまま。

## supervisor 管理ファイルと follow-up

- `docs/adr/README.md`: ADR-0060 を登録。
- `docs/backlog/backlog.json` / `docs/backlog/BACKLOG.md`: host gate / review / integration 結果に応じた INSPECT-001 の状態と根拠を更新し、backlog render / check を実行。
- `docs/README.md`: 必要に応じて本検証記録へのリンク。
- `docs/design-system/**`: 安定 code / structured fields を使用し、potentially_visible / estimate / unobserved の意味を表示へ反映。
- GUI-001 の enabled field を merge した際、disabled と ancestor disabled の VisibilityCode / 原因 / 回帰テストを追加する。この branch では field を追加しない。
- runtime warm cache / timings、control upload の精密推定、画素 coverage / occlusion の実観測、native preview / FrameBridge の経路は今回の計画 Query では扱わない。

上記 supervisor 管理ファイル、open-questions は worker で変更していない。変更は未コミット。

## host での実行結果（supervisor）

2026-10-05、Apple Silicon（Metal）の host で supervisor が実行した。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=5 cargo test --workspace --locked
python3 scripts/generate_swift_api.py --check
python3 scripts/build_ffi.py && python3 scripts/fetch_ui_fonts.py
swift build --package-path apps/macos && swift test --package-path apps/macos
```

すべて exit 0。workspace test は 529 passed / 0 failed / 4 ignored（GPU / FrameBridge を含む）、SwiftPM test も成功。
実 GPU 上の時間・キャッシュの実測値は引き続き未観測（推定値として返す）。
