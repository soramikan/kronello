# STORE-002 の検証

対象は `kronello-store` の完全 snapshot 間引き、保存 patch による履歴復元、旧全 revision snapshot 形式との互換性。Command の再実行、selective undo の競合規則、サイズによる snapshot 作成（STORE-003）は対象外。

## 保存・復元の規則

新規保存の完全 snapshot は revision 0、`revision % 64 == 0`、`compact(r)` の基点だけに作る。`snapshot_at(r)` は直前の完全 snapshot を選び、連続したイベントの `Set` / `Remove` patch を最大 63 個再適用する。`restore_snapshot` はその文書を新 revision の root `Set` として保存する。各 patch 後に通常の保存と同じ文書 decode・構造検証を行い、opaque 内容の import を復元できるよう editability 判定は行わない。

復元は SQLite の一つの読み取り transaction 内で行う。`compact` は `BEGIN IMMEDIATE` 内で基点を復元・保存した後に古い行を削除し、途中の失敗は全体を rollback する。必要な patch の欠落・JSON 破損・未知 operation・適用不能・文書不適合は `StoreError::HistoryReplayFailed { revision, reason }`（`HISTORY_REPLAY_FAILED`）。周期 snapshot が欠けて 64 個以上の再適用を必要とする場合も失敗する。保持範囲外は従来どおり `SNAPSHOT_NOT_FOUND`。

SQLite schema と `user_version=1` は変えず、migration を追加しない。旧形式の全 revision snapshot は削除・変換せず、そのまま読む。完全 snapshot のある revision の復元には過去 patch を必要としない。

`history_size()` の集計式は従来と同じくイベント patch / inverse / changed keys と、現在 revision 以外の**実際に保存された**完全 snapshot の UTF-8 payload 合計。間引いた revision の仮想 snapshot や現在文書を加算しない。警告境界は 256 MiB 以上で、自動削除しない。

## 条件とテストの対応

すべて `crates/kronello-store/tests/storage.rs`。STORE-001 の既存 26 テストに 6 テストを追加した合計 32 件（子プロセス入口 `subprocess_actor` を含む）。

| 条件 | テスト | 確認内容 |
|---|---|---|
| 初期・64 revision ごと・compact 基点だけに完全 snapshot を保存 | `selective_snapshots_and_compact_bases_bound_replay_to_63_patches` | DB の実 snapshot 行を検査し、63 / 64 / 65 / 127 / 128、非周期 compact 基点、current の compact、反復 compact と再 open を確認 |
| 任意 revision の文書が全 revision 保存と一致 | `randomized_edit_histories_match_an_every_revision_reference` | 16 seed、各 193〜208 回、合計 3,208 回の変更。全 revision の参照 `Project` をメモリに保持し、保存直後の現在文書・`snapshot_at`、compact と再 open 後の保持 revision 全件、`restore_snapshot` を比較。名前変更、batch の Set / Remove、配列置換、実 Composition / Curve の import、未知意味版・opaque・大きな整数の import、ランダムな履歴復元を含む。乱数は固定 seed の xorshift で再現可能 |
| 旧全 revision 保存ファイルの互換性・非破壊 | `legacy_every_revision_snapshots_open_and_restore_without_rewriting_history` / `restore_full_snapshot_without_replaying_events` | 同じ v1 schema に旧方式の全 revision snapshot 行を明示的に作成。0〜66 の全 snapshot を参照と比較し、安全モードで読み取り open / close 前後の元ファイル byte 一致を確認。新しい restore 後も旧 snapshot 全行と `user_version=1` を保持。旧 patch が解釈不能でも完全 snapshot から復元 |
| revision・inverse・idempotency・compact の意味を維持 | `atomic_apply_persists_event_keys_inverse_receipt_and_snapshot` / `stale_revision_rejected_across_connections` / `property_source_patch_and_generated_inverse_roundtrip_real_model_data` / `compact_retains_boundary_snapshot_and_event_without_changing_document` / `snapshot_insert_and_compact_failures_roll_back_all_tables` | 既存の revision 競合、session / keys / inverse / undo linkage、receipt、基点イベントと現在文書保持を継続検証。周期 snapshot INSERT 失敗と compact の基点 INSERT・履歴 DELETE 失敗を trigger で注入し、文書・event・snapshot・receipt を rollback |
| patch の欠落・破損を型付きエラーにする | `missing_or_corrupt_replay_patches_fail_without_writes` | event 欠落、JSON 破損、未知 operation、Remove の適用不能、文書型不適合、BLOB を注入。`snapshot_at` / `restore_snapshot` / `compact` が該当 revision の `HISTORY_REPLAY_FAILED` を返し、文書・履歴量・snapshot 行を変更しない。周期 snapshot 欠落も検出 |
| 履歴警告量が保存方針と一致 | `history_size_counts_only_persisted_snapshots_across_checkpoints_and_compact` / `history_warning_threshold_is_inclusive_and_never_prunes_automatically` | 1〜65 の各 revision で独立集計と比較し、checkpoint が現在文書になる時と次 revision、current compact 後の集計、日本語・é の UTF-8 byte 数、256 MiB の包含境界を確認 |

## 再現するコマンド

worktree のルートで実行する。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude kronello-framebridge --exclude kronello-gpu --locked -- --skip gpu_
for i in $(seq 1 20); do
  cargo test -p kronello-store --locked --test storage || exit 1
done
python3 scripts/backlog.py check
```

外部 font fixture がない環境では font を必要とするテストは失敗する。除外して検証した場合は、除外した crate と未検証条件を報告し、workspace 全体の成功として扱わない。GPU を除く検証から GPU 実行の成功を推定しない。

## 検証の境界

macOS / arm64、Rust 1.95.0 でローカル検証する。Windows / Linux 実行、GPU、実ネットワークマウント・同期クライアントはこの作業の検証対象外。旧形式の互換性はテスト内で構築した v1 全 revision 保存 DB に対する検証であり、未提供の利用者ファイルの個別検証は行っていない。
