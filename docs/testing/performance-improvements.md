# パフォーマンス改善計画の実測記録

参照チャット「パフォーマンス改善検討」の4段階をPERF-002〜005に分けて実施した。各変更のsource/binary identity、fixture、全trial、出力照合、失敗した試作、資源量は下記の受け入れ記録とJSONに保存する。PERF-002〜005は受け入れ済み（done）。最終受け入れの状態は`docs/backlog/backlog.json`を正本とする。

## 実装

| 段階 | 変更 | 記録 |
|---|---|---|
| 固定費 | Swift release、250ms期限の通知確認、request transfer観測。proxy選択と音声単独の映像省略は後続の同じ共有APIに接続 | [PERF-002](perf-002.md) |
| 準備とカーソル | revision/入力identity単位の検証済みsnapshot/font/LUT/static lowering、独立node/instance decoder、有界8cursor、GUI proxy、timeline索引 | [PERF-003](perf-003.md) |
| 書き出しと音声 | 音声単独video呼出0、linear-only、codec別並列度、正確なpacket span、二組の有界pipeline、再検証済みPCM共有 | [PERF-004](perf-004.md) |
| 長尺とGPU | 疎な検証済みseek、正確なDSP/WSOLA checkpoint、単一sample demux/永続VT、有界native output、局所coverageと最後のconsumerでのGPU memo解放 | [PERF-005](perf-005.md) |

## 主な観測

単位はms。中央値はwarm p50。行ごとに作品・測定範囲が異なり、同じFPSやend-to-end latencyへ換算しない。探索測定はquiet_host=false、後半のGPU交互比較は同じdevice/contextで実行した。

| 測定範囲 | 比較 | warm p50: 前→後 | 実際の出力/資源確認 |
|---|---|---:|---|
| 5000clipのpreview DAG準備 | 同じbinary stateless/session | 390.890 → 46.006 | 22時刻のDAG一致。GPU/GUIは時間外 |
| 異なる動画8clipのsoftware resolver | 固定before/candidate | 24.531 → 3.264 | 240画素組一致。decode miss192→8、保持native面は増加 |
| 5000clipのtimeline索引lookup | 元実装/索引 | 948.512 → 1.784 | checksum一致。SwiftUI描画は時間外 |
| 1080p設定のFLAC単独書き出し | 固定before/candidate | 3353.517 → 9.921 | 144 video tile呼出→0、sample/byte/時刻一致 |
| 1080p ProRes書き出し | 直列/二組pipeline | 1326.616 → 761.110 | encoded/native画素/audio/PTS一致、保持量は増加 |
| 300秒PCMの再生準備 | 保持なし/保持あり | 302.617 → 3.020 | decode1/hit8、115.2MB PCM保持。2素材は上限超過で保持しない |
| 60秒CFR後半の往復seek | 固定before/candidate | 37.278 → 0.697 | 全native planes/PTS/end一致、各素材proof10件 |
| 180秒配置のstaggered DSP seek準備 | 同じbinary保持なし/checkpoint | 4535.049 → 45.288 | 18要求×3block/metersとbeforeの全bits一致。状態約25MB追加 |
| 3/2 WSOLA + DSP seek準備 | 同じbinary保持なし/checkpoint | 5403.447 → 71.260 | 正確な履歴復元。cold origin warmは残る |
| CFR/NV12 hardware要求 | 同じbinary従来/永続VT | 336.928 → 0.282 | mixed forward/backward要求、480native plane/PTS組一致 |
| 960×540共有mask GPU | 同じdevice交互全面/局所 | 29.287 → 23.875 | 全f32 bits一致、owner peak149.4→16.9MB |
| 日本語/図形/blur作品のGPU DAG | 同じdevice交互全面/局所 | 73.390 → 68.847 | 全22組のf32 bits一致、owner peak52.2→39.3MB |

