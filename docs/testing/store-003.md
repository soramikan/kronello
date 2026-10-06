# STORE-003 の検証

STORE-003 は **`in_progress`**。条件 1 の比較評価・採否の記録は完了した。条件 2 は iCloud Drive のみホストで確認済み。条件 3 の Linux / Windows 競合・強制終了回復は2026-10-06の実 CI で確認済みだが、Dropbox・ネットワーク FS は未確認であり、タスク全体の受け入れ完了ではない。

## 適応的 snapshot の比較

worktree のルートで実行する。通常のテストでは測定を `ignore` にし、明示実行だけで行う。

```sh
cargo test -p kronello-store --locked --test snapshot_policy snapshot_policy_evaluation -- --ignored --exact --nocapture
```

`crates/kronello-store/tests/snapshot_policy.rs` は同じ編集列を固定周期と候補の DB に適用する。候補は初期・64 revision 周期に加え、直前の完全 snapshot 以降の保存済み mutations JSON の UTF-8 byte 累計（今回の patch を含む）が現在 Project JSON の UTF-8 byte 数を**超えた**時に完全 snapshot を追加する。inverse / changed keys は閾値に含めず、各 snapshot 後に累計をリセットする。等しい時には追加しない。

合成履歴は次の 3 ケース。UUID はケース構築時に一度割り当て、両方式で同じ編集内容を使う。初期空文書の UUID だけは別だが byte 長は同じである。

- `many_small`: 初期空文書の名前を `edit 0000` 等に変更する 256 回の小さな member patch。
- `few_huge`: 同一 ID の文書を 12 回 root import。名前に日本語 32,768 組（約 192 KiB）を保存する大きな patch のストレスケース。実作品の名前長の推奨ではない。
- `template_heavy`: 32 個の既知 `TemplateDefinition`、128 個の既知 `TemplateInstance` と参照する Composition を持つ文書を import し、instance 集合の duration を一括変更する 31 回の array `Set`。合計 32 回。これは保存層の workload で、Service のテンプレート公開・意味検証を実行する試験ではない。

全 revision の完全文書を適用直後に参照として保持し、close / 再 open 後の `snapshot_at` と照合する。初期を含む 257 + 13 + 33 = **303 revision / 方式**、両方式で 606 revision。1 周目で warm-up し、その後 3 周の各復元を計時して毎回参照と照合する。snapshot 行・最大 / 平均再適用 patch 数も調べる。

以下は 2026-10-04、macOS arm64、Rust 1.95.0、debug build、HEAD `0775916` に本変更を加えた worktree での実測。managed `TMPDIR` のローカル DB を使用。候補の追加 INSERT は `migrate_schema(1, ...)` による評価用の別 transaction で、production の保存方針を変更しない。時間は各周の平均復元時間の中央値で、初期 revision は時間集計から除く。最終ファイルは全 connection を close / checkpoint してから測る。

| 履歴 | 方式 | snapshot 数 | snapshot payload byte | 最終 DB byte | 論理書き込み byte | 最大 / 平均 replay patch | 復元平均 ms |
|---|---|---:|---:|---:|---:|---:|---:|
| many_small | fixed64 | 5 | 661 | 94,208 | 85,257 | 63 / 31.500 | 0.810 |
| many_small | adaptive | 89 | 11,917 | 110,592 | 96,513 | 2 / 0.984 | 0.060 |
| few_huge | fixed64 | 1 | 125 | 4,759,552 | 6,889,831 | 12 / 6.500 | 83.126 |
| few_huge | adaptive | 13 | 2,360,969 | 7,127,040 | 9,250,675 | 0 / 0.000 | 7.116 |
| template_heavy | fixed64 | 1 | 125 | 1,454,080 | 2,532,810 | 32 / 16.500 | 170.818 |
| template_heavy | adaptive | 17 | 579,437 | 2,043,904 | 3,112,122 | 1 / 0.500 | 13.674 |

論理書き込み量は初期 document + snapshot、各 revision の current document 更新、event の mutations / inverse / changed keys の UTF-8 payload、UUID 2 個の 72 byte と revision の 8 byte、保存する snapshot payload を累計したもの。receipt はこの workload にはない。SQLite row / index / page / journal の overhead、WAL の物理書き込み、fsync 回数は含めない。従って物理 write amplification の実測とは呼ばない。候補の別 transaction の書き込み時間も比較していない。

候補 / 固定の論理書き込み比は 1.132 / 1.343 / 1.229、DB 容量比は 1.174 / 1.497 / 1.406。復元短縮は確認したが、root `Set` が毎回 snapshot を作り、履歴自動削除のない運用では容量増加が蓄積するため既定採用を見送る。保存方針と最大 63 patch の上限は維持する。[ADR-0052](../adr/0052-snapshot-policy-evaluation.md) を参照。

