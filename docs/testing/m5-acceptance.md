# M5 の受け入れ進捗

確認日: 2026-10-07。`codex/m5-completion` の作業ツリーで、12件のタスク固有条件と統合checkpointを確認した。NAME-001 は所有者の確保判断が保留のため `in_progress` のままであり、M5の全13件をdoneにはしない。mainへのマージ・最終CIの保証と区別する。

## 受け入れたタスク

| タスク | 条件を裏付ける証拠 |
|---|---|
| VEC-002 | [記録](vec-002.md)。morph対応点/segment不一致、SVG非対応/外部参照の型付き拒否。実CLI/MCPとMetalの画素比較、rootのGUI数値編集・Undo・時刻変更 |
| TEXT-002 | [記録](text-002.md)。書記素/clusterを壊さないselector、親文字/ルビ結合とreflow。rootの修正ビルド表示・同時移動・折り返し確認 |
| AUDIO-001 | [記録](audio-001.md)。入力hash・解析版・窓/hop・有理数時間写像固定、不変特徴量の参照。共有保存/再送/未知版拒否、描画時に再解析しない経路 |
| MATTE-001 | [記録](matte-001.md)。保存する関係とtransient入力、循環/欠落拒否、共有編集/Undo。rootのGUI・外部変更確認、CLI/MCPと同梱FFIの固定snapshot画素一致 |
| FRAMEBRIDGE-001 | [記録](framebridge-001.md)。generic拒否と具体8経路を区別。M4実機のpaths 5件/native probe 3件成功。汎用encode/decode対応を追加しない |
| INSPECT-002 | [記録](inspect-002.md)。単一graph・status-only preview・CPU/unknown/temporal/tile見積もり。読み取り専用性とM4/Metal actual countersの3件比較 |
| REPEAT-001 | [記録](repeat-001.md)。共有Source、固定ID/seed、個別制御と明示expand。nested templateとNoiseの保持、CLI/MCP/Metal全画素比較 |
| EXPR-003 | [記録](expr-003.md)。固定DataAsset、動的過去sample、連続noise。静的循環拒否・共有予算・旧版互換とCLI/MCP再保存・全画素一致 |
| GUI-007 | [記録](gui-007.md)。共有APIの追加編集、色・文字書式・ガイド、clip操作と合成。111 CLI/MCP checks、実workerのUndo・競合・IME回帰とrootの直接GUI確認。Option+dragは同一snap:false経路の試験で補完 |
| SIM-001 | [記録](sim-001.md)。固定刻み・checkpoint・粒子の共有API統合。service 12試験、transport+GPU 27 checks、native GUIのseek・外部編集検出・再描画 |
| EXPR-002 | [記録](expr-002.md)。ADR-0105の構文parser/formatterと`property_expression_text_set`/`expression.format`。eval 6・service 5・api 12試験、Swift 12試験、実GUIの診断・Undo・detach・IME marked-text非commit |
| AUDIO-005 | [記録](audio-005.md)。ADR-0106のAAC-LC/Opus採用、4 profileとlibopus配布構成。audio5 5試験を含む5コマンドのevidence、厳密長・bounded error・拒否の型付け |

## 最初の6件の統合checkpoint（796件）

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0。
- `cargo test --workspace --locked`: exit 0。集計796 passed / 0 failed / 41 ignored、119 test-result suites。ignoredの実機検証は上表の個別記録で補い、通常テスト成功だけを実機確認とは呼ばない。
- 公開APIの44操作を含む12試験とNLE schema試験1件が成功。公開schemaの再生成と `scripts/generate_swift_api.py --check` が成功。

ローカルログは `target/m5-acceptance/` の `workspace-build-isolation.log`、`clippy-acceptance.log`、`fmt-check-acceptance.log`、`api-final-freeze.log`、`swift-check-acceptance.log`。これらはGit管理外。再現コマンドと条件は各タスク記録に残す。

先行workspace実行は、並行したGUI用の通常CLIビルドがテスト用の `test-job-control` 付き実行ファイルを置き換え、worker gate等のジョブ試験を壊した。通常CLIのビルドを止めた同じ試験は31 passed / 0 failed / 2 ignored、その後上記の全workspaceが成功した。期待値緩和やproductionの挙動変更では解消していない。テスト用CLIを参照する実プロセスが終了するまで、同じ出力先への別構成ビルドを重ねない。

## REPEAT/EXPR追加後の統合checkpoint（805件）

