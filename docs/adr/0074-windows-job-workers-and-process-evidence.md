# ADR-0074: Windows worker の独立起動・成果物確定と実プロセス検証

- 状態: 部分置換（[ADR-0082](0082-windows-ffmpeg-runtime.md): Windows media / CLI / MCP の実装・検証方針。OS ごとの受け入れ確認は別 gate）
- 日付: 2026-10-05
- 対象: JOB-002
- 部分置換: [ADR-0050](0050-fixed-job-execution-and-publication.md) の Windows 未実装境界、worker の競合待機、状態照会の writer 取得。固定入力、共有 API、FIFO、lease/cancel fence、保持、電源断の窓は維持する。

## 背景

Windows の親 Job Object / console / pipe から独立した worker 起動と、既存成果物を置換しない publication が必要。
既存 heartbeat の再試行だけでは、slot claim / frame checkpoint が SQLite の5秒の busy 待機で失敗する。
それとは別に、bundled SQLite 3.51.1 の Unix VFS で WAL close と同一 DB open の mutex 順序が逆転する。
supervisor が macOS の worker PID 57005（27分経過、Running / completed_frames=0）を sample し、
main の `checkpoint → update → sqlite3_close → sqlite3WalClose → unixLock → unixIsSharingShmNode → unixEnterMutex` と
heartbeat の `heartbeat → connect_with_timeout → sqlite3_open_v2 → unixOpen → findReusableFd → sqlite3_mutex_enter`
が同時に停止していることを確認した。これは SQLITE_BUSY を返す file lock 待機と異なり、busy_timeout では解消しない。
この sample は supervisor 提供の既存 binary の証拠で、この実装の検証結果としては扱わない。
テストが panic / timeout で終わると gate を待つ detached worker が残り、後の負荷を増やす。

## 決定

- `kronello-platform` を native 境界とする。workspace.package を継承し、workspace lints は `unsafe_code=deny` だけ例外として同じ表を置く。Cargo/Rust は inherited `forbid` の局所解除を許さないためである。`scripts/job_evidence.py --check-lints` が他の lint の一致を確認する。unsafe の許可は `cfg(windows)` の private module のみ、各 block に SAFETY コメントを置く。公開 API は安全な path / process 操作で、OS handle を公開しない。jobs / service / model / time は `forbid` を維持する。
- Windows はまず `CreateProcessW` に `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB | CREATE_UNICODE_ENVIRONMENT` を指定する。`ERROR_ACCESS_DENIED` かつ親が Job Object 内の場合だけ、BREAKAWAY_FROM_JOB を外して1回再試行する。それ以外の spawn error と再試行の失敗は `WORKER_DETACH_ERROR`。両試行とも `bInheritHandles=FALSE`、security attributes は null、console / transport handle を継承しない。worker は起動後、固定 job directory の `worker.log` を append で開き、stdout/stderr に設定、stdin は NUL とする。console がないことを確認し、実際の Job Object 所属を `detach_mode: "breakaway"` / `"in_parent_job"` として親・worker両方のlogとevidenceへ記録する。外側Job Objectの制限は下記の修正記録による。
- Unix は既存の `setsid` を維持し、stdin は null、stdout/stderr は worker.log。どの OS も spawn と child PID 登録を同じ DB writer transaction 内で行う。親の reaper thread は worker 終了を待つが submit を待たせない。親終了後は OS が回収し、Windows の process/thread handle はそれぞれ閉じる。
- Windows の生存確認は `OpenProcess(SYNCHRONIZE)` とゼロ待機の `WaitForSingleObject`。ACCESS_DENIED は Unix の EPERM 同様、生存を否定できないものとして扱う。heartbeat 期限と死亡確認を併用する。DB の PID 再利用・hang の識別は未解決で、進捗の証明には使わない。
- Windows publication は `MoveFileExW(..., 0)`。`MOVEFILE_REPLACE_EXISTING` と `MOVEFILE_COPY_ALLOWED` を指定しない。同一 volume 上の file / directory を公開し、既存 file / 空 directory は `OUTPUT_EXISTS`。別 volume の `ERROR_NOT_SAME_DEVICE` は `OUTPUT_CROSS_VOLUME`。macOS/Linux の NOREPLACE rename の EXDEV も同じ型付きエラーにする。copy/delete fallback を行わない。既存の `JobStore::publish` transaction 内で running / worker PID / cancel を確認してから rename する。rename と DB commit の間の電源断の窓は ADR-0050 のまま。
- status polling は WAL の SELECT で読む。期限切れかつ死亡した active record がある場合のみ writer transaction を取り、最新 record を再確認して recover する。読み取り snapshot が古くても terminal record を復活させない。
- slot claim と frame checkpoint も heartbeat と同じ `min(100 ms, heartbeat_interval, heartbeat_timeout/4)` の busy 待機を使う。`SQLITE_BUSY` / `SQLITE_LOCKED` / process gate の `JOB_PROCESS_BUSY` のみ待機・再試行、cancel / ownership loss / その他の storage failure は返す。slot contention / acquisition（wait_ms）、checkpoint contention、既存 heartbeat failure/recovery を worker.log に記録する。lock を永続保持した場合の完了時間は保証しない。
- テストは submitter の起動前に `WorkerCleanup` を作り、state root 削除前に Drop する。応答検証前と Drop 時に DB の登録 PID を取得し、panic / timeout でも kill + wait する。Windows は捕捉時に process handle を保持し PID 再利用を避ける。Linux harness は test-only subreaper で孫 process を adopt し waitpid する。macOS は OS 回収を待ち、消失しなければテストを失敗させる。MCP test client 自体も Drop で kill/wait する。
- evidence runner は各 worker の registry と一時 state DB を保持し、command failure/timeout 後も最後に残存 worker を照会して回収する。Linux の外側 runner も subreaper となり、死亡した harness の子を adopt / waitpid する。残存を見つけた run は回収に成功しても失敗とし、結果を artifact に残す。libtest process 自体の強制 kill では RAII が動かないため、この外側の回収を併用する。

