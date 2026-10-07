# SERVICE-002 の検証

現在の状態（2026-10-06）: SERVICE-002はM3の受け入れ範囲で`done`。最終統合・実機検証と保証外の範囲は [M3統合受け入れ](m3-acceptance.md) と本書の後続記録を参照する。以下の初回worker記録にある「未コミット」「pending host run」は、その記録時点の状態であり、現在の未完了判定ではない。

## 初回実装とその後の検証履歴

対象: `.worktrees/m3-service2`、branch `m3-service2`、基点
`87fd361118989fe2e0039efc58c1fb24c7af9c16` からの未コミット変更。
2026-10-05、macOS / arm64、Rust 1.95.0、GPU / hardware codec なしの worker sandbox。
Cargo は注入された共有 CARGO_HOME / CARGO_TARGET_DIR、managed TMPDIR、CARGO_BUILD_JOBS=3 を使用する。
仕様は [ADR-0071](../adr/0071-project-change-plans-and-modifier-edits.md)。

## 受け入れ条件と証拠

| 条件 | テスト | 内容 / 状態 |
|---|---|---|
| 1. create / import の計画・保存後の再送 | CLI `project_plans_and_durable_retries_from_real_cli` | 実 binary の各呼び出しを独立 process とし、subcommand / tagged request の計画一致、親 path 正規化、plan の未作成、hash 不一致、no overwrite、保存後・後続 import 後の元結果再送、異なる payload 拒否、古い revision、stdout 全体の単一 JSON / typed Response と exit code を確認。成功 |
| 1. create の出力予約・同時実行 | CLI `concurrent_create_publication_and_import_receipts_from_real_cli` | 二 binary process が stdin 待機した状態から要求を送る。同一 target / 別 key の create は一つだけ成功、敗者 PROJECT_EXISTS。未公開 staging が残らない。二 import の同一 key / payload は同じ結果かつ event は一つだけ。任意の既存ファイルも上書きしない。成功 |
| 1. publication 失敗の cleanup | service unit `failed_publication_cleans_fully_initialized_staging_and_preserves_competitor` | publication 直前の hook で target が未存在、staging が revision 1 と receipt を持つことを検査。競合 target を作って publication を失敗させ、競合 bytes と staging cleanup を確認。成功 |
| 1. hash・compaction・opaque | service `project_plans_bind_target_and_snapshot_and_receipts_survive_compaction`、`project_plans_preserve_opaque_data_and_reject_client_history_and_invalid_keys` | 別 target hash 拒否、同 revision の別文書への置換拒否、元 event を compact しても元 ProjectInfo を再送、再送時の SQLite 本体 bytes 不変、opaque JSON の候補・保存・export 保持、1..256 UTF-8 bytes の key、client history fields 拒否。成功 |
| 2. Modifier の型付き変更・配列順・inverse・selective Undo | service `modifier_order_inverse_conflicts_and_selective_undo` | insert / replace / remove / reorder、順序付き配列全体 patch と inverse replay、同 Property・主値源・active inverse の UNDO_CONFLICT と競合 event、無関係 Property を保持する Undo、Redo、receipt replay。成功 |
| 2. 無効な Modifier 編集 | service `modifier_commands_reject_invalid_ids_order_versions_and_raw_history_fields` | 重複 ID、欠落 ID、index 範囲、zero version、不完全 / 重複 permutation、欠落 object / Property の INVALID_EDIT、拒否後の文書・revision 不変。成功 |
| 2 / 3. Expression と保存・評価境界 | service `unsupported_modifiers_preserve_storage_but_block_evaluation_and_final_render` | EXPR-001 の expression_set / property_source_set と enabled な未対応 Modifier を同 batch で保存。export は保持し、sample / frame / sequence は UNSUPPORTED_FEATURE、sequence の出力を残さない。Undo / Redo で式と chain を復元。disabled へ明示変更した場合だけ式の0.4を sample / CPU render、Undo 後は再び拒否。成功 |
| 3. 入口の安全性・共有 schema | service API `every_request_payload_and_envelope_matches_schema_and_denies_execution_fields`、`actual_results_for_every_command_match_envelope_and_registry_schemas`、`public_schema_matches_and_all_registry_schemas_are_safe`、`all_filesystem_boundaries_reject_uris_before_access` と新 project / Modifier tests | 38操作の envelope / payload / 実際の結果を Rust 生成 schema と照合し、raw patch / mutations / inverse / changed_keys、実行 field、asset URI を拒否。Project schema は変更なし。成功 |
| 共通 registry の MCP transport | MCP `project_plans_receipts_and_modifiers_use_registry_tools` | 実 stdio process の tools/list に新 Query と read-only metadata、実 create / import plan と結果 schema、保存済み receipt 再送、型付き Modifier 保存と sample の UNSUPPORTED_FEATURE。成功 |

service の新テストは `tests/project_changes.rs` / `tests/modifiers.rs`、publication unit は
`src/project.rs`。CLI は既存 `tests/machine.rs`、MCP は既存 `tests/stdio.rs` に追加した。
既存 service tests の CreateRequest / ImportRequest initializer に optional fields を補い、
API / CLI の操作数を36から38へ更新した。通常の既存 editing / Expression / template 回帰も実行対象。

## 再現コマンド

cwd は指定 worktree root。共有 Cargo cache / managed TMPDIR をそのまま使う。

