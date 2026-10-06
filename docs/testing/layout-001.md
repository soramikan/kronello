# LAYOUT-001 検証記録

## 実装と範囲

`m3-layout`（基点 `ecf99f2ce4d75bff77e93b2aa1abac32bb7cf167`）で実装。
三段階の純粋 `LayoutValue`、明示した帯の `BoundsStage`、静的依存宣言、幅 overflow、共有 query / schema を接続した。
定義は [ADR-0057](../adr/0057-layout-bounds-stages.md)。既定の wrap_width 基準と既存 text-local `evaluated.layout_bounds` を維持する。
新しい canonical `evaluated.bounds` は全段階を root Composition の `design_px` で返す。
空白・空本文の ink / visual は null、追従帯は text 原点の padding だけとなる。
Shape の線、変換した AABB、子の集約は保守的な包含矩形。mask / 透明度による tight な alpha bounding box は保証しない。
visual の連続 support と出力 pixel 格子の丸めは区別する。

## 受け入れ条件と対応

| 条件 | 通常テスト・確認内容 |
|---|---|
| 1. layout / ink / visual の区別 | render `explicit_bounds_stages_follow_short_whitespace_multiline_and_transformed_text`。短文・前後空白・複数行・全空白・空本文、回転・uniform scale・反転、blur + shadow の全 stage を比較 |
| 1. glyph / stroke / effect と階層 | render `supported_stroke_styles_nonuniform_transform_and_zero_scale`、`fx_halo_requests_and_transformed_visual_bounds_are_analytical`、`group_bounds_union_children_and_apply_group_effect_after_child_effects`。vector `analytic_bounds_use_curve_extrema_instead_of_control_polygon_or_output_resolution` |
| 1. 共有 Query API | service `evaluated_query_returns_all_bounds_stages_and_explicit_follower_values`。全 stage を同時に返し、従来 local layout、Composition 空間、Shape shadow、placement の集約を確認 |
| 2. 循環の型付き診断 | render unit `compiler_declares_each_stage_and_diagnoses_closed_wrap_band_cycles`。実 compiler が作る各 stage の依存辺へ逆 wrap 依存を加え、layout / wrap / band size を含む閉経路と `PROPERTY_DEPENDENCY_CYCLE` を確認。eval の既存 `declared_layout_values_schedule_consumers_and_reject_reverse_wrap_cycle` も維持 |
| 2. 診断の wire shape | service `cycle_diagnostics_keep_closed_typed_runtime_keys_in_service_details`。details.path の構造化 runtime key と閉経路を確認 |
| 2. 行数 overflow と未公開 | 既存 render `overflow_is_typed_and_sequence_has_no_published_output`、`inactive_placement_parent_skips_layout_and_overflow`。`TEMPLATE_OVERFLOW`、非アクティブな親の除外、出力ディレクトリ・中間ファイルなし |
| 2. 幅 overflow と未公開 | render `indivisible_width_overflow_is_typed_for_templates_and_plain_text_and_not_published`。分割不能 glyph の幅超過を通常 text / template とも `LAYOUT_OVERFLOW`。連番・中間ファイルなし |
| 2. Query と最終出力の同じ失敗 | service `evaluated_query_and_final_render_share_structured_width_overflow`。node / instance_path / line / advance / wrap_width を 明示 CPU renderer と完全一致比較。Query は GPU を初期化しない |
| 3. 純粋値・stage 選択と描画 | render `explicit_bounds_stages_follow_short_whitespace_multiline_and_transformed_text`。各選択と padding / size / position、DAG Geometry と CPU 画素、順序の異なる有理数 time と cache の有無を確認 |
| 3. pixel と解像度分離 | render `semantic_visual_bounds_cover_cpu_text_pixels_at_multiple_output_scales`。1× / 2× / 3× の text + blur + shadow の非ゼロ alpha を semantic visual envelope と AA の 1 pixel 許容内で包含し、Scene IR が解像度に依存しないことを確認 |
| 3. 宣言順に依存しない計算 | render `visual_band_dependencies_are_scheduled_independently_of_constraint_order`。別の帯を変換親として読む visual projection を、producer / consumer の定義順を逆転して完全一致比較。`DependencyGraph::dependency_order` の静的順序で入力を生成 |
| 3. 親空間と明示診断 | render `visual_following_in_shared_parent_space_and_singular_parent_is_diagnosed`、`bounds_query_preserves_renderer_rejection_of_nonuniform_blur`、`nonleaf_text_follower_is_explicitly_unsupported`。共通親の scale、特異な親、既存の非一様 Gaussian 拒否、leaf text の制約 |
| 既定・保存互換性 | model `bounds_selection_is_explicit_and_legacy_layout_default_round_trips`。旧 JSON の既定 layout、ink / visual の保存、未知 stage の opaque 保持。既存 render の日本語 2 instance 回帰で wrap_width 基準を維持 |
| 版固定・schema | 既存 render snapshot 契約テストに bounds 版 99 の拒否を追加。Rust から生成した Project / API schema の一致テストを通常実行 |

