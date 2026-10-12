# PERF-007: 書き出し準備とsample SceneIR共有

2026-10-12受け入れ済み、`done`。[ADR-0159](../adr/0159-prepared-export-and-bounded-sample-scenes.md)に従い、共通media export入口へ`ExportRenderSession`を追加した。固定workerも同じ入口を使う。

受け入れには、通常/temporalの画素bit比較、3/8 samplesの評価回数、frame数に依存しない検証・hash・静的準備、font/LUTの不変identity、cut/ROI/halo、4096 samplesの容量上限と失敗伝播、実Metal workerの固定A/Bとworkspace検証が必要。実装しただけではdoneにしない。

基準binaryはPERF-006の`cd9ef76095725aa1cf38da854140dae00d9519d1bc1708e07afbe14b2b5dbcfc`。比較は同じcase・source・runtimeを用い、結果のidentityや画素を変更して速度を比較しない。

## 実Metal worker A/B

[全72試行の記録](perf-007-worker-comparison.json)。baselineは上記、改善側は`788edfeee37d1b99e83cecfd462678ed3b40f38f52e36eaa24c8a6792ce5cc98`。各case 9 pairを交互に実行し、全pairでrender/export snapshot identity・全decoded bits/layout/PTS/DTS/duration・encoded bytesが一致した。

| case | SceneIR before → after | canonical/evaluation hash before → after | 後続fresh worker p50 before → after ms |
|---|---:|---:|---:|
| 4K 0 samples | 1 → 1 | 4/1 → 4/1 | 1,972.8 → 1,968.0 |
| 4K 3 samples | 121 → 3 | 124/121 → 4/1 | 6,102.0 → 6,021.7 |
| 4K 8 samples | 321 → 9 | 324/321 → 4/1 | 14,210.0 → 14,064.3 |
| 3,000 clip＋日本語字幕/blur、12frame | 12 → 12 | 15/12 → 4/1 | 19,521.3 → 17,495.0 |

worker全体の構造検証はそれぞれ5回へ固定され、session内部は1回。static sequence preparationは各export 1回。3 samplesは中央sampleを共有し、8 samplesは中央を含め9時刻を共有した。4K素材変換は依然40/120/320回で、PERF-008の改善対象である。

hostは非静穏で、Clippy/workspaceテストを並行実行した。速度値はこの測定条件の観測であり、一般的な作品の速度や静穏時の因果倍率とは扱わない。first/subsequent fresh workerと同じworker内のfirst/warm frameを記録し、単一frameにwarm frameを捏造しない。p95・payload・RSS・native/GPU host工程はJSONに残す。

## 検証の進捗

`prepared_export`の5件は成功した（`/private/tmp/kronello-perf007-unit-final.log`）。日本語全画素bit、frame数によらない不変処理、3/8 samplesのタイル間共有、cut/halo、HDR/zero shutter、4096 samplesの件数/byte上限、巨大sceneの非保持fallback、font所有とmissing/hash mismatch、sink失敗を確認した。LUTの全画素bit・欠落/破損拒否の追加1件も成功した（`/private/tmp/kronello-perf007-lut.log`）。

workspace Clippy（warnings denied）、fmt、生成Swift checkは成功した。全workspaceテストは`/private/tmp/kronello-perf007-workspace.log`で進行中。追加テストを含む最終検証と、受け入れ条件の最終照合までは`in_progress`を維持する。

## 最終受け入れ

PERF-007 runtimeの全workspaceテストは終了コード0。1,343 passed、0 failed、47 ignored、200 suites（`/private/tmp/kronello-perf007-workspace.log`）。後から加えたHDR/zero shutterとLUTの追加確認は前節の5件＋1件で成功しており、workspace集計とは別に記録する。最終Clippyは`/private/tmp/kronello-perf007-clippy-final.log`で終了コード0。固定worker72試行、CPU bit比較と不変処理回数、cut/halo、font/LUT、4096 samples・容量超過fallbackを三つの受け入れ条件へ対応付け、PERF-007をdoneにした。依頼全体のPERF-008〜014は引き続き未完了。