## 検証範囲と Windows の制約

### CI 後の Windows 起動方針の修正（2026-10-05）

初回採用時は breakaway 拒否を型付きエラーとした。
[CI run 37306209358](https://github.com/soramikan/kronello/actions/runs/37306209358)
（revision `4f4da0c3383f670cdec6e819a33ae034bc325330`、Windows Server 2025 / 10.0.26100）で
6件の親終了・publication等の実プロセス試験が access denied により開始できなかった。
supervisor は親 process の終了からの独立を維持し、環境の外側 Job Object には従う方針を指定した。
この追記と上の Windows 決定は、初回の「breakaway拒否では再試行しない」部分を修正する。

親が Job Object 内にあり最初の spawn が ERROR_ACCESS_DENIED の場合だけ、同じ executable / argv / env / cwd / stdio方針で
breakaway flag を外す。成功すれば flag の差による拒否だったと確認できる。別 error・Job Object外のaccess denied・
2回目の失敗は返し、3回目を試さない。CreateProcessW が書き換える argv と出力構造体は各試行で作り直す。
親側は child process handle、worker側は自分のprocessの `IsProcessInJob` で実所属を確認する。
nested jobs で一部しかbreakawayできない場合も、残る所属を `in_parent_job` と記録する。

`in_parent_job` は親CLI/MCP processが終了しても続行できるが、外側Job Objectの終了・強制終了では停止する。
CI step / service manager等が外側Job Objectを終了させる寿命を超えて継続する保証はない。
`breakaway` はこの所属制限を持たない。どちらも console からは detached、stdin / stdout / stderr の transport継承はない。
制限付き親を終了させてから固定入力workerを成功させる Windows-only test と、portableな6分岐policy testで確認する。
Windowsの修正版実プロセスはCI再実行待ち。

`JobStore` の private `JobConnection` は process-local `CONNECTION_LIFETIME` gate を取得してから
SQLite に入り、Connection を破棄し終えてから gate を解放する。open / close だけでなく SQL 操作も
この寿命内に収め、同じ process の二つの job connection が native mutex に同時に入らない。
gate の取得も bounded で、worker の `JOB_PROCESS_BUSY` は再試行する。別 process の SQLite file lock、
FIFO / cancel / publication transaction と schema は維持する。短い state 操作を直列化するコストを受け入れる。

`WorkerHeartbeat` は jobs crate で共有し、CLI/MCP と deterministic test worker が同じものを使う。
heartbeat の最後の成功から heartbeat_timeout が過ぎると、DB を触らない別 watchdog thread が
deadline と job ID を log に記録して process を終了する。次の reader が死亡と stale heartbeat から
interrupted にする。heartbeat thread の join 中も watchdog を止めない。全 process の SIGSTOP や
OS scheduler 自体の停止中は watchdog も実行できず、PID 生存の既存制限は残る。

[SQLite 3.51.2 release notes](https://www.sqlite.org/releaselog/3_51_2.html) は
broken-posix-lock detection の deadlock 修正を記録している。現在の libsqlite3-sys 0.36.0 は3.51.1を同梱する。
この変更では workspace 全体の rusqlite / SQLite 更新を行わず、job connection の順序を本 crate で制御する。
将来の依存更新でも gate / watchdog と回帰試験を維持する。

`kronello-media` の C loader は dlfcn.h / Unix loading と pkg-config FFmpeg headers に依存し、Windows の full CLI/MCP はまだ build できない。JOB-002 ではこの移植を行わない。
Windows CI は opt-in `test-worker` の別 executable を使う。別親 process が固定入力を submit / spawn して終了し、worker は本番の detach / claim / heartbeat / checkpoint / publish / cancel / recovery / prune を呼ぶ。render payload のみ既知の bytes を出力する。この試験を Windows CLI/MCP render の保証へ昇格させない。

Linux/macOS はこの試験に加え実 CLI / MCP worker を明示 CPU reference で実行する。CI は OS/version、Rust version、revision、command、exit、log、worker registry、detach_mode、回収結果を artifact に保存する。初回Linux JOB-002 evidenceは成功（詳細はtesting文書）、修正版各OSはpush後のCI再実行待ち。second-volume test は別 volume を指定した場合だけ明示実行する。

一次仕様: [Windows process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)、[MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)。条件別の試験と未検証範囲は [JOB-002](../testing/job-002.md)。
