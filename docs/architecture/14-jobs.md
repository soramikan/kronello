# 14 ジョブ

長時間の処理（最終レンダー、書き出し）を、投入したプロセスや接続の寿命から独立して実行する仕組み。[ADR-0025](../adr/0025-detached-render-workers.md) による。JOB-001（M2）で macOS の CLI / MCP と明示 CPU reference の実プロセスを検証した。実装判断は [ADR-0050](../adr/0050-fixed-job-execution-and-publication.md) と [ADR-0074](../adr/0074-windows-job-workers-and-process-evidence.md)、条件ごとの結果は [JOB-001](../testing/job-001.md) / [JOB-002](../testing/job-002.md)。初回Linux JOB-002 CI evidenceは成功、Windows jobs/platform の修正版はCI再実行待ち。Windows full CLI/MCP、再開・GPU 実機経路は未検証。

## 構成

```text
GUI / CLI / MCP
   |  render.submit
   v
1. 固定スナップショットを直列化してジョブディレクトリへ保存
2. 状態 DB にジョブを記録 (queued)
3. worker プロセスを切り離して起動  --->  kronello worker --job <id>
4. job ID を返す                              |
                                              |  実行スロットを取得 (running)
   job.get / job.list / job.cancel            |  heartbeat と進捗を状態 DB へ記録
   <------------- 状態 DB ------------------->|  一時ファイルへ出力 -> 検証 -> 確定名へ切り替え
                                              v
                                     succeeded / failed / canceled
```

- 常駐するプロセスはない。worker はジョブ 1 件のために起動し、終わると終了する。
- GUI から投入したジョブも同じ仕組みで動く。GUI を閉じても書き出しは続く。
- worker は投入したプロセスと同じ版の実行ファイルから起動する。ジョブにはエンジンの版を記録する。
- CLI は `kronello worker --job <id>`、MCP は同じ実装の `kronello-mcp worker --job <id>` を起動する。Unix worker 開始時に `setsid`、stdin は null、stdout/stderr は worker.log とし、MCP pipe を継承しない。埋込み service は worker executable を注入できる。
- Windows は `kronello-platform` の `CreateProcessW` に NEW_PROCESS_GROUP / DETACHED_PROCESS / BREAKAWAY_FROM_JOB、handle inheritance FALSE を指定する。ERROR_ACCESS_DENIEDかつ親がJob Object内の場合だけbreakaway flagを外して1回再試行する。他のerror・2回目の失敗は `WORKER_DETACH_ERROR`。worker 自身が NUL / worker.log を開き、consoleがないことを確認する。実所属を `detach_mode: "breakaway"` / `"in_parent_job"` としてlog / evidenceへ記録する。後者も親CLI/MCP process終了後は続行するが、外側Job Objectの終了は越えられない。親生存中は reaper thread、親終了後は OS 回収。Windows の full CLI/MCP build は media loader 移植待ちで、CI は同じ jobs/platform API を使う test-only worker で検証する。

## 置き場所

| 対象 | 場所 |
|---|---|
| 状態 DB | ユーザーごとの状態領域の `jobs.sqlite3`（macOS root は `~/Library/Application Support/Kronello/`） |
| ジョブディレクトリ（固定スナップショット、ログ） | `jobs/<UUID>/input.json` / `worker.log` |
| 一時出力 | 出力先と同じボリューム上の一時ファイル |
| 成果物 | 利用者が指定した出力先 |

ジョブは「このマシンでの実行」であり、作品の内容ではない。ジョブの進行で `.kronello` へ書き込まない。

`KRONELLO_STATE_ROOT` / `Service::with_job_config` で state root を注入する。全テストは一時 directory を使い、実ユーザー領域を開かない。Linux の既定 root はユーザーの data-local 領域の Kronello（実プロセスは未検証）。

## ジョブの記録

