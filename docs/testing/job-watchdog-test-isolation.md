# Windows CIの短期限watchdog試験の隔離

PR #21のhead `8442de17` のWindows job `115004706877`（run `38307414378`）は、`heartbeat_deadline_exits_worker_and_recovers_interrupted`で失敗した。最初の`wait(..., Running)`が既にInterruptedのrecordを取得しており、意図した`BEGIN IMMEDIATE`によるheartbeat停止はまだ始まっていない。最後のheartbeatから約2.979秒後に期限切れと判定され、fixtureの1秒timeoutを超えていた。ログは`target/perf-windows-failure.log`に保存した。実workerのcleanupは成功し、orphanは0だった。

同じtest binaryではFULL同期SQLite connection lifecycle stress（2000 writesと内部並行writer/open-close）が他のprocess試験と同時に動く。短期限watchdog fixtureは、無関係なCPU/disk負荷下で正常heartbeatが1秒以内に継続するという未保証の前提を置いていた。

試験ハーネスに共有/排他のload guardを追加した。通常fixtureはread guardで引き続き互いに並行実行し、1秒watchdog fixtureだけwrite guardで全childのreap・store closeまで隔離する。stress child内部の並行処理と2000 writes、FIFO/複数slot/実process競合は削減しない。productionコード・heartbeat timeout1秒/interval50ms・DB lockによる停止・exit確認5秒・Interrupted/log/未公開のassertionは維持した。

修正後のjobs全feature testsは36 passed / 0 failed / 1 ignored（`target/perf-windows-watchdog-isolation-tests.log`）。all-targets/all-features Clippyも成功（`target/perf-windows-watchdog-isolation-clippy.log`）。これはmacOSでのharness回帰であり、Windowsの修正確認はPRの新しいCI結果で行う。元のWindows失敗をローカル成功に読み替えない。
