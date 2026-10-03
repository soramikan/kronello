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
- `ordered_modifiers_replacement_undo_redo_and_periodic_replay`: leaf node を同じ node / Property / Modifier ID のまま remove → add し、`[A, B]` → `[B, A]` を保存する。UUID 順と異なる複数 Modifier の追加、plan 候補・適用・Undo / Redo の厳密な順序、revision 64 の周期 snapshot とその後の inverse patch 再生、`compact(63)` / `compact(65)` 後の復元と receipt を確認する。
- service unit `ordered_arrays_with_id_members_are_atomic_and_invert_exactly`: 順序付き配列の位置ではなく配列全体を置換し、inverse / Redo が元の順序を復元することを確認する。
- service unit `unordered_collections_keep_member_patches_for_selective_undo`: UUID member patch を許す全7種類の collection で、保存順序だけの変更を無視し、値変更は member patch にする。逆操作が別 member の独立した後続変更を保存することを確認する。
- service unit `concurrent_compact_between_undo_validation_and_commit_rejects_target`: 異なる Property を変更する実 service plan の patch / changed keys で revision 2・3 を作る。Undo の read validation を終えた直後に別 connection が `compact(3)` を commit し、revision 2 の対象を消す。revision が 3 のままでも、書き込み transaction 内の再検証で `EVENT_NOT_FOUND`。compact 後の文書・revision・history を変更せず、Undo receipt も作らない。
- service unit `concurrent_undo_retry_receipt_precedes_compacted_target_validation`: read validation 後、別 connection が同じ Undo を commit して対象を compact した場合は、transaction 内の receipt 照合を優先して元 Event を返す。
- service unit `locked_undo_validation_rejects_conflicts_and_already_undone_targets`: 保存済み inverse を書き込み経路へ直接渡しても、transaction 内で `UNDO_CONFLICT`（event ID / keys）と `EVENT_ALREADY_UNDONE` を再検証し、文書・revision・history・receipt を変更しない。
- `concurrent_service_apply_and_undo_are_serialized`: barrier 付き service threads の同じ key、異なる key、Undo と Apply の競争。文書と history の revision が一度だけ進む。

## 実装上の決定

一つの plan / apply は一つの Event。ID は caller が事前に確保し、plan 中に生成しない。計画 hash は project ID・正規化 revision・commands・候補・生成 patch / inverse / changed keys（hash 自体は空文字列）を canonical JSON にした SHA-256。適用結果は保存 Event 全体とし、再送の応答に現在 revision や replayed flag を加えない。

receipt は operation・session・正規化 revision・commands / hash、または undo target を照合する。project path は project 内 key の名前空間なので payload には入れない。exact retry を古い revision の拒否より先に扱い、transaction 内でも重複判定する。SQLite schema / user_version は変更せず、旧 receipt の raw ApplyRequest と新 `service_payload` を区別する。

保存 patch の UUID member 指定は、順序に意味のない Project の compositions / curves / shapes / texts、Composition の nodes / properties、SceneNode の properties に限定する。モデルの保存配列を監査し、Modifier、root_nodes / child_order、時刻順の keyframe、path segment、gradient stop、text style / ruby とその他の配列は全体置換とした。ID を持つことだけでは集合とみなさない。同じ親の構造編集を競合にするため、他の順序変更を inverse で上書きしない。content / curve / definition を参照する新しい構造や source は直接 resource ID をキーに含める。評価・式・layout の間接的依存はキーに含めない。未対応意味・opaque 文書は引き続き通常編集を拒否する。

Undo の事前 read validation と同じ service 関数を、`ProjectStore::apply_with_payload_checked` の `BEGIN IMMEDIATE` 内でも呼ぶ。receipt / revision 照合後、書き込み前に対象の存在・未取り消し状態・後続 active Event のキー競合・inverse 後候補の意味を再検証する。compact は revision を進めないため、revision 照合だけでは代用できない。保存層には意味規則を移さず、immutable snapshot / events を検証 callback に渡す。SQLite schema と低水準 `apply` / `apply_with_payload` の契約は変えない。

ADR-0026 の未取り消し Event の規則を Undo Event 自体にも適用する。後続の変更を Undo した場合でも、その Undo Event は同じキーの未取り消し Event である。さらに逆操作を行うには最新の Undo / Redo Event を対象にする。失敗時に部分適用、暗黙の rebase、retry による stale 要求の書き換えをしない。