最終 workload の測定は 1 test passed、exit 0、33.31 秒。初回は template ケースを 96 回で実行して 1 test passed、exit 0、133.22 秒だったため、短時間で再現できる 32 回に縮めて再測定した。上表は後者の結果だけを記載している。実作品 / release build / Linux / Windows の性能、採用実装の atomicity を検証した結果にはしない。

## 実同期フォルダ・ネットワーク FS のホスト手順

example は**既存ディレクトリ**と期待する判定 `local` / `sync` / `network` を受け取る。実際の同期クライアントが管理するフォルダ、または実マウントを指定する。名前だけ似せたローカルフォルダを実サービスの検証として扱わない。

```sh
cd /Users/sora/Repositories/soramikan/kronello/.worktrees/store3
cargo run -p kronello-store --locked --example sync_folder_check -- "$HOME/Library/Mobile Documents/com~apple~CloudDocs" sync
```

Dropbox があるホストでは、実際の管理場所を確認して指定する（File Provider の例）:

```sh
cargo run -p kronello-store --locked --example sync_folder_check -- "$HOME/Library/CloudStorage/Dropbox" sync
```

実ネットワークマウントの例（実在する mount path に置き換える）:

```sh
cargo run -p kronello-store --locked --example sync_folder_check -- /Volumes/actual-network-share network
```

期待する成功 JSON は `detected_location=sync` または `network`、`auto_safe_mode=true`、`journal_mode=delete`、`second_process_auto=PROJECT_LOCKED`、`second_process_force_normal=PROJECT_LOCKED`、`reopened=true`、`cleaned_up=true`。成功時は stdout に一つの JSON、失敗時は stderr に理由と非ゼロ exit。期待する場所の分類と異なれば失敗し、強制安全モードでその失敗を隠さない。

example は指定先に一意名の専用一時サブディレクトリを作り、その中で新規 project を `Auto` で開く。別の実プロセスで `Auto` と `ForceNormal` を試し、両者の `PROJECT_LOCKED` を確認する。子を wait し、holder を close した後に再 open / close し、専用サブディレクトリだけを清掃する。既存 project は開かず、リネーム・削除しない。プロジェクト外の OS 一時ロックファイルは ADR-0046 に従い unlink しない。

ホスト実行者は OS / 同期クライアントまたは FS の種類・版、実際の canonical directory、コマンドの stdout / stderr、exit code、日時を下表に追記する。iCloud Drive は supervisor がホストで実行した。Dropbox・ネットワーク FS はこのホストに無く未記入。

| 対象 | 状態 | 未確認の理由 | OS / client / FS | 日時 | 判定 / safe / journal | 別プロセス結果 | exit / 証拠 |
|---|---|---|---|---|---|---|---|
| iCloud Drive | 確認済み | — | macOS 27.0 (Apple Silicon) / iCloud Drive / APFS。`~/Library/Mobile Documents/com~apple~CloudDocs` | 2026-10-04 | `sync` / safe=`true` / journal=`delete` | Auto・ForceNormal とも `PROJECT_LOCKED`。再 open・清掃成功 | exit 0。supervisor がホストで `cargo run -p kronello-store --locked --example sync_folder_check -- "$HOME/Library/Mobile Documents/com~apple~CloudDocs" sync` を実行 |
| Dropbox | 未確認 | このホストに同期クライアント・管理フォルダがない | | | | | |
| ネットワーク FS | 未確認 | このホストにネットワークマウントがない | | | | | |

この手順は同一ホスト内の二プロセスを調べる。異なるホスト間のロック・同期競合、同期完了・オフライン時・同時 upload の整合性までは試験しない。非標準同期先・Windows の network drive letter 等の未検出は記録し、運用時は ADR-0046 の `ForceSafe` を使う。FS が必要なロックを提供しない場合の保証はない。

## ローカル対照試験

```sh
cargo test -p kronello-store --locked --example sync_folder_check local_directory_control_uses_real_detector_and_cleans_up -- --nocapture
```

2026-10-04 の sandbox macOS arm64 実行: 1 passed、exit 0。実検出器による `local`、`Auto` の `safe_mode=false` / `journal_mode=wal`、別プロセスの Auto / ForceNormal が両方 `OPENED` を確認した。その後に**別の対照条件として** ForceSafe holder を作り、別プロセスの ForceNormal が `PROJECT_LOCKED` になることを確認した。再 open・清掃は true、親ディレクトリの残存ファイル数は 0。

この試験は example の同じ `check` / `probe` 関数を test harness から呼ぶ。子プロセス入口は harness 用であり、通常の `cargo run` の引数処理・`--probe` 入口は compile 確認のみ。ホストでは上記 `cargo run` を実行する。