```sh
export CARGO_BUILD_JOBS=3
python3 scripts/fetch_fixtures.py
python3 scripts/fixtures.py generate
python3 scripts/fixtures.py check --generated target/fixtures/generated
cargo test -p kronello-service --test modifiers --locked
cargo test -p kronello-service --test project_changes --locked
cargo test -p kronello-service --lib --locked failed_publication_cleans_fully_initialized_staging_and_preserves_competitor
cargo test -p kronello-cli --test machine --locked project_plans_and_durable_retries_from_real_cli
cargo test -p kronello-cli --test machine --locked concurrent_create_publication_and_import_receipts_from_real_cli
cargo test -p kronello-mcp --test stdio --locked project_plans_receipts_and_modifiers_use_registry_tools
KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --test api --locked
KRONELLO_SCHEMA_UPDATE=1 cargo test -p kronello-service --test nle_schema --locked
python3 scripts/generate_swift_api.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
KRONELLO_STATE_ROOT="$TMPDIR/service2-state" cargo test --workspace --exclude kronello-gpu --exclude kronello-framebridge --locked -- --skip gpu_
python3 scripts/generate_swift_api.py --check
git diff --check
```

fixture の最初の offline 照合は Noto Sans CJK JP 不足で失敗した。
manifest の固定 HTTPS URL から取得し、size / SHA-256 照合に成功した。
初回 workspace は既存 CLI jobs test に必要な cfr-24-1.nut が未生成で失敗した。
既存 fixtures.py generate / check により9 media fixturesを生成・decode、16 fixture entries /
9 scenes の検証に成功してから再実行した。MCP test の stderr に typed error を期待する
修正前 binary が使われた中間 workspace run も失敗し、最終 run と区別する。
Project schema の再生成は no-op（変更なし）、API schema と GeneratedAPI.swift は更新した。

## Worker の実行結果

最終 command はすべて実行済み。結果:

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- 指定 CPU workspace command: exit 0、doctests を含む **534 passed / 0 failed / 1 ignored / 8 filtered**、
  81 suite。既存の ignored は snapshot_policy_evaluation。GPU / FrameBridge crate は除外し、
  gpu_ に一致する8 test を実行していない。
- 新規9 test はすべて最終 workspace run で実行・成功。Modifier 3件、Project 2件、
  publication unit 1件、CLI 2件、MCP 1件。
- API integration: 11 passed、CLI machine: 22 passed / 1 filtered、MCP stdio: 13 passed / 1 filtered。
  通常モードの API / NLE schema 一致テストも成功。
- 指定の schema 更新 command: API 11 passed、NLE schema 1 passed。Project schema は no-op。
- `python3 scripts/generate_swift_api.py` / `--check`: exit 0。
- `git diff --check`: exit 0。変更禁止ファイル・Cargo.lock・Project schema の diff は空。
- 最終 run 前後の変更対象コード・API schema・生成 Swift（21ファイル）の SHA-256 は一致。

最終ログは managed TMPDIR の service2-workspace.log / service2-clippy.log、
schema 更新ログは service2-schema-api.log / service2-schema-nle.log、個別テストは
service2-publication.log / service2-mcp.log。worker の一時記録であり配布 artifact ではない。

初期の試行では、Modifier test の active inverse 競合を
誤って非競合と期待した2件、read-only SQLite sidecar を staging と数えた CLI assertion、
MCP の error envelope の test assertion、Clippy の test assertion / item 配置に失敗した。
既存 Undo / sidecar / error envelope 契約に合わせてテストを修正し、Clippy 指摘を修正した。
これらを成功件数に含めず、最終結果と区別する。

## 変更ファイル

- `crates/kronello-service/src/project.rs`（新規）、`api.rs`、`edit.rs`、`lib.rs`、`wire.rs`。
- `crates/kronello-service/tests/project_changes.rs` / `modifiers.rs`（新規）、`api.rs`。
  既存 `editing.rs` / `expressions.rs` / `media.rs` / `nle.rs` / `nle2.rs` / `template.rs` は
  CreateRequest / ImportRequest initializer の optional fields だけを補った。
- `crates/kronello-store/src/store.rs` / `lib.rs`。
- `crates/kronello-cli/src/main.rs` / `tests/machine.rs`、`crates/kronello-mcp/tests/stdio.rs`。
- `schemas/api-v1.schema.json`、`apps/macos/Sources/KronelloCore/GeneratedAPI.swift`。
  既存 enum variant の順序を維持し、新 variant を末尾に加えた。
- `docs/adr/0071-project-change-plans-and-modifier-edits.md`（新規）、本書（新規）。
- `docs/architecture/01-data-model.md` / `03-property-animation.md` / `08-api-cli-mcp.md` /
  `09-storage-concurrency.md`。

Project schema の生成は unchanged / no-op。Cargo.lock、model / eval の本体、backlog、ADR index、
open questions、docs/README.md、design-system は unchanged。開始時の worktree は clean だった。

## pending host run

```sh
CARGO_BUILD_JOBS=3 cargo test --workspace --locked
```

期待結果: GPU / FrameBridge を含む全 workspace tests / doctests の exit 0。
worker はこの command を実行しておらず、GPU 成功・Metal golden・hardware codec・
GUI の SwiftPM build / 実画面・Windows / Linux・network FS・電源断・強制 kill 後の staging 回収を
検証済みと扱わない。明示 cpu-reference の成功は GPU の代替証拠ではない。
backlog / ADR index / open questions は supervisor の指定に従って変更していない。