静的循環のテストは依存宣言の compiler 境界である。今回、公開 API に任意の逆向き width binding / Expression authoring を追加したものではない。
responsive variant は TEMPLATE-002、glow と非一様 Gaussian は既存の後続タスク範囲。

## 公開契約の追加

- 保存 `TemplateBandBinding.bounds?: "layout" | "ink" | "visual"`（既定 `layout`）。旧定義の JSON は省略を維持。
- `SceneNodeEvaluation.bounds: LayoutValue` と `SceneNodeIr.bounds: LayoutValue`。
  wire は `{layout_bounds: Bounds|null, ink_bounds: Bounds|null, visual_bounds: Bounds|null}`、
  `Bounds = {min: [number, number], max: [number, number]}`。root Composition / design_px。
  既存 `SceneNodeEvaluation.layout_bounds` は text-local のまま。
- `SemanticVersions.bounds: u32`（固定 1、旧 snapshot の省略も初版 1、未知版は拒否）。
- render の `DesignBounds::{checked, transform, union}` と `LayoutValue::select(BoundsStage)` は純粋な導出・選択。
- `DependencyGraph::dependency_order(&self, keys: &[RuntimePropertyKey]) -> Result<Vec<RuntimePropertyKey>, EvaluationError>`。
- `DependencyGraph::node_parent_world_transform_with_inputs(&self, key: &NodeKey, time: Time, inputs: &BTreeMap<RuntimePropertyKey, Value>) -> Result<Affine2, EvaluationError>`。
- `kronello_vector::geometry_bounds(&ResolvedGeometry) -> Result<Option<GeometryBounds>, VectorError>`、
  `GeometryBounds = ([f64; 2], [f64; 2])`。局所解析幾何であり、出力 pixel scale を受け取らない。
- `RenderError::LayoutOverflow {node: NodeId, instance_path: InstancePath, line: usize, advance: f64, wrap_width: f64}` → `LAYOUT_OVERFLOW`。
  ServiceError の details も同名の五項目。`RenderError::SingularLayoutTransform` → `LAYOUT_SINGULAR_TRANSFORM`。
- `PROPERTY_DEPENDENCY_CYCLE` の `details.path` は閉経路。node / layout / composition の wire key は [08 API](../architecture/08-api-cli-mcp.md) に記載。

新しい service operation は追加していない。`kronello-text` 本体・組版意味版、Cargo dependency / lock、supervisor 管理ファイルは変更していない。

## 再現コマンド

