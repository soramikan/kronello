# M4 統合受け入れ

正本は [backlog.json](../backlog/backlog.json)。12 タスクすべての受け入れ条件と最終全 OS CI を確認し、2026-10-06にM4を完了した。
変更は [PR #8](https://github.com/soramikan/kronello/pull/8) に統合する。

## 基準機でのローカル検証

2026-10-06、Apple M4 / Mac mini Mac16,10 / 32 GB、macOS 27.0.1 (26A434)、Rust 1.95.0。
生ログは target/m4-acceptance/ に保存する。途中のテスト件数を足し合わせて最終 gate の件数として扱わない。

| 検証 | 結果・証拠 |
|---|---|
| fmt / workspace all-targets clippy | 成功。final-coverage-fmt.log / final-coverage-clippy.log |
| workspace test | 754 passed / 0 failed / 38 ignored。final-coverage-workspace.log |
| SwiftPM、統合 evidence 指定 | 71 tests（Design 19 / Core 4 / AppModel 48）/ 0 failures / 0 skipped。final-coverage-swift.log。8つの landscape / portrait と時刻の組で Inspector / Viewer の共有 FFI parity も含む |
| Metal golden | 40 scenes 全件成功。target/golden/run.m4-coverage-20261006/report.json。M1 基準と許容誤差は変更していない |
| VideoToolbox / Metal 常駐経路 | 4 tests / 0 failures / 0 skipped。final-coverage-resident.log |
| 8K HDR 明示実行 | 1 passed。final-coverage-hdr.log。33,177,600 pixels、linear 265,420,800 bytes、最大 linear/display tile payload 8,388,608 bytes |
| 公開 API | schema / Swift 再生成・整合検査成功 |
| 公式 MCP SDK 2.3.0 | 実 stdio / HTTP と legacy 回帰が成功。final-mcp-sdk.log |
| Python | native build regression 7件、golden adoption regression 11件成功 |
| release macOS app | build・通常の ad-hoc signing・署名検証成功。final-coverage-macos-build.log |

ignored の実機専用項目は、通常 workspace 件数と明示実行の結果を区別する。
HDR 全体画像と原寸の日本語文字・マスク境界は root agent が目視確認した。
Rust 1.95 の strip に起因する LINKEDIT alignment 問題には、macOS release FFI の strip だけを無効にする限定的対処を行った。[原因・再現・検証](macos-linkedit.md)を参照。

## root agent による GUI 確認

CUA で release app を直接操作し、M3 実機作品の独立した SQLite backup を開いた。元の作品は変更していない。
Motion の日本語字幕・背景帯と Inspector の bounds、Edit の 7:23→8:00 の字幕切替と逆方向の復帰、Full / Quarter の解像度切替、Template の横/縦 variant、Export の入力検証を確認した。
出力先入力後の preflight は成功した。GUI から実書き出しは開始していない。

synthetic output root 省略後、および coverage bounds 最適化後も実画像と Inspector・クリップ境界を再確認し、Cmd-W で Welcome、Cmd-Q で process 終了を確認した。
画像は gui-final/motion-coverage.png、edit-coverage-cut.png、edit-full-reverse.png、export-preflight.png。
再生時の host clock / underrun 0 は、音声の可聴品質や物理 display FPS の測定を意味しない。
安全モード・共有 project session・音声と close の先行検証は [FFI-002](ffi-002.md)を参照。

## 機能別の証拠と制限

- [GPU-003](gpu-003.md): 実 VideoToolbox / Metal、H.264 / HEVC、CFR / VFR / B-frame、正確な PTS、所有権、転送量と不正 metadata の拒否。
- [MCP-003](mcp-003.md): 公式 SDK、HTTP / stdio、進捗・取消・再利用と legacy 回帰。
- [COLOR-001](color-001.md): native PQ / HLG 10-bit、固定入力の同期・独立 worker、HLG 表現域外の明示拒否、8K tile と CPU 参照 ROI。
- [RECOVERY-001](recovery-001.md): 固定入力の再開、実 SIGKILL、公開境界の照合、故障注入、prune 後の result 復元。遅延した旧 controller が新 attempt を変更しない CAS も検証済み。
- [CACHE-003](cache-003.md): GPU texture / pool、外部 disk cache、同時 process、削除・破損・容量・意味キー。全 graph の数値検証成功後だけ cache を公開し、隠れた overflow を繰り返しても error を返す。
- [PERF-001](perf-001.md): release の参照シーン・履歴測定。[GPU fusion](perf-001-gpu-fusion.md)と[実 media 製品経路](perf-001-media-seek.md)を区別する。

basic 4K native preview は最後の不要な単一子 root の省略で既定 admission 内に収まり、実機検証に成功した。
complex lower-third の whole-graph preview は 1080p / 4K とも既存512MiBの保守的 admission で型付き拒否となる。
1080p は execution 1956×1116 / 45面 / 785,842,560 bytes の見積もりである。
これは実 GPU 物理使用量ではない。proxy preview と tile 化済み final を分けて測る。被覆範囲外の計算省略後、動く基本4K native preview は21 samples / 21 distinct hashes、p50 17.163ms / p95 22.139msとなった。利用者が承認したwarm p95≤33.3ms基準に合格する（[ADR-0093](../adr/0093-m4-reference-preview-performance-target.md)）。cold p95は34.166msとして別途報告し、GUI表示込みFPSとは区別する。

最適化前後のbasic 44組・complex 66組の解像度/時刻別linear/display hashと作品SHA256が完全一致した。complex tiled4K finalのwarm静止p95は8,369.600msから858.174msへ短縮した。この作品の異時刻出力は同じ画素であり、動くシーンの性能とは区別する。

1080pの共有render.frameで4つの異なる時刻を合成し、独立したf64平均・平均後のdisplay変換・fresh再描画と全画素が一致した。10ms OS samplingの最大RSS181,698,560bytes / footprint354,828,984bytes、GPU所有payload peak77,416,824bytesを記録した。CPU accumulatorの要求payload66,355,200bytesは計算値であり、OS観測やallocator peakとは区別する。whole4K temporalの既存budget拒否も維持する。詳細と全測定値は[PERF-001](perf-001.md)に保存し、PERF-001を受け入れ済みとする。

## 最終 CI と受け入れ

[run 37418631507](https://github.com/soramikan/kronello/actions/runs/37418631507) は全5jobsが成功した。検証対象の実装headは `fde889e002287903564fe8e267b93225156cdbe8`、実checkoutはmainとのmerge `275380d2eb41b15f3b7c94a0d2115c252a9a5c26`。[CI記録](m4-ci-acceptance.json)にjobs・steps・ログhashを保存する。その後の文書のみのhead `6f6f143` の再実行で、下記のSwift競合と古い証拠の再アップロードを検出した。先行成功と後続修正の検証は区別する。

| 環境 | 最終結果 |
|---|---|
| macOS Apple Silicon | Rust 755 passed / 0 failed / 38 ignored。Swift 71 tests / 0 failures / 1 skipped。fmt・clippy・実process・CPU integration・公式MCP SDK・FFI終了検証も成功 |
| Linux Mesa lavapipe | Rust 753 passed / 0 failed / 30 ignored。fmt・clippy・実process・CPU integration・公式MCP SDK・FFI終了検証も成功 |
| Windows MSVC / MinGW LGPL runtime | MEDIA-003の全6commands exit 0、必須CLI/MCP testsは各1 passed / 0 skipped。後続のstorage / operational testsは63 passed / 0 failed / 2 optional ignored |
| Linux software Vulkan | 採用済み基準との通常比較40 scenes成功。許容誤差 `2^-10` は変更していない |
| Windows software DX12 | 採用済み基準との通常比較40 scenes成功。hardware GPUの保証には置き換えない |

CI Swiftの1 skipは実機stage-2 evidenceを要求する `testStage2FFIPresentationParity`。基準機ではそのevidenceを指定して成功しており、上記ローカル71 testsにはskipがない。Windowsのoptional ignoredは追加storage fixtureと別volumeを要する項目であり、MEDIA-003の必須受け入れはすべて実行した。

利用者の指示でrepositoryをpublicに変更した後、請求制限による実行前停止は解消した。再開した実検証で次を修正し、上記の最終CIで確認した。

- Linuxのworker失敗状態保存と実process終了の間の競合: testで実際の終了とreapを待つ。生存workerの再開拒否とattempt fenceは維持する。
- FFmpeg 6.1.1のProRes stream rangeがunknownになる差: 他の形式・色・寸法が一致するnative frameのlimited rangeだけを採用する。明示的なfull rangeや不一致を許容しない（[COLOR-001](color-001.md)）。
- Windowsの拡張絶対パスとDLL名のseparator: Unicode loaderへ渡す前にWindows用separatorへ統一する。DLL検索範囲は広げない（[MEDIA-003](media-003.md)）。
- macOSの全体30分予算不足: workspace 1,018秒、実process 127秒、CPU integration 238秒を要した実測に基づきjobだけ60分へ変更した。全testsと個別timeoutは維持した。最終Swiftは408.744秒で完走し、失敗はなかった。

M4全12タスクを `done` とする。M2のSTORE-003の実環境残件は別範囲であり、ここでは完了に変更しない。

## 後続 CI の Inspector 競合修正

[run 37421914965](https://github.com/soramikan/kronello/actions/runs/37421914965) はmacOSの `testInspectionScheduling` だけが失敗し、他4jobsは成功した。テストが20msのsleep後に100msの模擬応答をキャンセルできると仮定していたが、負荷によって応答が先に完了し、次の同時刻queryがcacheに命中して型付き失敗の検証に到達しなかった。

模擬transportを明示的な応答ゲートに置き換え、要求の実送信を確認してからキャンセル・再生・応答解放を行う。キャンセル済みの遅延成功と、キャンセルされていない旧generationの遅延成功の両方を拒否し、停止後に新しいqueryを発行することを確認する。開始前にキャンセルされたrefreshが新しいrefreshのgenerationを変更する本体の競合も再現したため、入口でキャンセル状態を確認し、状態変更前に戻るように修正した。後者の回帰テストは本体修正前に失敗し、修正後は集中テスト20回すべて成功した。

同runでは未実行のMCP/FFI検証が、target cacheに残った旧checkout `275380d2` の成功JSONをアップロードしていた。macOS/Linuxではcache復元直後に3種類の検証出力だけを削除し、証拠のuploadは対応する検証stepがsuccessまたはfailureになった場合に限定する。未実行・キャンセル時の古い成功報告を除外し、実際に失敗した検証の診断は残す。

ローカル検証はSwift全71件成功・失敗0・skip0（統合evidence指定、独立state root）、集中テスト20/20回成功、workflow YAMLとstep依存・upload条件の構造検査成功。root agentがrelease FFIで再構築したアプリをCUAで直接操作し、Motionの連続時刻変更、再生中の更新保留、停止後のInspector更新と日本語字幕・258×36のbounds表示を確認した。競合の順序は決定的テスト、実画面はGUI操作で確認する。生ログと画面 `gui-inspector.png` は `target/ci-fix/` に保存する。修正を含むCIの結果はPR #8の最新headで確認する。
