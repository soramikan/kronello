# M4 統合受け入れ

正本は `docs/backlog/backlog.json`。この記録は途中経過であり、M4 全体の完了宣言ではない。

## 基準機と中間コミット

2026-10-06、Apple M4 / Mac mini Mac16,10 / 32 GB、macOS 27.0.1 (26A434)、Rust 1.95.0。
中間コミット `572a4e80ea98a277d28eb86fc13a865cfb8a76cb` を
[draft PR #8](https://github.com/soramikan/kronello/pull/8) に保存した。
このコミットには RENDER-002 / CACHE-002 / COMP-002 / GPU-003 / MCP-003 / FFI-002 の
受け入れと、MEDIA-003 の Windows 実行用実装・CI を含む。

## 中間検証

- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `cargo fmt --all --check`、公開 API schema / Swift 再生成、`python3 scripts/backlog.py check`: pass。
- SwiftPM: 48 tests、0 failures、1 skipped。stage-2 evidence を指定した追加実行で skip 対象の FFI parity も 1 passed。8 つの landscape / portrait と時刻の組で Inspector / Viewer の値が一致した。`swift-checkpoint.log` / `swift-integration-checkpoint.log`。
- `python3 -m unittest scripts.tests.test_media_build -v`: 6 passed。design preview の検証も pass。
- GPU-003: 実 VideoToolbox / Metal の native 2 tests と production 4 tests。CFR / VFR / B-frame、H.264 / HEVC、正確な PTS、所有権、転送量、偽った SDR metadata の拒否を確認。詳細は [GPU-003](gpu-003.md)。
- FFI-002: root agent が CUA で実 GUI を操作し、安全モードの帯、音声再生、close / reopen を確認。CLI / MCP の競合と native process の kill / close / audio / render.submit は [FFI-002](ffi-002.md)。
- MCP-003: 公式 SDK 2.3.0 の実 stdio / HTTP、進捗・取消・再利用と legacy 回帰を確認。[MCP-003](mcp-003.md)。

workspace テストの最初の実行では、新しい仕様で有効になった小数 progress token と Video 内音声を
未対応としていた旧テストが失敗した。無効型の拒否、未対応 3ch 音声の拒否を維持して期待値を修正した。
修正後の後半 10 crates は exit 0（473 passed、11 ignored）。前半の成功分は 246 passed、7 ignored。両ログは重複する suite を含みうるため件数を合算しない。ignored の native GPU-003 は上記の明示実行、golden は QA-004 の明示実行で補う。全変更確定後に workspace 一括 gate を再実行する。CI の結果は確認後に追記する。

生ログは `target/m4-acceptance/`。GPU / FFI / MCP の詳細ログはそれぞれの検証文書に記載する。
Windows の受け入れは [初回 CI](https://github.com/soramikan/kronello/actions/runs/37400231020) の
実結果を確認するまで未完了とする。HDR / 8K、障害復旧、追加 golden、GPU / disk cache、性能測定は継続中。
