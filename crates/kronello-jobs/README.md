# kronello-jobs

ユーザーごとの SQLite 実行記録、FIFO の実行 lease、heartbeat、キャンセル、
保持と掃除、切り離した worker 起動を担当する。作品 DB は開かず、model / render /
eval に依存しない。

固定 render 入力の作成と実行は `kronello-service`、共通 worker 入口は CLI / MCP の
両 binary にある。テストは `JobConfig` / `KRONELLO_STATE_ROOT` を注入し、実ユーザーの
状態領域を開かない。

[ADR-0050](../../docs/adr/0050-fixed-job-execution-and-publication.md) と
[JOB-001 の検証](../../docs/testing/job-001.md) を参照。
