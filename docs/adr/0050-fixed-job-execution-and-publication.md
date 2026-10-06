# ADR-0050: 固定ジョブ入力・実行 lease・成果物確定を共有 service で扱う

- 状態: 部分置換（[ADR-0074](0074-windows-job-workers-and-process-evidence.md): Windows 起動・生存確認・publication、worker の競合待機、状態照会。[ADR-0079](0079-bounded-streaming-movie-export.md): movie export の AUDIO-000 payload 上限の継承。その他の決定は維持）
- 日付: 2026-10-04
- 対象: JOB-001

## 背景

ADR-0025 / 0034 / 0045 に従い、常駐プロセスを設けず、CLI / MCP の寿命から独立した書き出しを実装する。作品の revision とジョブの進行を分離し、入力の互換性・素材 lock・確定出力を検証する。既存 ADR の決定は変更しない。

## 決定

- `kronello-jobs` は bundled rusqlite による実行状態とプロセス起動を担当する。model / eval に逆依存を追加しない。状態 DB は `<state_root>/jobs.sqlite3`（内部版1、WAL、synchronous=FULL）、入力とログは `jobs/<UUID>/input.json` / `worker.log`。macOS の既定は `~/Library/Application Support/Kronello/`。`KRONELLO_STATE_ROOT` と `Service::with_job_config` で注入できる。テストは必ず一時 directory を指定する。
- `render.submit` は `SequenceRenderRequest` を `render` member にそのまま利用する。独自の target enum を作らない。同期 render と submit は `freeze_render_input` で同じ target compiler を通す。本基点の target は Composition。並行 NLE-001 の Sequence 対応は共有 RenderInput / compiler の拡張として統合する。
- `ProjectStore::read_snapshot` は READ_ONLY / query_only で現在の revision と文書を一つの SELECT から取得する。journal 変更・migration・checkpoint をしない。固定 envelope schema 1 に owned RenderSnapshot、元要求（出力・音声配置・必要機能・絶対 locator）、明示 backend を保存する。入力を fsync してから状態 DB に queued を記録し、worker を起動する。worker は元の `.kronello` を開かない。作品 path は相対素材の基準としてのみ使う。
- snapshot の canonical content hash と、保存した envelope の byte SHA-256（`input_hash`）を分ける。出力設定・音声配置・font locator も後者に含む。構造版、文書を含む意味版、選択依存の実行能力、要求の required_features、全外部 asset hash、必要 font lock を worker が検証する。未知機能は `UNSUPPORTED_FEATURE`、素材不一致は `ASSET_HASH_MISMATCH`。固定入力を最新文書で置換しない。
- CLI は `kronello worker --job <id>`、MCP は `kronello-mcp worker --job <id>` で同じ `worker_entry` を呼ぶ。現在の executable を再起動するため、MCP の隣に別 binary があるという仮定を置かない。埋込み側は `with_worker_executable` で同じ版の worker 対応 executable を指定できる。stdio は null / job log に切り替え、Unix worker の開始時に safe nix の `setsid` で独立 session を作る。親が生存中は reaper thread、親終了後は OS が子を回収する。Windows の detach は明示 `UNSUPPORTED_FEATURE` とし、検証していない実装を提供しない。
- queued worker 自身が待機する。常駐 scheduler は置かない。`BEGIN IMMEDIATE` 内で投入順の DB seq、running 数、queued 内順位を照合して slot を取得する。既定1 slot。queued / running の両方を別 thread の heartbeat で維持する。読み取り・slot 取得・prune 時に、heartbeat が期限切れで、かつ所有 worker の生存を確認できない record を interrupted にし、slot を解放する。起動側は child PID を DB に登録してから submit を返し、Unix では `kill(pid, 0)` の成功または `EPERM` を生存とみなす（PID 未登録・不正・Unix 以外は期限のみで判定）。heartbeat は専用 connection と短い busy 待機（`min(100 ms, 間隔, 期限/4)`）を使い、SQLite の競合で lease を失わない。異常終了した queued worker も検出できる。自動再開はしない。生存確認は進捗や起動 identity を証明しないため、hang・停止・PID 再利用では slot 解放が遅れうる。進捗監視と起動 identity の強化は後続（RECOVERY-001 / JOB-002）で扱う。
- `job.cancel` は要求を記録し、worker は待機時、frame 境界、符号化後・確定前に確認する。通常のキャンセル・失敗では temporary directory を削除する。reader が lease を失効させた worker は確定できない。heartbeat は重い render / encode と独立し、作品に書き込まない。
- 出力は `image_sequence`（RGBA16F + PNG + JSON）または `pro_res_mov`（SDR linear Rec.709、明示背景、ProRes + stereo 48 kHz PCM24）。MOV は AUDIO-000 の AvExportSnapshot / export を使い、映像と明示 clips を同じ入力に固定する。clips が空なら音声 track は silence。clipping は Reject。image_sequence は映像成果物で、音声配置を受け取らない。暗黙の codec / backend fallback は設けない。
- 同梱 LGPL FFmpeg 9.0.2 は software `prores_ks` / native `pcm_s24le`、AV1 `libsvtav1`、VideoToolbox H.264 / HEVC を提供する構成（ADR-0048 / 0049）。JOB-001 で公開する movie profile は ProRes / PCM24 MOV のみ。AV1・H.264・HEVC の job profile、AAC、HDR、streaming 大容量 export は今回提供しない。既存 AUDIO-000 の時間・payload 上限を維持する。
- temporary output を destination の親の一時 directory に作る。画像は manifest・frame metadata・全 artifact の byte 長 / hash / snapshot identity を再読検証する。MOV は既存 export の stream / codec / PTS / duration / snapshot metadata 検証に加え、worker が再 probe する。最後に短い DB transaction で running / cancel を再照合し、rustix の atomic NOREPLACE rename（macOS / Linux）で file または directory を確定する。既存の空 directory も上書きしない。
- file system の rename と DB commit は一つの原子的 transaction にはできない。rename 後・DB commit 前の電源断等では、検証済み成果物と interrupted 記録が共存しうる。これを無検証で succeeded に補正しない。SIGKILL が残す destination 側 temporary output の回収と、この窓の照合は RECOVERY-001 の範囲。半端な final file を作る方法で解決しない。
- 成功・失敗・キャンセルの終了から既定30日を**過ぎた** directory を次の submit と `job.prune` で削除する。DB 記録は残し、directory_pruned を記録する。interrupted は常に除外。`job.resume` は registry / wire に登録せず INVALID_REQUEST とする。

## 設定

| 環境変数 | 既定 | 意味 |
|---|---|---|
| `KRONELLO_STATE_ROOT` | OS のユーザー領域 | DB / job directory の root |
| `KRONELLO_JOB_SLOTS` | 1 | 正の同時実行数 |
| `KRONELLO_JOB_HEARTBEAT_MS` | 1000 | 正の heartbeat 間隔 |
| `KRONELLO_JOB_TIMEOUT_MS` | 30000 | heartbeat 間隔より大きい期限 |
| `KRONELLO_JOB_RETENTION_SECONDS` | 2592000 | 終了 directory の保持期間 |

submit の config を child 環境へ渡す。同じ状態領域を使う入口は同じ slot / timeout 設定を使う。テストの checkpoint gate / artifact corruption は `test-job-control` と debug_assertions の両方を要求し、通常・release build では無効。

## 検証と影響

[JOB-001 の検証](../testing/job-001.md) に7条件と real CLI / MCP / SIGKILL / CPU reference の対応を記録する。Linux / Windows runtime、実 GPU、hardware encoder、長尺の性能、RECOVERY-001 の再開は未検証。GPU golden は変更しない。