## Linux / Windows の競合・強制終了回復

各 OS の checkout root から、GPU・FFmpeg の実行を必要としない次のコマンドを実行する。shell 固有の `kill` / `sleep` / `mv` は使用しない。

```sh
cargo test -p kronello-store --all-targets --locked
cargo clippy -p kronello-store --all-targets --locked -- -D warnings
```

| 条件 | `tests/storage.rs` の試験 | 確認内容 |
|---|---|---|
| 二 writer の競合 | `separate_process_writers_serialize_and_reject_one_stale_base` | 同じ base を読んだ二子を file barrier で同時解放。成功一つ・REVISION_CONFLICT 一つ |
| 新規 DB の競合 | `creation_is_serialized_when_separate_processes_open_a_new_project` | 初期化も直列化し、event / revision が一つだけ増える |
| 安全モードの別プロセス排他 | `safe_mode_locks_out_other_processes_even_with_normal_override` / `safe_override_cannot_exclude_an_existing_normal_process` | 相反モードの PROJECT_LOCKED と close 後の open |
| WAL 強制終了 | `killed_process_recovers_committed_wal_and_discards_inflight_write` | commit 済み WAL を保持、未 commit document / revision を破棄、再書き込み・最後の close で sidecar 清掃 |
| DELETE journal 強制終了 | `killed_safe_process_releases_lock_and_rolls_back_inflight_write` | cache spill を強制し、hot journal の magic を確認してから kill。commit 済み変更を保持、未 commit の実 DB page 書き込みを rollback、モードロック解放、safe 再書き込み、normal 再 open、journal 清掃 |

`std::process::Command` は `current_exe()` の test actor を起動し、パスは `OsStr` のまま環境変数に渡す。ready / release file の存在を期限付きで待つ。強制終了は `Child::kill()`、終了確認は `wait()` を使う。子を reap し、全 SQLite / project handle を閉じてから復旧・清掃するため、Windows の live handle による削除拒否を避ける。実行中の DB の rename / replace は行わない。symlink の追加チェックだけは `cfg(unix)` で、プロセス競合・回復の試験には OS 除外がない。

2026-10-06の [CI run 37426096876](https://github.com/soramikan/kronello/actions/runs/37426096876) で実行した。
head `153922384937f41c697da825948daf903f312bc6`、実 checkout merge
`78fe20300f87b712ce9b6565d2aa6edb5c1250b5`。Linux は workspace tests、Windows は
現在の `Windows (full CLI/MCP and LGPL media)` job（60分上限）の
`cargo test -p kronello-store --all-targets --locked` に保存層検証を含む。
旧15分の保存層専用 job は現在の構成ではない。両 OS で上表の6つの実プロセス試験がすべて `ok`、
`tests/storage.rs` はそれぞれ35 passed / 0 failed / 0 ignored。
ローカル保存先は `target/ci-fix/freshness-37426096876/{Linux-X64,Windows-X64}.log`、
job ID は Linux `112145981202`、Windows `112145981134`。
CI はローカル filesystem の競合・回復証拠であり、実同期サービス・ネットワーク mount の証拠ではない。

| 実行 OS | 状態 | 証拠または未確認の理由 |
|---|---|---|
| macOS arm64 | ローカル確認済み | unit 2 + storage 33 + threshold 1 + example 2 = 38 passed、測定 1 ignored。測定は別途明示実行して passed |
| Linux | CI確認済み（2026-10-06） | storage 35 passed / 0 failed / 0 ignored。実二writer・初期化競合、安全モード排他、WAL / DELETE journal kill回復成功 |
| Windows | CI確認済み（2026-10-06） | storage 35 passed / 0 failed / 0 ignored。実二writer・初期化競合、安全モード排他、WAL / DELETE journal kill回復成功 |

## 初回ローカル検証コマンド（2026-10-04）

```sh
cargo fmt --all
cargo check -p kronello-store --all-targets --locked
cargo clippy -p kronello-store --all-targets --locked -- -D warnings
cargo test -p kronello-store --locked --all-targets
python3 scripts/backlog.py render
python3 scripts/backlog.py check
git diff --check
```

crate の check / Clippy は修正後 exit 0。初回 check は評価器の rusqlite `u64` bind / read が対応型でないため exit 101、`i64` への変換で修正した。初回 Clippy は評価器の大きな enum variant を指摘して exit 101、`Box<Project>` に修正した。保存層の production コード・公開 API・schema・Cargo.lock は変更していない。STORE-002 の旧ファイル・revision / inverse / idempotency / compact・破損検出のテストは引き続き成功した。workspace 全体の Cargo 検証は委任範囲外として実行していない。