## 再現するコマンド

すべて worktree の root を cwd とする。依存は追加せず、Cargo.lock も変更していない。既存 cache の欠けた `cc-1.6.0/Cargo.toml` で最初の check が失敗したため、専用の書き込み可能な Cargo cache を使う。

```sh
export CARGO_HOME=/private/tmp/kronello-service-cargo
# P2 修正検証は空き容量不足を避けるため debug info / incremental を無効化。
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p kronello-service -p kronello-cli --locked -- --skip gpu_
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
for i in $(seq 1 10); do
  cargo test -p kronello-service --lib --locked concurrent_ || exit 1
  cargo test -p kronello-service --test editing --locked concurrent_service_apply_and_undo_are_serialized -- --exact || exit 1
  cargo test -p kronello-cli --test machine --locked concurrent_cli_same_key_returns_one_event_and_different_keys_conflict -- --exact || exit 1
done
python3 scripts/backlog.py check
git diff --check
```

## SERVICE-001 初回実装の記録（`9dce6ae` から継承）

検証日: 2026-10-03、macOS / arm64、`rustc 1.95.0`。重複した SERVICE-001 job の停止後に統合差分を確認し、以下を最終ソースで再実行した。UUID alias 拒否、patch replay / compact、receipt の revision 正規化・session 照合は継承した差分をレビューして保持した。

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- 指定の CPU workspace command: exit 0、doctest を含め **289 passed / 0 failed / 0 ignored / 4 filtered**。除外は CLI の GPU 成功テスト1件、render の GPU 成功テスト3件。service unit 1件・editing integration **15件**、CLI integration **10件**を含む。
- service の concurrency test と CLI の concurrency test をそれぞれ **10回**、全20回で各1 passed / 0 failed。最終 loop は全 command 成功時だけ終える shell の `set -e` で実行した。
- 最終検証の開始前・終了後の `edit.rs` / `editing.rs` / CLI `machine.rs` / store `store.rs` の SHA-256 が一致した。
- SERVICE-001 の状態を `done` にして backlog を再生成。`python3 scripts/backlog.py render` / `python3 scripts/backlog.py check` / `git diff --check` は exit 0。

最終 workspace log は `/private/tmp/kronello-service-workspace-final.log`、clippy は `/private/tmp/kronello-service-clippy-final.log`、反復結果は `/private/tmp/kronello-service-concurrency-final-1.log` ～ `-10.log` と `/private/tmp/kronello-cli-service-concurrency-final-1.log` ～ `-10.log`。これらは worker の一時検証記録であり配布 artifact ではない。

## 前 worker が残した P2 回帰修正の検証記録

本節は上記の初回実装時の測定と区別する。SERVICE-001 の backlog status は `done` を維持し、backlog 本体・生成文書・Cargo.lock は変更しない。

前 worker が未 commit 文書に残した測定記録を以下に保持する。今回の継続 worker の実行結果ではなく、ログの再確認もしていない。記録上の検証日・環境は 2026-10-03、macOS / arm64、`rustc 1.95.0`。その worker は未 commit 実装・回帰テストをレビューし、compact の決定的 interleaving テストを実 service plan と非空の導出キーで強化したと記録している。

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `cargo test -p kronello-service --lib --locked`: exit 0、**5 passed / 0 failed**。強化テストの初回 compile は index 型の推論不足（`E0282`）で exit 101、closure 引数に型を明記して修正後に再実行した。
- 指定の CPU workspace command: exit 0、doctest を含め **294 passed / 0 failed / 0 ignored / 4 filtered**。service unit **5件**・editing integration **16件**、CLI integration **10件**を含む。除外した GPU 成功テストは CLI の1件と render の3件。
- 上記 concurrency loop は **10回**すべて成功。各回の service unit `concurrent_` は2件、service / CLI integration は各1件、合計 **40 passed / 0 failed**（30 command）。
- 検証開始前・終了後の `edit.rs` / `editing.rs` / store `store.rs` / CLI `machine.rs` の SHA-256 が一致した。
- `python3 scripts/backlog.py check`: exit 0、60 tasks。`git diff --check`: exit 0。

