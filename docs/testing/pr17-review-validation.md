# PR #17 レビュー修正の検証

2026-10-12、PR #17 の先端 `7e98f692` を基に修正した。

- CPU DAG が返す `UNSUPPORTED_FEATURE` の surface budget 診断も preview の縮小再試行対象に戻した。無関係な未対応機能・I/O エラーは再試行しない。
- `KRAnchorTracker` は同じ backing view を再通知せず、SwiftUI 更新ごとの state 書き込みを防ぐ。別 view への置換は通知する。
- Windows CI [run 38131672004](https://github.com/soramikan/kronello/actions/runs/38131672004) の `panic_and_timeout_cleanup_leave_no_workers` は fixture の `JobStore::open` が process-wide connection gate の `JOB_PROCESS_BUSY` を返して失敗した。fixture 初期化のみ、実 worker と同じ retryable contention を最大30秒再試行する。production の gate・期限・cleanup assertion は変更しない。

macOS のローカル検証:

- `cargo fmt --all --check` と FFI / jobs の all-targets clippy（`test-worker` 有効、`-D warnings`）: exit 0。
- `cargo test --locked -p kronello-ffi cpu_dag_budget_errors_fit --lib`: 1 passed。
- `cargo test --locked -p kronello-jobs --features test-worker --test processes`: 8 passed / 1 ignored（別 volume を指定する既存条件付き試験）。
- `swift test --disable-sandbox --package-path apps/macos -c release --filter MenuAnchorTests`: 1 passed。繰り返し update と backing view の置換を確認。

Windows の修正版実プロセス検証と workspace 全体の検証は、この修正を push した最新 PR CI の結果を正本とする。上記のローカル結果で代替しない。