- job ID、投入時刻、エンジンの版
- プロジェクトの ID と、固定スナップショットの hash・revision
- 出力プロファイル、出力先
- 状態、進捗、heartbeat の時刻、worker のプロセス情報
- 結果（成果物の場所と検証結果、または型付きエラー）
- 入力 envelope の byte SHA-256、完了 frame 数 / 全 frame 数、cancel_requested、finished_at_ms、directory_pruned。時刻は Unix epoch の millisecond、revision は10進文字列。job.list は投入順で全記録を返す。

## 状態

| 状態 | 意味 |
|---|---|
| `queued` | 記録済み。実行スロットを待っている |
| `running` | worker がスロットを取得して実行中 |
| `succeeded` | 成果物を検証し、確定名へ切り替えた |
| `failed` | 型付きエラーで終了した |
| `canceled` | 取り消し要求を受けて停止した |
| `interrupted` | heartbeat が期限を超え、所有 worker の生存も確認できない（異常終了、マシンの停止など） |

## 入力の固定

投入時に、その時点の revision の RenderSnapshot を直列化してジョブディレクトリへ保存する。投入後にプロジェクトが編集されても、ジョブは投入時点の内容で完了する。

投入側は `ProjectStore::read_snapshot` の READ_ONLY / query_only を使い、journal 変更・migration・checkpoint をしない。envelope schema 1 は RenderSnapshot、`SequenceRenderRequest`、出力 profile、明示音声配置、required_features、backend を固定する。同期 render と同じ `freeze_render_input` と `RenderTarget`（Composition / Sequence）を使い、job 専用 target を作らない。Sequence、配置、資産、意味版、revision も固定入力に含める。AUDIO-003 の MOV は出力 profile に音声 mode を固定する。省略は既存の version 1 / explicit clips（空なら silence）。version 2 の document は同じ RenderSnapshot の Sequence / Composition 音声、silence は意図的な無音。document / silence と非空 clips の併用は拒否する。

素材は外部参照のままであり、worker が content hash を照合する。一致しなければジョブは失敗する。フォントやデータの lock も同様に照合する。

固定 snapshot には公開 `schema_version` と、文書の `semantic_version` を含む `semantic_versions` を記録する。worker は開始・再開時に構造・意味の版、必要機能、lock を検証する。同じエンジン版から起動したことだけで省略せず、未知の必要機能は `UNSUPPORTED_FEATURE` とする。migration や再開時に固定入力を最新の Project / 意味へ上書きしない（[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)）。

## 同時実行

- 既定の同時実行は 1 ジョブ。4K / 8K では GPU メモリとエンコーダーが競合するためである。
- worker は状態 DB のトランザクションで実行スロットを取得する。取得できない間は `queued` のまま待ち、投入順に実行する。
- スロット数は設定で変更できる。
- `KRONELLO_JOB_SLOTS=1` が既定。heartbeat は queued / running を別 thread で維持し、間隔1000 ms・期限30000 ms が既定（`KRONELLO_JOB_HEARTBEAT_MS` / `KRONELLO_JOB_TIMEOUT_MS`）。全入口は同じ設定を使う。slot 取得は `BEGIN IMMEDIATE` で DB の投入順 seq と running 数を照合する。
- heartbeat 専用 connection の SQLite busy 待機は `min(100 ms, heartbeat_interval, heartbeat_timeout / 4)` とする。通常の状態操作の5秒待機を使わない。`SQLITE_BUSY` / `SQLITE_LOCKED` は worker.log に記録し、次の間隔で再試行する。所有喪失など他のエラーは記録して heartbeat thread を終了する。worker 起動・heartbeat 開始・書込み失敗・復帰・thread panic も stderr（切り離した worker では worker.log）へ記録する。
- プレビュー（`preview.render`）はジョブではなく、呼び出したプロセス内で即時に実行する。スロットを消費しない。
- slot claim / frame checkpoint も heartbeat と同じ短い busy 待機と競合のみの再試行を使う。slot 待機時間と contention、checkpoint contention を記録する。get/list は通常 WAL read、期限切れかつ死亡した active record のある場合のみ writer を取得して最新状態を再確認する。lock が解放されなければ完了時刻は保証しない。
- slot claim は初回の owner 登録と Running への遷移時に record を保存する。同じ owner の queued poll は冗長な FULL 同期更新を避け、queued heartbeat は専用 pulse thread が維持する。

