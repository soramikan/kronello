# M4 統合受け入れ

正本は [backlog.json](../backlog/backlog.json)。12 タスク中 11 タスクを受け入れ済みであり、M4 全体の完了宣言ではない。
MEDIA-003 の Windows native 実行と、再開した最終全 OS 検証の失敗修正が残る。QA-004 の両環境通常比較は完了した。
変更は [PR #8](https://github.com/soramikan/kronello/pull/8) に統合する。

## 基準機と最終ローカル検証

2026-10-06、Apple M4 / Mac mini Mac16,10 / 32 GB、macOS 27.0.1 (26A434)、Rust 1.95.0。
生ログは target/m4-acceptance/ に保存する。途中のテスト件数を足し合わせて最終 gate の件数として扱わない。

| 検証 | 結果・証拠 |
|---|---|
| fmt / workspace all-targets clippy | 成功。final-coverage-fmt.log / final-coverage-clippy.log |
| workspace test | 754 passed / 0 failed / 38 ignored。final-coverage-workspace.log |
| SwiftPM、統合 evidence 指定 | 48 tests / 0 failures / 0 skipped。final-coverage-swift.log。8つの landscape / portrait と時刻の組で Inspector / Viewer の共有 FFI parity も含む |
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

## CI と外部実行制限

Linux full workspace / native / SDK / process と Vulkan golden 通常比較は
[run 37401385847](https://github.com/soramikan/kronello/actions/runs/37401385847) で成功した。
Windows DX12 も同 run で CPU oracle 40 scenes を通過し、root の画像確認と clean revision baseline 採用を完了した（[QA-004](qa-004.md)）。
これらは後続の HDR / recovery / cache / performance 変更を含む最終全 OS gate の代用ではない。

Windows FFmpeg configure が WSL の bare bash を拾った問題は、23dde7a で検証済み absolute MSYS2 bash に修正した（[MEDIA-003](media-003.md)）。
その後の23dde7a、4b75d40、6384858の CI は全 job が step 開始前に終了した。
[run 37412738412](https://github.com/soramikan/kronello/actions/runs/37412738412) の annotation は account payment failure または spending limit の引き上げが必要と報告している。
利用者へ Actions 再開の確認を依頼し、有料設定は変更していない。
Windows native 修正の再検証、Windows baseline 通常比較、最終全 OS gate は未完了である。利用者のリセット連絡後に同runを一度再実行したが、attempt 2のWindows annotationも同じ請求・利用上限理由でstep開始前に停止した。

性能受け入れ済みの25dc305をpushした[run 37414848745](https://github.com/soramikan/kronello/actions/runs/37414848745)も、全5jobsがsteps=[]のまま停止した。Windowsのannotationは同じaccount payment failure / spending limit理由だった。未検証の2タスクはin_progressを維持し、外部のActions実行再開を待つ。

## public 変更後の検証再開

利用者の明示的な指示によりrepositoryをpublicへ変更し、run 37414923914 attempt 2の全5jobsが実際に開始した。請求制限による実行前停止は解消した。Linux Vulkan / Windows DX12は採用済み基準の通常比較で各40scenes / 20,950pixels、mismatched pixels=0。全actual PNGも基準とbyte一致し、QA-004を受け入れ済みとする（[証拠](qa-004.md)）。

同runのLinux全体検証でHDRメタデータ確認とworker失敗直後のresumeテスト2件が失敗した。23960feでテストに実workerの終了待ち・reapを追加し、HDRの期待値/実測値を型付きエラーに含めた。生存workerの再開拒否とHDRの検証基準は維持する。後続CIで修正を確認する。

67a0b50の[run 37417246467](https://github.com/soramikan/kronello/actions/runs/37417246467)でLinux全体検証・実process・公式MCP SDK・FFI終了検証が成功し、HDRとresumeテストの修正を実環境で確認した。Windows実行では拡張絶対パスとDLL名の間にforward slashが入ることによるloader error 126を確認し、b98af22でWindows用separatorに統一した。DLL検索範囲は維持し、実Windowsの再検証を進める。

macOSの初回再開jobは全体30分上限でFFIテスト中にcancelされた。先行Rust検証に失敗はなく、workspace 1,018秒、実process 127秒、CPU integration 238秒を要し、FFI step開始がjob開始約25分後だった。Rust/Swiftビルド後には約112秒しか残らなかった。全test・個別timeoutを維持し、macOS jobだけ60分へ変更する。Linuxは15分22秒で完了しており30分を維持する。
