# SERVICE-001 の検証

対象: branch `m2-service-001`、`.worktrees/service` の SERVICE-001 差分。M1 の共通 service / CLI と SQLite store に、型付き編集計画・適用・receipt 再送・selective Undo・最小 history.list を追加する。検証は macOS / arm64、Rust 1.95.0 の worker sandbox で行う。MEDIA-001 / FX-001 の別 checkout の変更や、その後の統合はこの検証に含めない。

## 受け入れ条件とテスト

service のテストは `crates/kronello-service/tests/editing.rs`、binary のテストは `crates/kronello-cli/tests/machine.rs`。binary test は built `kronello` を実際に毎回別プロセスとして起動し、stdout 全体の JSON、stderr、exit code を確認する。編集では GPU 初期化も font bytes の読み出しも必要ない。

| 条件 | テスト | 確認内容 |
|---|---|---|
| 1. revision / plan_hash / idempotency_key | `deterministic_plan_hash_revision_checks_receipts_and_no_client_keys` | 同一 plan の決定性、10進 revision の正規化、plan の無変更、hash 不一致、空 key、stale plan / apply の拒否、client の changed_keys 拒否 |
| 2. 同じ payload の重複適用防止、異なる payload の拒否 | 上記、`concurrent_service_apply_and_undo_are_serialized`、`concurrent_cli_same_key_returns_one_event_and_different_keys_conflict` | 同じ key で同じ Event 全体を返す。payload / session 変更は `IDEMPOTENCY_KEY_REUSED`。同時の同じ key も一つの Event、異なる key の stale writer は `REVISION_CONFLICT` |
| 3. project 内 receipt、別プロセス再送 | `persisted_edit_receipts_replay_from_real_cli_processes_after_edits_and_compact` | real CLI で create → plan → apply → 新プロセス再送 → 後続編集 → undo の再送。元の revision / Event を返して重複変更しない。元 Event を compact しても receipt の完全な結果を返す |
| 4. selective Undo / Redo と atomic な競合拒否 | `selective_undo_preserves_other_property_redo_and_history_status`、`undo_conflict_lists_events_keys_and_rejects_whole_batch`、`invalid_candidates_and_undo_validation_are_atomic` | 同じ node の別 Property の後続変更を保存したまま inverse を新 revision にする。undo_of と history.undone、Undo を取り消す Redo。同じ Property の後続 Event は ID / keys を details に含め、文書・revision・history・receipt を変更しない。Undo 後候補の不適合も拒否 |
| 5. service が導出する Value / Structure の競合キー | `structure_conflicts_cover_parent_containers_and_object_values`、`shared_curve_edits_derive_all_consumers_and_source_creation_undo`、`typed_node_composition_instance_content_operations_roundtrip`、`composition_inverse_and_instance_reference_conflicts` | `(object, Property)`、同じ親の兄弟構造変更、構造と対象 object の値変更、旧・新の親、共有 curve の全直接消費者、後続 instance の definition 参照を確認 |

## 追加の回帰検証

- `keyframe_commands_shared_consumers_and_inverse_roundtrip`: insert / upsert / replace / remove、重複・欠落時刻、生成 inverse。
- `typed_node_composition_instance_content_operations_roundtrip`: node / subtree、reparent、独立した transform parent、reorder、composition create、instance place、実 Shape / 日本語 Text content 編集と逆操作。
- `optional_content_collection_inverse_preserves_independent_insertions`: 空の optional collection の最初の member を Undo しても別の content を残す。
- `service_commit_failure_rolls_back_document_history_and_receipt`: event INSERT trigger に失敗を注入し、先に更新した文書と receipt を含め全体が rollback する。
- `stable_id_patches_replay_and_compact_with_service_receipts`: UUID array member patch の snapshot_at 復元、Undo の再生、compact の基点と receipt 保持。
- `cross_kind_object_uuid_aliases_are_rejected_before_planning`: 異なる model kind の同じ object UUID を拒否して、object lookup / conflict の曖昧さを作らない。
- `strict_command_decoding_rejects_unknown_and_duplicate_fields`: model 型の float を含む command の往復、未知・重複 field の拒否。
- `concurrent_service_apply_and_undo_are_serialized`: barrier 付き service threads の同じ key、異なる key、Undo と Apply の競争。文書と history の revision が一度だけ進む。

