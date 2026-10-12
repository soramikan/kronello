# 全workspace検証で観測したSQLite VFS deadlock

2026-10-13、PERF-011/012の現ソースで実行した全workspace testsが`kronello-service/tests/editing.rs`で停止した。時間超過だけで再起動せず、実プロセスと短いstack sampleを確認した。

## 観測と修正

`target/perf-011-012-editing-sample.txt`では、一方のconnection closeが`unixClose`からglobal mutexを取得した後inode mutexを待ち、もう一方はWAL closeの`unixLock`から`unixIsSharingShmNode`へ入りglobal mutexを待っていた。他の11test/worker threadsもSQLite mutexを待ち、子プロセスのCPUは0%だった。使用中のbundled SQLiteは3.51.1。

[SQLite 3.51.2の修正記録](https://www.sqlite.org/releaselog/3_51_2.html)と[公式news](https://www.sqlite.org/news.html)は、3.51.0からのbroken POSIX lock検出に起因するUnix VFS deadlockの修正を記載している。観測したstackとregistry内の`unixClose`/`unixIsSharingShmNode`のlock順がこれに対応した。

`rusqlite`を0.38→0.39、`libsqlite3-sys`を0.36→0.37へ更新した。[rusqlite 0.39 release](https://github.com/rusqlite/rusqlite/releases/tag/v0.39.0)のbundled SQLiteは3.51.3で、deadlock修正とWAL reset修正を含む。CLI/jobs/storeの3利用crateとCargo.lockを揃え、storage schemaやtransaction/publicationの設計は変更していない。

確認済みdeadlockの所有する子テストだけをSIGTERMで終了し、元の全workspace runはexit101として保持した。元ログは`target/perf-011-012-final-workspace-tests.log`、成功扱いにしない。新しいrunとは別の証拠である。

## 回帰検証

- 同じ並列editingスイート: 21 passed、1.59秒（`target/perf-sqlite-editing-fixed.log`）。
- 追加`sqlite_concurrency`試験: 2 passed・subprocess helper 1 ignored、0.13秒。runtimeの最低SQLite版を確認し、16threads/4databases/800 WAL transactionのopen/commit/close競合を検証する。子プロセスの30秒deadlineはテストを無限待機させないための境界で、直列化して競合を隠さない。
- 更新後の全workspace/all-targets Clippyは成功（`target/perf-sqlite-final-clippy.log`）。fmt・生成Swift整合・backlog checkも成功した。
- store/jobs全スイートは9 suites・67 passed・0 failed・2 ignoredで成功（`target/perf-sqlite-storage-jobs-fixed.log`）。
- 全workspace testsは更新後の依存で実行中（`target/perf-sqlite-final-workspace-tests.log`）。終了確認を受け入れ残件とする。

版更新だけを完了証拠にせず、復旧・lease・receipt・idempotencyの検証を維持する。新しいperformance比較では更新後binaryを新しいhashとして固定し、以前のbinary/raw証拠を書き換えない。