前 worker が記録した workspace / clippy / unit log は `/private/tmp/kronello-service-p2-workspace.log`、`/private/tmp/kronello-service-p2-clippy.log`、`/private/tmp/kronello-service-p2-unit.log`。反復 log は `/private/tmp/kronello-service-p2-unit-concurrency-1.log` ～ `-10.log`、`/private/tmp/kronello-service-p2-concurrency-1.log` ～ `-10.log`、`/private/tmp/kronello-cli-service-p2-concurrency-1.log` ～ `-10.log`。一時検証記録であり配布 artifact ではない。

## 継続 worker の P2 回帰修正レビューと再検証

検証日: 2026-10-03、branch `m2-service-001`、HEAD `9dce6ae107541519f9017550bec8a0f5d2fd69d8` に未 commit の P2 修正を重ねた状態。macOS / arm64、`rustc 1.95.0`。作業と Cargo の cwd は `.worktrees/service` に限定した。

引き継いだ `edit.rs` / `editing.rs` / store `store.rs` と architecture の修正をレビューして保持した。根本原因は、すべての ID 付き配列を集合と扱って Modifier の順序を落としたことと、revision が変わらない compact の後に Undo 対象を再検証していなかったことである。既存の whitelist と配列全体の inverse、および receipt 照合後の `BEGIN IMMEDIATE` 内の Undo 再検証は、ADR-0026 / ADR-0030 / ADR-0046 の契約を維持する。

今回追加したのは `unordered_collections_keep_member_patches_for_selective_undo`。順序付き配列の修正を全配列の置換へ広げず、順序に意味のない全7種類の collection では ID member patch を維持し、Undo が独立した後続 member 編集を消さないことを確認した。Modifier の順序・周期 snapshot / compact の再生、決定的な Undo / compact interleaving、receipt 優先、locked validation の4 unit test と integration test は前 worker の変更として保持した。検証記録の帰属を今回の結果と区別した。store `store.rs`、`editing.rs`、architecture には今回の追加編集をしていない。

上記の環境変数と指定コマンドで、今回実際に再実行した結果:

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `cargo test -p kronello-service --lib --locked`: exit 0、**6 passed / 0 failed**。
- 指定の CPU workspace command: exit 0、doctest を含め **295 passed / 0 failed / 0 ignored / 4 filtered**。service unit **6件**・editing integration **16件**、CLI integration **10件**を含む。CLI の GPU 成功1件と render の GPU 成功3件を除外した。
- 指定の concurrency loop: **10回**すべて成功。各回の service unit `concurrent_` 2件、service / CLI integration 各1件、合計 **40 passed / 0 failed**（30 command）。各 command の exit 0 と期待するテスト件数を個別に確認した。
- 検証開始前・反復終了後の `edit.rs` / `editing.rs` / store `store.rs` / CLI `machine.rs` の SHA-256 は一致。
- `python3 scripts/backlog.py check`: exit 0、60 tasks。`git diff --check`: exit 0。補助スクリプトの最後の status 確認は誤ったキー `tasks` で `KeyError` となったが、実スキーマの `items` で再実行し exit 0、SERVICE-001 の `done` を確認した。テストや backlog check の失敗ではない。

今回のログは worktree 内の `target/service-p2-continuation/` に保存した。`fmt.log` / `clippy.log` / `workspace.log`、`unit-concurrency-1.log` ～ `-10.log`、`service-concurrency-1.log` ～ `-10.log`、`cli-concurrency-1.log` ～ `-10.log`、`backlog.log` / `diff-check.log`、`source-sha256-before.txt` / `source-sha256-after.txt`。独立実行した unit test の6件成功は terminal 出力で確認した。一時検証記録であり配布 artifact ではない。backlog 本体・生成文書・Cargo.lock・CLI test は変更せず、commit / push は実行していない。

## 検証の境界

CPU workspace 検証は GPU 成功テストを除外し、既存 render 検証では明示 `cpu-reference` を使う。GPU 実行・固定環境 golden・性能、Windows / Linux の runtime、実ネットワーク保存、GUI / MCP transport、別タスク統合後の workspace 実行は未確認。Command / Query envelope の公開 JSON Schema の正式化は API-001 が担当する。
