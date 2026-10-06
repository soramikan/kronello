# EXPR-001 の検証

現在の状態（2026-10-06）: EXPR-001はM3の受け入れ範囲で`done`。最終統合・実機検証と保証外の範囲は [M3統合受け入れ](m3-acceptance.md) と本書の後続記録を参照する。以下の初回worker記録にある「未コミット」「pending host run」は、その記録時点の状態であり、現在の未完了判定ではない。

## 初回実装とその後の検証履歴

対象: `.worktrees/m3-expr`、branch `m3-expr`、基点 `ecf99f2ce4d75bff77e93b2aa1abac32bb7cf167` からの未コミット変更。2026-10-04、macOS / arm64、Rust 1.95.0、worker sandbox。Cargo は注入された共有 CARGO_HOME / CARGO_TARGET_DIR と `CARGO_BUILD_JOBS=3` を使う。GPU / hardware codec を利用できる環境としては扱わない。

## 受け入れ条件と証拠

| 条件 | テスト / 手順 | 結果と範囲 |
|---|---|---|
| 1. 静的依存列挙と命令 / メモリ / sample 予算 | eval `static_dependencies_cycles_missing_and_wrong_types`、`instruction_memory_sample_node_dependency_budgets_are_typed`、`default_sample_ceiling_counts_repeated_references`、`literal_and_upstream_payload_memory_are_bounded`、`rational_curve_samples_and_arithmetic_failures` | CPU 成功。Property / Curve を列挙・型照合し、閉じた循環を診断。5 資源の低い予算、既定 64 sample に対する 65 回の同一参照、literal / upstream の 1 MiB payload、Curve の rational offset を確認 |
| 2. 固定 seed と禁止能力 | eval `fixed_seed_noise_is_independent_of_query_order_and_scoped_by_instance`、`canonical_ast_roundtrip_types_and_forbidden_capabilities`、service `shared_edit_schema_revision_idempotency_undo_and_fixed_render_snapshot` | CPU 成功。257 rational times の順 / 逆 / 固定 permutation、配置識別、要求順を比較。network / file / clock / environment / random / loop / dynamic_lookup / eval / shell を typed decode で拒否。opaque 保存の round-trip と実行拒否、Command / schema の clock 拒否を確認 |
| 3. AST 正本と将来の一意な往復 | eval `canonical_ast_roundtrip_types_and_forbidden_capabilities`、`arithmetic_builtins_and_explicit_constructors_have_typed_results` | CPU 成功。JSON 往復で AST が等しい。先行 operand、連続 postorder、root 型、演算型を検証し、共有 / dead node / forward ref / wrong type を拒否。Scalar 演算と Vec2 / Vec3 / Angle の意味を確認。言語 syntax / parser は定義していない |
| 4. 共通編集 / schema / sample / final render / revision / idempotency / Undo / fixed snapshot | service の新規 3 tests、CLI `expression_commands_and_samples_use_the_shared_cli_api`、MCP `expression_commands_and_samples_use_the_shared_mcp_api`、CLI job `expression_job_pins_ast_and_renders_after_source_project_removal` | CPU 成功。設定 batch、wire/schema、revision 2、再送の同じ Event、stale revision、Undo revision 3、競合・循環候補の原子拒否、snapshot_at(2)、serialized RenderSnapshot を確認。budget / division / cycle / missing Property / missing Curve / missing Expression は query と最終 render 入口で同じ code・message。独立 worker が AST を固定し、後続 AST 更新と原本削除後も revision 2 の 3 frames を成功公開 |

上記 4 条件の機能証拠は CPU / 共通 API の範囲で確認した。GPU の画素・host 全 workspace gate は未確認であり、全体の受け入れ完了を宣言しない。

## 実行コマンドと結果