## 中断と取り消し

JOB-002 は SQLite 3.51.1 の Unix VFS の concurrent WAL close / open の mutex deadlock を防ぐため、
job DB の connection 寿命（open / SQL / close）を process-local gate で直列化する。
heartbeat 用 gate の取得待機も bounded、`JOB_PROCESS_BUSY` は競合として再試行する。
gate は FIFO 待機列を持ち、終了した thread の即時再取得で既存 waiter を追い越さない。
期限切れ waiter は列から取り除き、後続を妨げない。Connection の close 完了まで ownership を保持する。
`WorkerHeartbeat` の独立 watchdog は最後の heartbeat 成功から heartbeat_timeout で log を記録し process を終了する。
native mutex が待ち続けても DB へ終了記録を書こうとせず、次の reader の stale / dead 判定で interrupted にする。
heartbeat thread の join 中も watchdog は有効。全 process の停止中は watchdog も動かない。

- `job.cancel` は状態 DB に取り消し要求を記録する。worker は処理の区切りで要求を確認し、一時出力を片付けて `canceled` にする。
- heartbeat が一定時間途絶え、所有 worker の生存も確認できないジョブは、次に状態を読んだプロセスが `interrupted` と判定し、スロットを解放する。
- `interrupted` のジョブは自動では再開しない。`job.resume` は RECOVERY-001 の提案で、JOB-001 の registry / wire にはなく `INVALID_REQUEST`。出力ファイルへの無条件 append を再開方法に使わない。
- 失敗や中断で、プロジェクトや確定済みの成果物を壊さない。

中断判定は [ADR-0050](../adr/0050-fixed-job-execution-and-publication.md) のとおり、heartbeat の期限に Unix の生存確認を組み合わせる。起動側は child PID を DB に登録してから submit を返すため、worker が heartbeat thread を開始する前も確認できる。期限を超えた queued / running record でも `kill(pid, 0)` が成功、または `EPERM`（存在するが権限なし）なら中断させない。PID が未登録・不正、または生存を確認できない場合は従来どおり期限で中断する。get / list、slot 取得、prune は同じ判定を使う。terminal record の復活、自動再開、成果物の自動成功補正は行わず、publish の DB fence は維持する。

この生存確認はプロセスの進捗や起動 identity を証明しない。停止・hang・未回収 zombie、PID 再利用では slot 解放が遅れる場合がある。生存中のプロセスを期限だけで中断する方法へ戻さず、進捗監視・起動 identity の強化は後続で設計する。Windows は `OpenProcess(SYNCHRONIZE)` / `WaitForSingleObject(..., 0)` で生存を確認し、ACCESS_DENIED は生存を否定できないものとして扱う。死亡した worker の期限切れ record は interrupted にする。

Windows publication は同じ DB fence の中で `MoveFileExW(..., 0)` を使い、既存 file / 空 directory を `OUTPUT_EXISTS` として拒否する。copy/delete は許可せず、別 volume は `OUTPUT_CROSS_VOLUME`（Unix の EXDEV も同じ code）。同一 volume の rename と DB commit の間の電源断の窓は維持する。

画像連番は destination volume の temporary directory に全 artifact を出力し、manifest・metadata・byte 長・hash・snapshot identity を再読検証する。既存 pro_res_mov は ProRes + stereo 48 kHz PCM24、明示 background と選択 mode の音声を同じ AvExportSnapshot から出力し、stream / PTS / duration / snapshot metadata を probe する。frame 境界と確定前に cancel を確認する。DB transaction 内で lease / cancel を再確認し、atomic NOREPLACE rename で全 directory または movie file を一度に確定する。既存成果物は空 directory も上書きしない。

