# TEMPLATE-002 検証記録

## 実装範囲

worktree `m3-template2`、基点 `dd7a24f55f2d67129fde86ce09c42d0906c27373`。
[ADR-0059](../adr/0059-template-duration-variants-and-migration.md) に追加契約を記録した。
hold / loop / stretch、明示した portrait variant、inline table の Text / Property projection、
Null node の MediaSlot、read-only preview / migration plan、revision と hash による明示 apply / Undo を実装した。
LAYOUT-001 の stage / overflow / cycle を再利用し、既存の wrap_width 基準を維持する。

MediaSlot の保存・束縛・asset ID の比較は対応するが、Composition Media-node 描画は後続タスク。
active slot の final render は `UNSUPPORTED_FEATURE`、参照欠落は `ASSET_MISSING`。
supervisor が承認した境界であり、media 画素の描画成功を確認したものではない。
GPU / hardware codec / 他 OS の成功は今回の sandbox の証拠に含めない。

## 条件と証拠

| 受け入れ条件 | テスト・再現内容 |
|---|---|
| 1. NLE の保護 policy | service nle variant_clip_stretch_cannot_bypass_protected_duration_policy。variant の Composition も汎用 clip stretch を PROTECTED_INTERVAL で拒否 |
| 1. 短尺拒否、hold / loop / stretch | template `protected_hold_loop_stretch_have_exact_boundaries_and_no_history`。intro / outro、最低尺の前後、周期端・端数周期、hold の静止 pose、stretch の中点、評価順を逆転、domain 拒否、JSON 往復 |
| 2. 差分計画・preview、既存作品の不変 | service `migration_diff_previews_are_read_only_deterministic_and_explicit_apply_is_undoable`。版・入力 default・寸法・bounds の before / after、plan の完全一致、不変 revision、hash すり替え拒否、明示 apply、再送、Undo、別 instance の保持 |
| 2. 入力解決・異系列・古い revision | service `migration_requires_explicit_resolution_and_variant_content_cannot_be_obscured`。旧上書きの勝手な削除を拒否、inputs の明示リセットを diff に表示、異系列と revision conflict、variant の内容を opaque に隠す import 拒否 |
| 2. 実 CLI の画像 preview と適用 | CLI `template2_cli_previews_and_migration_plan_share_schema_and_explicit_apply`。portrait、loop→hold の local_time、schema に適合する before / after の CPU FrameResult、非ゼロ alpha、plan 後の版固定、edit.apply 後の版変更 |
| 3. table・公開入力・variant の独立性 | template `table_schema_projections_variants_and_authoring_pins_are_explicit`。不正 cell / 列 / 型 / row、不足・重複 binding、未知 variant、内容 hash。service `data_projection_variant_inputs_durations_versions_and_preview_failures_are_isolated`。異なる table・尺・variant、非公開名、preview overflow と保存不変 |
| 3. MediaSlot | service `media_slots_validate_refs_expose_bindings_and_reject_final_execution`。Null target、2 instance の異なる AssetRef / 尺、preview の binding と未対応 diagnostic、final render の型付き拒否、欠落 ID と revision 不変 |
| 3. 共有 API / schema | service api の全 registry request decode / schema / actual successful result / URI 拒否。MCP `public_api_fixture_commands_return_schema_valid_success_from_real_binary` の対応 2 protocol で preview / migration_plan。model / service の生成 schema 一致 |
| 4. variant + data + ink + 変換 + padding | render `template2_variants_tables_versions_and_ink_padding_share_pure_renderer`。短文・空白・空本文・複数行、portrait、反転・回転・scale、異なる版 / hold / loop、正しい帯寸法と位置、cache と評価順に依存しない結果 |
| 4. 既定 layout / ink / visual の区別 | 既存 render `explicit_bounds_stages_follow_short_whitespace_multiline_and_transformed_text` と `japanese_instances_have_independent_text_color_duration_and_following_bands`。旧 wrap_width 幅と全 stage の意味・padding・CPU 描画 |
| 4. 循環・overflow | render unit `compiler_declares_each_stage_and_diagnoses_closed_wrap_band_cycles`、render `indivisible_width_overflow_is_typed_for_templates_and_plain_text_and_not_published` / `overflow_is_typed_and_sequence_has_no_published_output`、service integration_query の typed details の renderer 一致 |

cycle は静的依存宣言の compiler 境界で検査する。公開 API に任意式・逆向き依存を追加していない。
preview の diagnostic は render 成功ではなく、frame は未生成。region 省略時は意味的結果のみ、
region 指定時は選択 backend の frame を生成する。通常 scene.query の bounds と同じ root design_px。

## 公開契約

- `TemplateMiddleMode: hold | loop | stretch`。`TimeMap::Protected` /
  `ProtectedMiddleMode: hold | loop` を追加。Linear / PiecewiseLinear は従来どおり。
- `TemplateDefinition.variants?: BTreeMap<String,TemplateVariant>`、
  `TemplateVariant = {composition_ref,targets,constraints,content_hash}`。
  `TemplateInstance.variant?: string`、省略は従来 base。
- `ValueType::DataTable` / `Value::DataTable({columns,rows})`。
  `TemplateInputTarget::DataTable {bindings:[{row,column,target}]}`、
  target は Text / Property。`TemplateInputTarget::MediaSlot {node}` は Value::AssetRef。
- `template.preview: {project,instance,time,fonts,region?}` →
  `TemplatePreviewResult {revision,instance,definition,design_extent,local_time,resolved_inputs,media_slots,nodes,frame,diagnostic}`。