## 実装上の決定

一つの plan / apply は一つの Event。ID は caller が事前に確保し、plan 中に生成しない。計画 hash は project ID・正規化 revision・commands・候補・生成 patch / inverse / changed keys（hash 自体は空文字列）を canonical JSON にした SHA-256。適用結果は保存 Event 全体とし、再送の応答に現在 revision や replayed flag を加えない。

receipt は operation・session・正規化 revision・commands / hash、または undo target を照合する。project path は project 内 key の名前空間なので payload には入れない。exact retry を古い revision の拒否より先に扱い、transaction 内でも重複判定する。SQLite schema / user_version は変更せず、旧 receipt の raw ApplyRequest と新 `service_payload` を区別する。

保存 patch は ID-bearing array の member を UUID で指定する。draw order / keyframe 配列は全体置換。同じ親の構造編集を競合にするため、他の順序変更を inverse で上書きしない。content / curve / definition を参照する新しい構造や source は直接 resource ID をキーに含める。評価・式・layout の間接的依存はキーに含めない。未対応意味・opaque 文書は引き続き通常編集を拒否する。

ADR-0026 の未取り消し Event の規則を Undo Event 自体にも適用する。後続の変更を Undo した場合でも、その Undo Event は同じキーの未取り消し Event である。さらに逆操作を行うには最新の Undo / Redo Event を対象にする。失敗時に部分適用、暗黙の rebase、retry による stale 要求の書き換えをしない。

## 再現するコマンド

すべて worktree の root を cwd とする。依存は追加せず、Cargo.lock も変更していない。既存 cache の欠けた `cc-1.6.0/Cargo.toml` で最初の check が失敗したため、専用の書き込み可能な Cargo cache を使う。

```sh
export CARGO_HOME=/private/tmp/kronello-service-cargo
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p kronello-service -p kronello-cli --locked -- --skip gpu_
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
for i in $(seq 1 10); do
  cargo test -p kronello-service --test editing --locked concurrent_service_apply_and_undo_are_serialized -- --exact || exit 1
  cargo test -p kronello-cli --test machine --locked concurrent_cli_same_key_returns_one_event_and_different_keys_conflict -- --exact || exit 1
done
python3 scripts/backlog.py render
python3 scripts/backlog.py check
git diff --check
```

## Worker の最終実行結果

検証日: 2026-10-03、macOS / arm64、`rustc 1.95.0`。重複した SERVICE-001 job の停止後に統合差分を確認し、以下を最終ソースで再実行した。UUID alias 拒否、patch replay / compact、receipt の revision 正規化・session 照合は継承した差分をレビューして保持した。

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- 指定の CPU workspace command: exit 0、doctest を含め **289 passed / 0 failed / 0 ignored / 4 filtered**。除外は CLI の GPU 成功テスト1件、render の GPU 成功テスト3件。service unit 1件・editing integration **15件**、CLI integration **10件**を含む。
- service の concurrency test と CLI の concurrency test をそれぞれ **10回**、全20回で各1 passed / 0 failed。最終 loop は全 command 成功時だけ終える shell の `set -e` で実行した。
- 最終検証の開始前・終了後の `edit.rs` / `editing.rs` / CLI `machine.rs` / store `store.rs` の SHA-256 が一致した。
- SERVICE-001 の状態を `done` にして backlog を再生成。`python3 scripts/backlog.py render` / `python3 scripts/backlog.py check` / `git diff --check` は exit 0。

最終 workspace log は `/private/tmp/kronello-service-workspace-final.log`、clippy は `/private/tmp/kronello-service-clippy-final.log`、反復結果は `/private/tmp/kronello-service-concurrency-final-1.log` ～ `-10.log` と `/private/tmp/kronello-cli-service-concurrency-final-1.log` ～ `-10.log`。これらは worker の一時検証記録であり配布 artifact ではない。

## 検証の境界

CPU workspace 検証は GPU 成功テストを除外し、既存 render 検証では明示 `cpu-reference` を使う。GPU 実行・固定環境 golden・性能、Windows / Linux の runtime、実ネットワーク保存、GUI / MCP transport、別タスク統合後の workspace 実行は未確認。Command / Query envelope の公開 JSON Schema の正式化は API-001 が担当する。
