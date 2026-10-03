# STORE-001 の検証

対象は同期的な `kronello-store` 保存層。CLI / MCP の Command / Query、selective undo の競合判定、レンダー能力検証はこのテストの対象ではない。

## 再現するコマンド

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked
python3 scripts/backlog.py check
```

Schema を再生成する場合:

```sh
cargo run -p kronello-model --example project_schema --locked > schemas/project-v1.schema.json
cargo test -p kronello-store --locked
```

GPU が利用できる環境の workspace 全体の確認は `cargo test --workspace --locked`。GPU を除くコマンドの成功から GPU テストの成功を推定しない。

## 受け入れ条件とテスト

以下は `crates/kronello-store/tests/storage.rs` のテスト名。26 件には子プロセス用の入口 `subprocess_actor` も 1 件として含む。通常の単独実行ではこの入口は処理せず、下表の親テストが環境変数を設定して起動する。

| 条件 | テスト | 確認内容 |
|---|---|---|
| 1. 同一 transaction | `atomic_apply_persists_event_keys_inverse_receipt_and_snapshot` / `failure_after_document_update_rolls_back_all_tables` / `invalid_patch_is_atomic_and_inverse_reverses_a_batch` | 第 2 connection から状態・event・revision・snapshot・receipt の一致を見る。event INSERT の失敗を trigger で注入し、先に更新した document を含む全変更が rollback する |
| 2. 復元・migration | `restore_full_snapshot_without_replaying_events` / `failed_migration_is_nondestructive_including_ddl_and_internal_version` / `unsupported_versions_and_unknown_database_leave_original_unchanged` | 旧 command の解釈不能な記録でも完全 snapshot から新 revision に復元する。migration の DDL・文書・履歴・user_version の rollback と、元ファイルの byte 一致を確認する |
| 3. 単一ファイル・cache 分離 | `cache_is_outside_single_file_project_and_database_has_no_cache_tables` | OS cache path がプロジェクトの外、table は 4 個で cache table なし、閉じた後の project directory は `.kronello` 一つ |
| 4. 別プロセスの直列化 | `separate_process_writers_serialize_and_reject_one_stale_base` / `creation_is_serialized_when_separate_processes_open_a_new_project` / `stale_revision_rejected_across_connections` | 2 子プロセスが同じ base を読み barrier 後に競争し、成功一つ・REVISION_CONFLICT 一つ。別 connection でも stale write を拒否。新規 DB の同時初期化も確認 |
| 5. session・keys・inverse | `atomic_apply_persists_event_keys_inverse_receipt_and_snapshot` / `property_source_patch_and_generated_inverse_roundtrip_real_model_data` | Value / Structure 両キー、session、undo_of を保存。実 PropertySource を Constant から Curve に変え、生成した逆操作で元値に戻す |
| 6. WAL・異常終了復旧 | `last_process_close_removes_wal_and_shm` / `killed_process_recovers_committed_wal_and_discards_inflight_write` | 他プロセスが開いている間は sidecar が残り、最後の close 後に消える。commit 済み WAL と未 commit transaction がある子を kill し、前者を復元、後者を破棄して書き込みを再開する |
| 7. 安全モード | `location_detection_and_explicit_overrides_are_injectable` / `system_detector_uses_actual_filesystem_and_mode_lock_covers_symlinks` / `safe_mode_locks_out_other_processes_even_with_normal_override` / `safe_override_cannot_exclude_an_existing_normal_process` | 注入した sync / network 判定で非 WAL、override を確認。safe holder 中の別プロセスには mode に関わらず PROJECT_LOCKED。normal holder に safe override もできない |
| 8. 公開 JSON | `public_json_roundtrips_rationals_and_nested_opaque_content` / `opaque_json_retains_integer_precision_beyond_u64` / `unknown_semantic_version_is_preserved_but_not_editable` / `duplicate_public_envelope_fields_are_rejected_without_changes` / `committed_schema_matches_rust_types` / `public_schema_validates_known_and_opaque_exports` | rational decimal strings、未知の enum / 入れ子 / 大きな整数を保持して再 open / export。未知の意味で編集を拒否。Schema の生成一致と実 validator による適合・不適合を確認 |
| 9. compact | `compact_retains_boundary_snapshot_and_event_without_changing_document` / `history_warning_threshold_is_inclusive_and_never_prunes_automatically` | boundary より前を削除し boundary の完全 snapshot と event を残す。現在状態・revision と receipt の完全結果は保持し、boundary から復元できる。UTF-8 payload 量と警告の 256 MiB 境界を検証する |

## 未確認の環境

ローカル macOS でのプロセス実行を検証する。実 SMB / NFS / AFP / WebDAV マウント、実同期クライアント、Windows / Linux のプロセス実行は確認していない。これらについて、検出器の注入テストや cross target の compile を OS 実行成功として扱わない。OS のロックを正しく提供しないネットワークサービスを保証しない。

公開構造版の過去版 migration はまだない。`migrate_schema` の rollback 機構と新規 DB の初期化を検証し、具体的な旧版 migration の成功を主張しない。
