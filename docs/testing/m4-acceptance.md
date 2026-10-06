# M4 統合受け入れ

正本は [backlog.json](../backlog/backlog.json)。12 タスク中 9 タスクを受け入れ済みであり、M4 全体の完了宣言ではない。
PERF-001 の OQ-14 基準確定、MEDIA-003 の Windows native 実行、QA-004 の Windows 通常比較と最終全 OS 検証が残る。
変更は [PR #8](https://github.com/soramikan/kronello/pull/8) に統合する。

## 基準機と最終ローカル検証

2026-10-06、Apple M4 / Mac mini Mac16,10 / 32 GB、macOS 27.0.1 (26A434)、Rust 1.95.0。
生ログは target/m4-acceptance/ に保存する。途中のテスト件数を足し合わせて最終 gate の件数として扱わない。

| 検証 | 結果・証拠 |
|---|---|
| fmt / workspace all-targets clippy | 成功。final-fmt.log / final-clippy.log |
| workspace test | 753 passed / 0 failed / 34 ignored。final-workspace-root-elision.log |
| SwiftPM、統合 evidence 指定 | 48 tests / 0 failures / 0 skipped。final-swift-root-elision.log。8つの landscape / portrait と時刻の組で Inspector / Viewer の共有 FFI parity も含む |
| Metal golden | 40 scenes 全件成功。target/golden/run.m4-root-elision-20261006/report.json。M1 基準と許容誤差は変更していない |
| 8K HDR 明示実行 | 1 passed。final-hdr-root-elision.log。33,177,600 pixels、linear 265,420,800 bytes、最大 linear/display tile payload 8,388,608 bytes |
| 公開 API | schema / Swift 再生成・整合検査成功 |
| 公式 MCP SDK 2.3.0 | 実 stdio / HTTP と legacy 回帰が成功。final-mcp-sdk.log |
| Python | native build regression 7件、golden adoption regression 11件成功 |
| release macOS app | build・通常の ad-hoc signing・署名検証成功。final-macos-root-elision-build.log |

ignored の実機専用項目は、通常 workspace 件数と明示実行の結果を区別する。
HDR 全体画像と原寸の日本語文字・マスク境界は root agent が目視確認した。
Rust 1.95 の strip に起因する LINKEDIT alignment 問題には、macOS release FFI の strip だけを無効にする限定的対処を行った。[原因・再現・検証](macos-linkedit.md)を参照。

## root agent による GUI 確認

CUA で release app を直接操作し、M3 実機作品の独立した SQLite backup を開いた。元の作品は変更していない。
Motion の日本語字幕・背景帯と Inspector の bounds、Edit の 7:23→8:00 の字幕切替と逆方向の復帰、Full / Quarter の解像度切替、Template の横/縦 variant、Export の入力検証を確認した。
出力先入力後の preflight は成功した。GUI から実書き出しは開始していない。

最後の synthetic output root 省略後も実画像と Inspector を再確認し、Cmd-W で Welcome、Cmd-Q で process 終了を確認した。
画像は gui-final/motion-root-elision.png、edit-cut.png、edit-full-reverse.png、export-preflight.png。
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
これは実 GPU 物理使用量ではない。proxy preview と tile 化済み final を分けて測る。測定は完了した。動く基本4K native preview は21 samples / 21 distinct hashes、p50 43.96ms / p95 45.78msであり、暫定33.3ms目標を満たさない。停止画面のp95 9.55msを再生性能に流用しない。OQ-14の正式基準は利用者へ確認中である。

## CI と外部実行制限

Linux full workspace / native / SDK / process と Vulkan golden 通常比較は
[run 37401385847](https://github.com/soramikan/kronello/actions/runs/37401385847) で成功した。
Windows DX12 も同 run で CPU oracle 40 scenes を通過し、root の画像確認と clean revision baseline 採用を完了した（[QA-004](qa-004.md)）。
これらは後続の HDR / recovery / cache / performance 変更を含む最終全 OS gate の代用ではない。

Windows FFmpeg configure が WSL の bare bash を拾った問題は、23dde7a で検証済み absolute MSYS2 bash に修正した（[MEDIA-003](media-003.md)）。
その後の23dde7aと4b75d40の CI は全 job が step 開始前に終了した。
[run 37402180651](https://github.com/soramikan/kronello/actions/runs/37402180651) の annotation は account payment failure または spending limit の引き上げが必要と報告している。
利用者へ Actions 再開の確認を依頼し、有料設定は変更していない。
Windows native 修正の再検証、Windows baseline 通常比較、最終全 OS gate は未完了である。
