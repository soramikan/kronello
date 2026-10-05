# M3 統合受け入れ（2026-10-06）

統合ブランチは `m3-motion-authoring`。GUI / 音声 / QA / 第2段階の未コミット実装を元の作業ツリーを変更せず
`codex/preserve-m3-*` に保存して統合し、JOB-002 の `6ce31a0` とその親履歴を取り込んだ。
後続の `c860d07` / `9b5f2bc` は FIFO connection gate と OS 別 fixture / CI 対策、高負荷の成功記録を追加した。
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
| 最終 Rust workspace、`8b94950` | 711 passed、0 failed、12 ignored、exit0 | 104 suites。`target/m3-acceptance/final-rust-result.json` / `final-workspace.log`。RENDER・JOB最終fixを含む |
| Rust clippy、JOB 統合後 | 成功 | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Metal GPU golden、`def961f` | 1 test / 40 scenes / 40 frames、全 mismatch 0 | `target/golden/run.m3.compare.Ux5o9b/report.json`。clean HEAD、既存 baseline と CPU oracle の通常比較 |
| 最終 fmt / clippy / schema / backlog / Metal golden | 成功、golden全 mismatch 0 | `8b94950` clean HEAD、`final-clippy.log`、`target/golden/run.m3.final.compare.CKVIrc/report.json` |
| SwiftPM build | 成功 | 統合4ページ / audio harness / design gallery をビルド |
| SwiftPM XCTest | 68 passed、0 failures、0 skips | Design19 / Core4 / AppModel45。AppModel は Edit11 / Editor7 / Integration2 / Motion15 / Playback3 / QA2 / Workflow5 |
| 最終 SwiftPM / 三入口、`8b94950`のcode | 68 passed、0 failures、0 skips、23操作のsnapshot一致 | `target/m3-acceptance/final-qa/report.json` / `gui.json` / `ime.json` / `cli-mcp.json`。最適化FFI、実Integration evidenceを再取得 |
| Motion→Edit transition修正後のSwiftPM | 69 passed、0 failures、0 skips | `target/m3-acceptance/gui-transition-swift.log`。未取得Sequence geometry / 旧playback targetの回帰1件を追加、Rust core変更なし |
| native clip / AX数値入力修正後のSwiftPM | 70 passed、0 failures、0 skips | `target/m3-acceptance/clip-hit-final-swift.log`。Design19 / Core4 / AppModel47、3個のxctest bundleの総数。AppModelのnative mouse境界を1件追加 |
| 三入口等価性 | 23操作すべて同じ canonical snapshot | `target/m3-acceptance/qa/report.json`、`gui_compared:true`、実 FFI / CLI / MCP の独立 project |
| 第2段階 FFI 値表示 | 成功 | SwiftPM IntegrationTests が `target/m3-acceptance/integration-metal/gui-evidence.json` を明示取得して全値・boundsを照合 |
| 第2段階の直接GUI閲覧 | 横型 / 縦型の値・bounds・日本語2行→4行の再layout一致 | revision14、`target/m3-acceptance/integration-{landscape,portrait}-shape.png`。INTEGRATION-002の2条件を確認しdone |
| native IME 境界 / QA-002正式受け入れ | 成功、done | QA の `ime.json`。未確定 / 取消の commit0、確定の単一 Event、UTF-8 の結合濁点差分 |
| macOS JOB 実プロセス | 8 passed、0 failures、1 ignored | `--features test-worker --test processes`。ignored は load50 の明示 stress |
| macOS JOB 高負荷、FIFO修正後 | CLI24/24を2回成功、146 workers / orphan0 | `c860d07` clean HEAD、64 burners、最後40 sampleのload79.06–99.68。[JOB-002](job-002.md)末尾 |
| Linux / macOS JOB最終production CI | 全workspace / clippy / 実process証拠が成功 | run37381037423、各62 workers / orphan0 / errors0。Windowsは次の同一revision runでstateを含め成功 |
| JOB最終同一revisionの3OS CI | Linux / macOS / Windowsすべて成功、JOB-002 done | run37382296012、[JOB-002](job-002.md)末尾。production / focused / full gatesを区別 |
| 最終GUI / 本番配置receiverのSwiftPM | 71 passed、0 failures、0 skips | `target/m3-acceptance/final-gui-swift.log`。Design19 / Core4 / AppModel48。native clipとtransition、receiverを追加 |
| 第2段階 Metal 4K driver | 147 checksすべて成功 | `target/m3-acceptance/integration-metal/report.json`、横型 / 縦型 / 固定 snapshot / typed overflow |
| AUDIO-002 実engine / Metal | 3fps、underrun0、seek / stop-resume一致 | `target/m3-acceptance/audio-report.json`。負荷並行条件、physical scanoutは未測定 |
| AUDIO-002 最終quiet実測 | 全3率exit0、underrun / missing0、seek2 / resume一致 | `target/m3-acceptance/audio-quiet/report.json`。38.539–38.603秒、929 / 929 / 1160提示、最大frame格子差0 / 0 / 1frames |
| GUI-003正式受け入れ | 成功、done。直接両テーマのmove / 両端trim / blade / Undo / Motion、配置は本番receiver test | `target/m3-acceptance/edit-*`、[GUI-003](gui-003.md)末尾 |
| GUI-004 正式受け入れ | 成功、done。両テーマ / AX直接数値入力 / 型付き短尺拒否 / 明示保存Undoも確認 | `target/m3-acceptance/template-*`、[GUI-004](gui-004.md)末尾 |
| GUI-004 Export実操作（Dark / Light） | preflight disabled→ready、固定独立worker成功、OUTPUT_EXISTS / FONT_MISSING抑止 | `target/m3-acceptance/gui-export-jobs.json` / `gui-export-final.mov` / `export-*png`。Metal3frames、PCM24 6000samples |
| 最終canonical開発app | `--release` build / deep strict codesign / asset UTI宣言が成功 | `target/macos/Kronello.app`、`target/m3-acceptance/final-app-build.log`。最適化Rust coreと開発Swift UI、runtime FFmpegは別ディレクトリ |
| 元 GUI 作業ツリーの保全 | 成功 | `.worktrees/m3-{gui3,gui4,integ2,qa2}` の tracked patch / 未追跡 source の bytes 一致 |

