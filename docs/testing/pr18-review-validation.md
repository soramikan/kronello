# PR #18 レビュー修正の検証

2026-10-12、FFI の状態変更判定に `audio.plugin_process` を追加した。registry の `read_only` は作品を書き換えない意味であり、この操作は実行状態の job を投入する。成功応答の直後に通知を更新する対象に含め、250 ms の定期 poll を待たない。

本番 worker が使う判定を独立関数へ抽出し、実 `command_registry` の read-only 属性を保ったまま plugin job を更新対象にする回帰テストを追加した。`project.info` / `job.list`・未知操作・不正 JSON は更新対象にしない。

macOS の `cargo test --locked -p kronello-ffi --lib` は 4 passed。通知判定に加え、CPU preview budget と実 Metal adapter の外部出力 blit 比較も通過した。前段 PR のレビュー修正も merge commit で取り込んだ。workspace と各 OS の検証は最新 PR CI の結果を正本とする。