- `template.migration_plan: {project,base_revision,instance,definition,variant?,inputs?,time,fonts,region?}` →
  `TemplateMigrationPlan {plan,changes,before,after}`。
  `changes[] = {field,before,after}`。apply は返された EditPlan の commands / plan_hash。
- `TemplateCommand::Migrate {instance,definition,variant,inputs}`（EditCommand の template branch）。
  CLI は同名二語 subcommand、MCP は共有 registry から公開。read_only は両 query とも true。
- 新しい診断: `INVALID_DATA_TABLE`、`TEMPLATE_VARIANT_NOT_FOUND`、
  `TEMPLATE_MIGRATION_INCOMPATIBLE`。MediaSlot は既存 `ASSET_MISSING` / `UNSUPPORTED_FEATURE` を使用。
  既存 `DURATION_TOO_SHORT`、`INVALID_TEMPLATE`、`INPUT_NOT_PUBLIC`、
  `TEMPLATE_DEFINITION_CHANGED`、`TEMPLATE_VERSION_EXISTS`、`LAYOUT_OVERFLOW`、
  `TEMPLATE_OVERFLOW`、`PROPERTY_DEPENDENCY_CYCLE` 等を維持。

`examples/template-002.project.json` / `template-002.definition.json` は landscape と portrait、
一行の typed table、ink の帯、loop の保護尺を持つ再現用 authoring 入力。
content_hash は define が計算する。保存済み版は変更しない。

## コマンド

Rust 1.95.0 / edition 2024、全 Cargo command は `CARGO_BUILD_JOBS=3`。
注入された共通 CARGO_HOME / CARGO_TARGET_DIR と管理された TMPDIR を使用。

```sh
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
CARGO_BUILD_JOBS=3 cargo run -p kronello-model --example project_schema --locked > schemas/project-v1.schema.json
CARGO_BUILD_JOBS=3 cargo run -p kronello-service --example api_schema --locked > schemas/api-v1.schema.json
CARGO_BUILD_JOBS=3 cargo test -p kronello-template --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-service --test template --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-render --test template --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-cli --test machine template2_cli_previews_and_migration_plan_share_schema_and_explicit_apply --locked
CARGO_BUILD_JOBS=3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
CARGO_BUILD_JOBS=3 cargo test -p kronello-service --test integration_query --locked
git diff --check
```

## 実行結果

全コマンドをこの worktree の sandbox で実行した。最終結果は以下。

| command / suite | 結果 |
|---|---|
| fetch_fixtures | 終了コード 0、固定 Noto font を取得・検証 |
| fixtures generate / check | 各終了コード 0、9 scene の software media fixture を生成・decode、16 manifest entry / 28,012 bundled bytes を検証 |
| model / service schema generator | 各終了コード 0、生成 schema 一致テストも成功 |
| focused template | 終了コード 0、7 passed |
| focused service template | 終了コード 0、9 passed |
| focused render template | 終了コード 0、13 passed |
| focused CLI template2 | 終了コード 0、1 passed / 17 filtered |
| fmt --all --check | 終了コード 0 |
| clippy --workspace --all-targets --locked -- -D warnings | 終了コード 0 |
| workspace excluding gpu / framebridge, --skip gpu_ | 終了コード 0、69 suite の合計 445 passed / 0 failed / 1 ignored / 7 filtered |
| service integration_query（filter なし） | 終了コード 0、5 passed / 0 filtered |
| git diff --check | 終了コード 0 |

workspace 内では service api 11、service nle 15、service template 9、render template 13、
template contracts 7、CLI machine 17、MCP stdio 9 が成功した。
唯一の ignored は既存 `snapshot_policy_evaluation`（明示実行の policy measurement）。
7 filtered は CLI 1、MCP 1、render 4、integration_query 1。
integration_query の filter 対象は名前に `gpu_` を含む CPU query test であり、別途 filter なしで実行した。
これは GPU backend 検証の代替ではない。

初回 workspace 実行は追加 query の ResultData wire decode の不足と旧 registry 件数 30 の assertion により
CLI suite で終了コード 101。wire decoder に両 result tag を追加し、共有 registry 件数を 32 に更新後、
上記の focused CLI と最終 workspace 全体を再実行して成功した。
初回 Clippy で指摘された needless range loop と store fixture の新規 field 不足も修正し、最終 Clippy を再実行した。

protected 管理ファイルには差分がなく、commit / push / merge は行っていない。

## pending host run

```sh
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
```

期待結果: GPU / FrameBridge を含む workspace が終了コード 0。
sandbox の CPU / schema 成功をこの実機 gate の代替にしない。host 実行は supervisor が担当する。

## Supervisor 管理ファイルへの依頼

- docs/adr/README.md: ADR-0059 を登録。
- docs/backlog/backlog.json / 派生 BACKLOG.md: host gate とレビュー後に TEMPLATE-002 の状態・証拠を更新し、
  `python3 scripts/backlog.py render` / `check` を実行。
- Composition Media-node 描画を後続 backlog task にする（承認済み境界）。
- docs/README.md: 必要ならこの検証記録を登録。
- docs/open-questions.md、docs/design-system/**: worker は変更していない。未決論点は決定していない。

## host での実行結果（supervisor）

2026-10-05、Apple Silicon（Metal）の host で supervisor が実行した。

```sh
python3 scripts/fetch_fixtures.py && python3 scripts/fixtures.py generate
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=5 cargo test --workspace --locked
```

すべて exit 0。workspace test は 499 passed / 0 failed / 4 ignored（GPU / FrameBridge を含む）。
この branch は FFI-001 の merge 前に分岐しているため、Swift DTO の再生成（`scripts/generate_swift_api.py`）は統合 branch への merge 時に行う。
