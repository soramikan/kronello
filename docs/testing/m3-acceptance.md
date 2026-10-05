# M3 統合受け入れ（2026-10-06）

統合ブランチは `m3-motion-authoring`。GUI / 音声 / QA / 第2段階の未コミット実装を元の作業ツリーを変更せず
`codex/preserve-m3-*` に保存して統合し、JOB-002 の `6ce31a0` とその親履歴を取り込んだ。
元4作業ツリーの tracked diff と未追跡ソースが保存した内容と一致することも再確認した。
実装統合を受け入れ完了とは扱わない。タスク状態の正本は `docs/backlog/backlog.json`。

## 統合修正

- Sequence の `activatePlayback(for:)` を音声時計の `configurePlayback` に接続し、Motion に戻ると
  Composition の対象・rateへ戻す。既存の Sequence duration / extent / NTSC 契約も保持した。
- Metal の最新要求集約、target / revision / 時刻に結ぶ失敗、明示 CPU 参照と presentation host timestamp を共存させた。
- GUI-004 の preview callback を共有 `MetalView.changed(Bool)` に合わせた。
- 全 GUI / Motion / Playback / Edit / Workflow / Integration / QA checks を共通 runner に残した。
- `check_qa_002.py --swiftpm --release` は実 SwiftPM runner と最適化 FFI を使い、独立 CLI / MCP の比較まで検査する。

## 実行済みの証拠

基準コード: `15841a7` の JOB 統合に、`2a42050` の QA driver / ADR index を追加した状態。
Apple Silicon / macOS 27.0.1、Rust 1.95.0、Xcode の Apple Swift 6.4、LGPL FFmpeg 9.0.2 を使用。
CommandLineTools の SwiftPM は `SwiftUIMacros` が欠落し、初回 build は失敗した。
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` の実 Xcode toolchain を使って解消した。

| 検証 | 結果 | 証拠 |
|---|---|---|
| Rust fmt / 公開 Swift schema / backlog | 成功 | `cargo fmt --all --check`、`generate_swift_api.py --check`、`backlog.py check`（77 tasks） |
| Rust workspace、JOB 統合後 | 704 passed、0 failed、8 ignored、exit0 | GPU / FrameBridge を含む103 suites。RENDER-003統合前のbaseline |
| Rust clippy、JOB 統合後 | 成功 | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| SwiftPM build | 成功 | 統合4ページ / audio harness / design gallery をビルド |
| SwiftPM XCTest | 68 passed、0 failures、0 skips | Design19 / Core4 / AppModel45。AppModel は Edit11 / Editor7 / Integration2 / Motion15 / Playback3 / QA2 / Workflow5 |
| 三入口等価性 | 23操作すべて同じ canonical snapshot | `target/m3-acceptance/qa/report.json`、`gui_compared:true`、実 FFI / CLI / MCP の独立 project |
| 第2段階 FFI 値表示 | 成功 | SwiftPM IntegrationTests が `target/m3-acceptance/integration-metal/gui-evidence.json` を明示取得して全値・boundsを照合 |
| native IME 境界 | 成功 | QA の `ime.json`。未確定 / 取消の commit0、確定の単一 Event、UTF-8 の結合濁点差分 |
| macOS JOB 実プロセス | 8 passed、0 failures、1 ignored | `--features test-worker --test processes`。ignored は load50 の明示 stress |
| 第2段階 Metal 4K driver | 147 checksすべて成功 | `target/m3-acceptance/integration-metal/report.json`、横型 / 縦型 / 固定 snapshot / typed overflow |
| AUDIO-002 実engine / Metal | 3fps、underrun0、seek / stop-resume一致 | `target/m3-acceptance/audio-report.json`。負荷並行条件、physical scanoutは未測定 |
| GUI-003 実操作（Dark） | trim / Undo / blade各単一Event、Motion遷移でrevision不変 | `target/m3-acceptance/edit-*`、[GUI-003](gui-003.md)末尾 |
| 元 GUI 作業ツリーの保全 | 成功 | `.worktrees/m3-{gui3,gui4,integ2,qa2}` の tracked patch / 未追跡 source の bytes 一致 |

SwiftPM 初回の debug FFI run は中断時点で未完了だった。sample 採取では音声やUIの停止ではなく、
space-path全時刻のscene評価が大きい日本語フォントの SHA を繰り返す経路に時間を費やしていた。
最適化 FFI の再実行で Motion15件は3.17秒、AppModel45件は25.34秒で成功した。
初回の未完了 run を成功件数に含めない。

再現コマンド:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib" \
  KRONELLO_INTEGRATION_EVIDENCE="$PWD/target/m3-acceptance/integration-metal/gui-evidence.json" \
  python3 scripts/check_qa_002.py --swiftpm --release --output target/m3-acceptance/qa
KRONELLO_STATE_ROOT="$PWD/target/m3-acceptance/job-state" \
  cargo test -p kronello-jobs --features test-worker --test processes --locked -- --nocapture
```

## 受け入れ確認を続ける範囲

- 主エージェントが実アプリを直接操作して Dark / Light、クリップ操作、テンプレート・書き出し、
  macOS 日本語入力ソースの候補ウインドウ / 確定 / 取消を確認する。
- AUDIO-002 の3種類のfpsで実デバイス / Metal / seek / 停止再開を測定する。SwiftPMのhost fallbackだけで完了と扱わない。
- RENDER-003 の統合と長尺 / 4K / memory / 障害の実測後、最終コードの workspace tests と GPU golden を確認する。
- JOB-002 の Windows / Linux の実プロセス CI と load50 stress の受け入れを確認する。

その後の結果は本書と各タスクの検証文書へ追記する。