SwiftPM 初回の debug FFI run は中断時点で未完了だった。sample 採取では音声やUIの停止ではなく、
space-path全時刻のscene評価が大きい日本語フォントの SHA を繰り返す経路に時間を費やしていた。
最適化 FFI の再実行で Motion15件は3.17秒、AppModel45件は25.34秒で成功した。
初回の未完了 run を成功件数に含めない。

先行macOS CIのSwift buildではduration labelの複数文字列 `+` 連結が型検査の時間制限に達した。
同じ表示のstring interpolationへ変更し、実Xcode SwiftPM buildと後続hosted CIが成功した。
run37382296012のhosted Swiftは69 tests / skipped1 / failures0。Integration evidence未指定のskipを、
ローカル最終71 tests / skipped0の明示evidence付きrunと区別する。

再現コマンド:

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  KRONELLO_FFMPEG_LIB_DIR="$PWD/target/native/ffmpeg-lgpl/lib" \
  KRONELLO_INTEGRATION_EVIDENCE="$PWD/target/m3-acceptance/integration-metal/gui-evidence.json" \
  python3 scripts/check_qa_002.py --swiftpm --release --output target/m3-acceptance/qa
KRONELLO_STATE_ROOT="$PWD/target/m3-acceptance/job-state" \
  cargo test -p kronello-jobs --features test-worker --test processes --locked -- --nocapture
```

## 最終判定と保証範囲

- M3の25タスクは正式受け入れを確認しすべて `done`。GUI-003 / GUI-004は直接Dark / Light、clip操作、
  Template / Export / AX尺入力と本番receiverの検査で確認した。QA-002の正式2条件は本番native input境界と三入口比較で成功した。
  OSの実候補ウインドウ / Kotoeri / VoiceOverは未検証として保証範囲から分ける。
- AUDIO-002 は負荷並行 / 最終quietの実デバイス / Metal測定とnative callback分離のreviewが成功し `done`。
  物理scanout / loopback / 主観的listeningの保証は含めない。
- RENDER-003 の長尺 / 4K / memory / 障害の実測と最終統合の必須 Rust gate / GPU goldenは成功。
  [RENDER-003](render-003.md)に元実測と統合結果を分けて記録する。
- JOB-002 は最終3OS実プロセスCIとmacOSの修正後load50超stress / worker回収で成功し `done`。

OS pointerによる素材dropはproviderからdestinationへ未到達で、原因は未確定。
成功やCUA固有の制約とは断定せず、正式条件の本番receiver / shared API検査と区別する。
開発用の最終bundleは `target/macos/Kronello.app`。各fix番号付きbundleは比較実験の記録である。