2026-10-06、公開契約とRust実装のfreeze後にschema/Swiftを再生成した。fmt check、workspace all-targets Clippy `-D warnings`、workspace testはすべてexit 0。121 suites、805 passed / 0 failed / 41 ignored。API 12試験（44操作）とNLE schema 1試験も成功。rootが固有条件とこの結果を確認し、REPEAT-001・EXPR-003をdoneにした。

ログは `target/m5-acceptance/` の `workspace-repeat-expr.log`、`clippy-repeat-expr.log`、`schema-repeat-expr.log`、`swift-check-repeat-expr.log`、`fmt-check-repeat-expr.log`。上の796件は過去checkpointとして保持し、805件と加算しない。

## 残る範囲

M5は12 done / 1 in_progress（NAME-001）。NAME-001 は 2026-10-07 に所有者へ確保アクションを照会し「判断保留」の回答を得たため、採否・取得の確定まで `in_progress` のまま OQ-02 と [調査記録](name-001.md) で追跡する。検証済みの範囲だけで名称の確保を主張しない。

このcheckpoint以後の変更は必要な試験を追加・再実行し、M5最終変更の各OS CIを別に確認する。12件の受け入れを、mainへのマージや全13件の完了とは扱わない。

## SIM/EXPR-002/AUDIO-005 追加後の最終checkpoint（841件）

2026-10-07、SIM-001の統合補強・EXPR-002・AUDIO-005を含む最終sourceで freeze 検証を実施した。`cargo fmt --all --check`、workspace all-targets Clippy `-D warnings`、`cargo test --workspace --locked` はすべて exit 0 で、128 suites・841 passed / 0 failed / 41 ignored。公開schema再生成とSwift GeneratedAPIの再生成・Swift build/test（ExpressionAuthoring 12試験を含む）も成功した。

並行実行中の `kronello-cli --test jobs` で4件の復旧試験が flake したが、同一filterの単独実行（31件）は全て成功し、最終のworkspace全量でも失敗ゼロだった。負荷下のタイミング依存であり、挙動変更や期待値緩和では解消していない。

native GUI の最終確認は `target/macos/Kronello.app`（同梱FFI・CLI・Swiftを最終sourceから再構築）で実施し、SIM-001の fixture seek・外部編集検出・再描画、EXPR-002の attach/commit/構文診断/Undo/detach/IME marked-text非commit を確認した。この環境には Computer Use の MCP server が無いため、macOS Accessibility（System Events）と `screencapture` による直接確認で代用した。証拠は `target/m5-acceptance/gui/m5-final/`（Git管理外）にある。

## GUI-007最終受け入れ

2026-10-06、rootが直接GUI、worker回帰、111 CLI/MCP checksと805件core checkpointを照合し全3条件を受け入れた。最終Swift build `swift-gui007-final-build.log`、assemble `assemble-gui007-final.log`、runtime `app-gui007-final.log`。PID74238でMotion revision6→Text revision16へFileOpenしseek/editなしで正しいText/ruby/Matteを表示した。`gui/text-first-present-final-no-retry.png` とtraceのunattached defer→force schedule→presentedが証拠。bounded retryは含まない。GUI-007をdoneとしたがM5全体の最終CI、mainへのmergeを意味しない。

## Windows M5 regression gate（実行待ち）

既存Windows LGPL runtime構築後、hash-pinned fixtureを取得し、次の純粋層と共有CPU回帰をCIへ追加した。TEXTのNoto font/Japanese fixtureはmanifestのbytes/SHAで検証し、vector/model/eval/simulationは純粋な固定入力を使う。service対象はCpuReferenceのvec002/matte/blend/repeater/sim001に限定し、外部CLI起動やnative surface/GPUを要求しない。FFmpeg runtime/headersはserviceのリンク依存に必要で、既存Windows stepを先行する。fixture動画生成や共有CLI再buildはこの追加gateに含まない。

```sh
python scripts/fetch_fixtures.py
cargo test -p kronello-time -p kronello-model -p kronello-animation -p kronello-eval -p kronello-vector -p kronello-text -p kronello-simulation --locked
cargo test -p kronello-service --test vec002 --test matte --test blend --test repeater --test sim001 --locked
```

workflowへの追加と静的確認は実Windows成功を意味しない。SIMのcoherent freeze後に実CI結果を確認する。既存macOS/Linux workspace、Windows media/store/process、QA platform goldenのstepは維持した。

Windows gateを追加したworkflowはrootがRuby PsychでYAML parseを確認した（既存4jobsとWindows SIM stepを保持）。静的manifest/test target存在確認も成功した。実CIの実行待ちは変わらない。