movie の現行上限・SDR 契約は AUDIO-000 のまま。MEDIA-002 の追加 profile は下記。SIGKILL は destination の temporary directory を残す場合がある。また rename と DB commit の間の電源断では検証済み成果物と interrupted 記録が共存しうる。temporary output 回収・成果物照合・再開は RECOVERY-001 で設計し、自動で成功扱いにしない。

## 保持と掃除

[ADR-0034](../adr/0034-job-retention.md) による。

- 状態 DB のジョブ記録（いつ、どのスナップショットを、どこへ書き出したか）は残す。
- ジョブディレクトリ（固定スナップショット、ログ）は、終了から 30 日を過ぎた成功・失敗・キャンセルを削除する。`KRONELLO_JOB_RETENTION_SECONDS`（既定2592000）で変更できる。
- 掃除は、次にジョブを投入したプロセスが行う。
- `interrupted` のジョブは再開できるよう掃除の対象外とする。
- `job.prune` で手動でも掃除できる。

GUI の UI 状態も同じ状態領域に保存する（[10 デスクトップ GUI](10-desktop-gui.md)）。

## 未決事項

- 再開時に完了済みの区間をどこまで再利用できるか（RECOVERY-001 で設計）。

## AUDIO-003 の固定音声入力

[ADR-0063](../adr/0063-document-audio-and-clip-volume.md) の `audio` / `profile_version` と
Clip volume / Curve / Media source / nested maps は input.json に保存する。
worker は最新 Project を読まず、owned RenderSnapshot から文書音声 plan を再コンパイルする。
AvExportSnapshot schema 2 は mode と flatten 済み placement を hash に含め、元の固定文書と照合する。
文書音声の音量編集は RenderSnapshot hash にも反映し、mode は job input_hash / export hash に反映する。
report の audio_source / audio_profile_version と両 snapshot hash は同期 `render.export` と共有する。

動画 / 音声を同じ revision に固定し、作品を編集・削除しても変えない。外部 asset 自体は引き続き
hash lock で検証し、欠落 / hash 不一致 / clipping で失敗した worker は MOV を publish しない。
NTSC sample count / decoded A/V の固定性と failure publication は
[AUDIO-003 の検証](../testing/audio-003.md) を参照。host GPU / hardware 検証は別 gate。

## MEDIA-002: 追加 movie profile

`av1_mp4` / `h264_mov` / `hevc_mov` の version 1 を同期 `render.export` と
`render.submit` に追加する。audio mode と clips / background は同じ要求型。
追加 profile は AvExportSnapshot schema 3 / evaluator 2 に固定し、codec / container と
版付き MovieProfile を export hash / job input hash に含める。元作品を再読しない。
AV1 は .mp4、H.264 / HEVC は .mov、全て48 kHz stereo native ALAC（PCM24量子化後）。
probe は選択 profile の codec / start / duration / metadata を検証し、既存の lease / cancel
fence と no-clobber publication を使う。AAC / 未知 version は UNSUPPORTED_FEATURE。
既存 ProRes profile 1/2/3 と explicit / document / silence の意味は維持する。
[ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md)、[MEDIA-002 の検証](../testing/media-002.md)。

## RENDER-003 の有界 movie worker

固定 movie worker も共有 `export_av_with_checkpoint` の source spool / audio block / tile sink /
1-frame encodeを使う。input schema / snapshot hash / MovieProfileと publication fenceは維持する。
source chunk、audio block、video frameと最終mux前にcancelを確認する。
通常エラーで destination volume の一時 directoryを回収し、既存成果物を上書きしない。
強制終了後の回収とresumeはRECOVERY-001。
`AvExportReport.streaming` の byte counters とホスト検証範囲は
[ADR-0079](../adr/0079-bounded-streaming-movie-export.md)、[RENDER-003](../testing/render-003.md) を参照する。