Rust 1.95.0、edition 2024。全 Cargo command は `CARGO_BUILD_JOBS=3`。
worker は注入された共通 `CARGO_HOME` / `CARGO_TARGET_DIR` と管理された `TMPDIR` を使った。
以下は sandbox の CPU / schema 検証であり、GPU / hardware codec の成功を意味しない。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
CARGO_BUILD_JOBS=3 cargo run -p kronello-model --example project_schema --locked > schemas/project-v1.schema.json
CARGO_BUILD_JOBS=3 cargo run -p kronello-service --example api_schema --locked > schemas/api-v1.schema.json
CARGO_BUILD_JOBS=3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
CARGO_BUILD_JOBS=3 cargo test -p kronello-service --test integration_query --locked
git diff --check
```

## 今回の実行結果（2026-10-04、Darwin arm64 sandbox）

- `python3 scripts/fetch_fixtures.py`: 固定 Noto Sans CJK JP の bytes / hash を検証、終了コード 0。
- Project / API schema の生成: ともに終了コード 0。
- 初期の render 対象確認: unit **3 passed**、template **8 passed**（終了コード 0）。その後追加したテストの結果は最終 CPU command に含めて記録する。
- Query / 最終 render の比較 test 追加中、一度 GPU 選択の render.frame が `ADAPTER_UNAVAILABLE` で失敗した。GPU 成功の証拠はなく、再試行していない。比較の最終 test は明示した CPU backend を使う（GPU 成功への代替ではない）。
- `python3 scripts/fixtures.py generate`: 9 media fixture を生成・decode、終了コード 0。`check --generated target/fixtures/generated`: 16 entries / 9 scenes / 28,012 bundled bytes、終了コード 0。初回 CPU workspace の fixture 不足（2 media test 失敗）は生成後に解消。
- 最終 `cargo fmt --all --check`: 終了コード 0。
- 最終 `cargo clippy --workspace --all-targets --locked -- -D warnings`: 終了コード 0。
- 最終 CPU workspace command: **436 passed / 0 failed / 1 ignored / 7 filtered**、終了コード 0。ignored は既存 `snapshot_policy_evaluation`、filtered は名前に `gpu_` を含む 7 件（既存の CPU query test 1 件も名前で除外されるため、下記の対象 command で別に確認）。GPU / FrameBridge crate は command の明示除外。
- render は unit **3**、render **32**、template **12** 成功。CPU workspace の integration_query は **4 passed / 1 filtered**。
- `CARGO_BUILD_JOBS=3 cargo test -p kronello-service --test integration_query --locked`: **5 passed / 0 failed / 0 filtered**、終了コード 0。名前で除外された `evaluated_template_queries_share_layout_inputs_and_do_not_initialize_gpu_or_edit` も実行済み。
- 上表の全 test、Project / API schema の生成器一致は、最終 CPU workspace と対象 command で成功を確認。
- 既存の anisotropy test は、型付き拒否が DAG から bounds を導出する Scene IR compile へ早まったことに合わせて更新し、uniform scale / rotation の semantic と pixel bounds の丸め差も確認した。
- `git diff --check`: 終了コード 0。

## host での実行（supervisor）

2026-10-04、Apple Silicon（Metal）の host で supervisor が実行した。

```sh
CARGO_BUILD_JOBS=4 cargo test --workspace --locked
```

終了コード 0。78 の test binary で 490 passed / 0 failed / 4 ignored（GPU / FrameBridge の test を含む）。
他 OS と固定 GPU 画素の新規 golden はこのタスクの範囲外。

## Supervisor 管理ファイルへの依頼

- `docs/adr/README.md`: ADR-0057 を登録。
- `docs/backlog/backlog.json` / 派生 `BACKLOG.md`: host gate とレビュー結果に応じた LAYOUT-001 の状態・検証根拠の更新。TEMPLATE-002 の tight-ink 条件は今回の共通基盤を利用する variant 検証として重複範囲を整理する。
  `python3 scripts/backlog.py render` / `python3 scripts/backlog.py check` は supervisor の状態更新後に実行する。
- `docs/README.md`: 必要なら LAYOUT-001 検証記録へのリンクを追加。
- `docs/design-system/**`: canvas / template 比較 UI は旧 text-local layout ではなく `evaluated.bounds` の共通 Composition 座標を使用する旨を必要に応じて反映。
- `docs/open-questions.md`: 今回は未決事項を解決していないため変更依頼なし。

これらの supervisor 管理ファイルは worker では編集していない。