36分・163.7MBの圧縮素材は従来の全clip128MiB制限で型付き失敗した。有界cursorは一つの圧縮sampleと未来output2件だけを保持して正確に提示した。ただし初めて35分へ移動すると約11秒のorigin復号が残る。

## 適用範囲

初訪問の動画seek/DSP warm、逆方向hardware origin replay、metadata走査、全量asset検査、複雑作品のGPU負荷は残る。状態共有でRSSが増える例と、DNx temporalの初回試行で時間が増える例も記録した。全条件での高速化、process RSSの削減、一般作品の30/60fpsは保証しない。

任意の元/GPU変換済み動画frame cache、SwsContextの保持、UIのviewport仮想化、GPU専用encoder色変換/量子化、hardware encoder surface渡し、HDR/10bit VT拡張、submissionの統合などは参照計画の追加候補であり、本受け入れの実装済み項目には含めない。新たな対応範囲は独立した設計・計測・受け入れが必要である。

実音声デバイス/Metal提示と最終source固定の検証は[PERF-005](perf-005.md)の最終節・receiptを参照する。音響loopbackと物理scanoutは測定しない。画素・sampleの厳密比較、GPU descriptor payload、native codec保持、whole-process RSS、実presentation数をそれぞれ区別する。

最終版のfmt・workspace all-targets Clippy・workspace testが成功した。通常1325件、doctest2件、Swift選択回帰36件、製品hardware/Metal実機6件を確認し、589 sourceのhashは終了時も一致した。47件のignoredを通常成功数へ含めない。詳細は[最終receipt](perf-005-final-validation.json)。実音声/Metalは初回3rateと検証終了後の2rate、全5回でunderrun/missing audio frames 0、seek/stop/resume sample位置一致。検証負荷下の1〜2frame遅延も残して記録した。

PR用の切り出しでは、開始時点 `d03b25e2` を `codex/performance-baseline` として保存し、共存していたMODEL-004の27ファイルと共有ファイル内の変更を除外した。上記の測定・検証receiptは当時のsource identityを保持する。MODEL-004を除いたPR用ソースの再検証は[切り出し検証](perf-pr-validation.json)に別記する。元の作業場所の変更・既存PR #17は保持した。

## 書き出しの追加計画 PERF-006〜014

2026-10-12のmain `656ecc3`を基点にした[追加計画](performance-export-plan.md)の結果は以下へ分けて記録する。上のPERF-002〜005受け入れ範囲と測定値は当時の記録として保持する。

| 項目 | 結果と検証記録 |
|---|---|
| PERF-006 | [固定実worker・全工程診断・独立oracle](perf-006.md) |
| PERF-007 | [書き出し寿命の静的準備・有界sample SceneIR共有](perf-007.md) |
| PERF-008 | [素材変換cache・共有native frame・SwsContext保持、4K変換40→1](perf-008.md) |
| PERF-009 | [時間間隔での進捗保存・取消/lease/heartbeat/復旧](perf-009.md) |
| PERF-010 | [色変換の直接書込・同条件複数出力共有・native並列度比較](perf-010.md) |
| PERF-011 | [音声mix共有・検証済みspoolの任意再利用](perf-011.md) |
| PERF-012 | [status/画像のGPU待機統合・有界staging、4K wait80→40](perf-012.md) |
| PERF-013 | [長尺metadata/receipt・ページAPI/復旧・probe共有・mux取消](perf-013.md) |
| PERF-014 | [任意featureのMetal/VT surfaceとProRes直接muxの試作・対応範囲・採用保留](perf-014.md) |

新しい長尺writerと旧receiptの互換reader、default改善と未採用の試作、保持payloadとRSS、特定fixtureの速度と一般作品の速度をそれぞれ区別する。SQLite native VFS修正とSyphon CLI初期化の検証は[SQLite](sqlite-vfs-deadlock.md)・[Syphon](syphon-cli-bundle-probe.md)を参照する。
