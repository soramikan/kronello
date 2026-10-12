# PERF-009: 固定workerの進捗と制御確認

2026-10-12着手、`in_progress`。[ADR-0161](../adr/0161-time-based-worker-progress-and-control-gates.md)に従い、取消/attempt/leaseのread-only確認と、時間駆動の進捗persistを分離した。既定は確認50ms/進捗200ms。専用heartbeatと最終公開のtransaction fenceは維持する。

## 実workerの固定A/B

[全108試行](perf-009-worker-comparison.json)。beforeはPERF-008の`c7e639b65cec202c56726f5dabf6ba0d8a50e5899d17f11d245dcb8b49518aa1`、afterは`49df8a6d1cb6c44e305c0814e2128d8850d4261b461e0aa5662983f95632afed`。各case 9 pairを交互に実行し、全snapshot identity、全decoded bits/layout/PTS/DTS/duration、encoded bytesが一致した。

| case/進捗間隔 | 後続p50 before → after ms | checkpoint writes before → afterの一例 | 全worker write commits before → afterの一例 |
|---|---:|---:|---:|
| 長尺音声の短区間、100ms | 3,477.6 → 901.6 | 7,046 → 1 | 7,052 → 5 |
| 同200ms | 2,974.0 → 874.4 | 7,046 → 1 | 7,051 → 4 |
| 同250ms | 2,960.9 → 882.4 | 7,046 → 1 | 7,051 → 4 |
| MOV＋FLAC、200ms | 3,261.9 → 1,034.0 | 7,048 → 1 | 7,054 → 5 |
| 1万frame GIF/CPU、200ms | 17,601.7 → 11,056.2 | 20,002 → 5 | 20,030 → 21 |
| 4K1frame/Metal、200ms | 1,764.9 → 1,773.1 | 4 → 1 | 8 → 5 |

全commit counterはworkerとcloneの変更を伴う明示transactionだけを観測し、submit/cancel等の別processとautocommit/schema初期化を含めない。heartbeatやpublicationのcommitと進捗persistを分ける。短音声の進捗0は永続化されず、最終1frameをflushした。長尺GIFはelapsedに応じた5進捗writeとなり、最終1万frameが正確に確定した。

100/200/250msの音声結果は同じ。hostは非静穏で、各caseを別時間に測ったため数msの差を厳密な最適間隔の証拠にしない。200msを既定として維持する。CPU referenceとMetalを混ぜて倍率を比較しない。p95・fresh worker/同一worker frame・保持payload/RSS・native工程はJSONへ残す。

## 制御・実停止・固定再開

[実制御の8試行](perf-009-control.json)。各試行は専用state/cache/source、heartbeat100ms/timeout3000msを明示した。audio spoolの成長または最初の動画source変換を観測してから操作し、API完了とOSのworker終了を別時計で観測した。少数試行の観測であり停止時間の保証値ではない。

- 音声spool中の取消: before 約24ms、after 約53msで実worker終了。確認間隔50msによる違いを表示進捗200msと混同しない。
- 4K/8 samplesのnative/GPU描画中の取消: before/afterとも約12.27秒。callbackは描画途中に来ないため確認50msでもnative停止はこの時間になる。取消応答の既存制約を改善したとは扱わない。
- lease喪失: before/afterとも約3秒でwatchdogによるworker終了を観測し、未公開のままinterrupted。固定入力のattempt1でsucceededへ再開した。
- 強制終了: PIDと正確な--jobを確認した専用workerだけにSIGKILL。実停止約19〜24ms、heartbeat期限でinterrupted、固定入力のattempt1でsucceeded。4件の再開出力は基準の全decoded bits/PTS/layout/snapshot tags/encoded bytesと一致した。

新read-only gateのWAL writerとの共存、read後/write前の取消と同PID新attempt、同値/減少write省略、1万callback→6 commit、heartbeat不変のunitは成功。実プロセスtestsのFIFO・取消・lease・kill・heartbeat競合と固定再開も成功した。最終Clippyは成功。全workspace testと受け入れ条件の最終照合が残るためdoneにはしていない。依頼全体のPERF-010〜014も未完了。