```bash
python3 scripts/fetch_fixtures.py
CARGO_BUILD_JOBS=3 cargo run -p kronello-model --example project_schema --locked > schemas/project-v1.schema.json
CARGO_BUILD_JOBS=3 cargo run -p kronello-service --example api_schema --locked > schemas/api-v1.schema.json
CARGO_BUILD_JOBS=3 cargo test -p kronello-eval -p kronello-service --test expressions --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-cli --test machine expression_commands_and_samples_use_the_shared_cli_api --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-mcp --test stdio expression_commands_and_samples_use_the_shared_mcp_api --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-cli --test jobs expression_job_pins_ast_and_renders_after_source_project_removal --locked
CARGO_BUILD_JOBS=3 cargo test -p kronello-cli --test jobs --locked -- --test-threads=1
```

- fixture fetch: exit 0、manifest の Noto Sans CJK JP hash / size を検証。Cargo cache とは別の worktree 内 `target/fixtures/external` に置いた。
- schema 再生成: 両方 exit 0。Rust 型を正本にした。生成物一致の回帰も関連 suite で確認した。
- 新規 eval / service integration: exit 0、8 + 3 passed。
- 実 CLI: exit 0、1 passed / 17 filtered。
- 実 MCP: exit 0、1 passed / 10 filtered。
- 独立 worker Expression job: exit 0、1 passed / 15 filtered。
- CLI jobs を直列にした診断: exit 0、16 passed / 0 failed。後述の既定並列失敗を、この成功で置き換えない。

関連 crate の CPU / reference 回帰（GPU tests 5 件を明示除外）:

```bash
CARGO_BUILD_JOBS=3 cargo test -p kronello-model -p kronello-animation -p kronello-eval -p kronello-store -p kronello-template -p kronello-service -p kronello-render -p kronello-vector --locked -- --skip gpu_
```

exit 0、doctest を含め **284 passed / 0 failed / 1 ignored / 5 filtered**。render の 4 GPU tests と service `integration_query` の 1 GPU test を skip した。既定 ignored は store の snapshot policy benchmark。新規 eval 8 / service 3、既存 model / animation / eval / render / service / store / template / vector 回帰、および生成 schema 一致を含む。GPU skip は GPU 成功を意味しない。

最終静的検証:

```bash
CARGO_BUILD_JOBS=3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

すべて exit 0。Clippy は新規 CLI job / machine / MCP stdio のテスト target を含めた全 workspace / all targets。最終レビューで要求値を memo から clone せず move するよう修正し、追加の出力 payload が式のメモリ課金外で複製されないようにした。関連 suite / transport / job / 静的検証はこの最終コードで再確認した。ログは注入された TMPDIR の `expr-related-final.log` / `expr-clippy-final.log` / `expr-cli-final.log` / `expr-mcp-final.log` / `expr-jobs-serial-final.log` に置いた。一時ログであり、公開 artifact ではない。

## workspace gate と未解決事項

`CARGO_BUILD_JOBS=3 cargo test --workspace --locked` は実行し、exit 101。CLI jobs の既定並列実行で `cancel_running_and_queued_jobs_cleans_temporary_output` と `worker_checks_saved_schema_semantics_features_and_input_hash` が各 60 秒で timeout、13 passed / 2 failed。前者は Running / cancel_requested=true、後者は Queued のままで、原因は未確定。これらの既存 test 本体 / job scheduler は変更していない。GPU suite へは到達しなかった。以後の EXPR 依存診断・capability 追加と新規 job test を含む最終状態について、host での既定並列 gate は **pending host run**。

初回の関連 suite は、既存の非アクティブ試験が欠落 Expression を sentinel として使っていたため compile-time missing-source 診断で 2 件失敗した。欠落 ID を既知の有効な未対応 Modifier に置換し、非アクティブ部分を実行しないという回帰の意味を維持した。次の実行は固定 font fixture が未取得で render の 15 tests が失敗したため、manifest-pinned fixture を取得した。これらの途中の結果は成功件数に含めない。

Git metadata は sandbox の読み取り専用境界にある。2026-10-04 の supervisor 追記に従い、stage / commit は supervisor に渡す。worker の commit はない。禁止された backlog / ADR index / open questions / docs index / design-system は編集していない。

host で実行する必要があるコマンド（期待: exit 0、すべての非 ignored test が成功）:

```bash
CARGO_BUILD_JOBS=3 cargo fmt --all --check
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
git diff --check
```

レビュー後の stage / commit も **pending host run**。推奨 commit message は `Implement bounded canonical expression evaluation`。worker が実行していない host / GPU / codec / Windows / Linux の成功は主張しない。

## 公開契約と文書

`expression_set: {expression: {id, version, value_type, budget?, nodes}}`、既存 `property_source_set.source = {kind: "expression", value: ExpressionId}`、省略可能な `Project.expressions`、capabilities feature `expression`、`SemanticVersions.expression` を追加した。既存 operation registry の操作数・SQLite schema / user_version・Cargo dependencies / lock は変更していない。node variant と正確な payload は [08 API](../architecture/08-api-cli-mcp.md#m3-expr-001-の実装範囲)、予算・scope・noise の意味は [ADR-0058](../adr/0058-bounded-canonical-expression-ast.md) を参照。

エラー code は `EXPRESSION_BUDGET_EXCEEDED` を追加し、`EVALUATION_ERROR`、`PROPERTY_DEPENDENCY_CYCLE`、`UNSUPPORTED_FEATURE`、Command decode の `INVALID_REQUEST`、編集対象 / source catalog の `INVALID_EDIT` を使う。既存の `REVISION_CONFLICT` / `IDEMPOTENCY_KEY_REUSED` / `UNDO_CONFLICT` 契約を継承する。

supervisor に依頼: ADR README と docs README に ADR-0058 / 本記録を追加し、backlog status は host の gate とレビュー後に判断・render する。OQ-17 は未解決のまま維持する。DataAsset、動的な過去 Property sample、汎用 vector arithmetic、連続補間 noise、GUI / parser は後続設計であり、このタスクで成功を主張しない。

## 変更ファイル

追加:

- `crates/kronello-model/src/expression.rs`: AST / budget / 型・正規木の検証 / 静的依存。
- `crates/kronello-eval/src/expression.rs`、`crates/kronello-eval/tests/expressions.rs`: 有界 interpreter と 8 tests。
- `crates/kronello-service/tests/expressions.rs`: 共通 API / 保存 / render の 3 tests。
- `docs/adr/0058-bounded-canonical-expression-ast.md`、`docs/testing/expr-001.md`: 決定と本記録。

変更:

- model: `src/lib.rs`、`src/project.rs`。
- eval: `src/graph.rs`、`src/lib.rs`、`src/sequence.rs`、`tests/evaluation.rs`。
- service: `src/api.rs`、`src/edit.rs`、`src/query.rs`、`tests/api.rs`、`tests/nle.rs`。
- render: `src/snapshot.rs`、`tests/render.rs`。
- store: `src/store.rs`（optional expression 集合の ID patch materialization）。
- template: `src/lib.rs`（edition identity に AST と参照 Curve を含む）。
- CLI: `tests/jobs.rs`、`tests/machine.rs`。MCP: `tests/stdio.rs`。vector: `tests/derivation.rs`（新しい snapshot field の空 catalog）。
- `docs/architecture/03-property-animation.md`、`docs/architecture/08-api-cli-mcp.md`。
- `schemas/project-v1.schema.json`、`schemas/api-v1.schema.json`（生成物）。

Cargo manifests / Cargo.lock、`kronello-animation` の実装、CLI / MCP の実装本体、supervisor 所有ファイルは変更していない。

## host での実行結果（supervisor）

2026-10-04、Apple Silicon（Metal）の host で supervisor が実行した。

```sh
python3 scripts/fetch_fixtures.py && python3 scripts/fixtures.py generate
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=4 cargo test --workspace --locked
```

すべて exit 0。workspace test は 490 passed / 0 failed / 4 ignored（GPU / FrameBridge を含む）。
sandbox で 60 秒の timeout になった `cancel_running_and_queued_jobs_cleans_temporary_output` と
`worker_checks_saved_schema_semantics_features_and_input_hash` も host では成功した。
sandbox の失敗は、三つの worker が同時に build・test していた負荷によるものと判断する。
