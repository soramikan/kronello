# ADR-0161: worker制御確認と時間駆動の進捗保存

状態: 採用（PERF-009受け入れ進行中）
日付: 2026-10-12
部分置換: ADR-0050の固定workerのcheckpointごとの進捗/heartbeat書込み。専用heartbeat、FIFO、固定入力、no-clobber/attemptを伴う最終公開transactionと復旧契約を維持する。

## 決定

- 固定render/export workerの`WorkerProgress`はblock/frameごとのcallbackを受けるが、表示進捗の永続化と取消/lease確認を別の期限で行う。modelの有理数時間・snapshot・評価へ時計を渡さない。
- 制御確認は既定50msのmonotonic deadlineで、`query_only` connectionから該当recordだけを読む。running/PID/attempt/cancelを照合し、recovery・result hydration・heartbeat更新・writer transactionをしない。callbackが来なければ確認できず、native処理を含む実際の停止時間は間隔と別に計測する。
- 進捗は既定200ms。同じ値と減少する値を保存せず、latestの最大値を保持する。書込み側は独立した`BEGIN IMMEDIATE`でrunning/PID/attempt/cancelを再確認し、read-write間の世代交代や取消を拒否する。進捗書込みはheartbeatを代替しない。
- runtime測定policyとして`KRONELLO_PROGRESS_INTERVAL_MS=100/200/250`のみ許容する。省略は200。request・固定input・content hash・GUI設定には追加しない。取消確認の50msと専用heartbeatのpolicyは変えない。
- SQLite/process gateのbusyは従来のworker retry規約で再試行する。heartbeat pulseと独立watchdogを維持し、DBの期限停止、クラッシュ時のinterruptedと固定再開、receipt再照合を維持する。
- 描画・検証成功後に最終進捗をflushし、その後の短い公開transactionで改めてattempt/lease/cancelを照合する。失敗時はtyped errorのままstageを回収する。成功recordのcompleted=totalは公開transactionでも確定する。
- 既存の単発`JobStore::checkpoint`は他のjob/test用途の互換入口として残す。新制御gateとpersist入口はそれぞれ独立して安全条件を検証する。

## 検証

1万callbackの時間駆動commit数、同値/減少、WAL writer中のread、read-write競合、同PID新attempt、取消/lease/最終公開拒否を通常テストで確認する。test workerも`WorkerProgress`とattempt付き公開/失敗を使い、実プロセスのheartbeat/kill/FIFO/競合/復旧を確認する。実render.submitの100/200/250ms比較、実native中の取消・lease喪失・killと固定再開は[PERF-009](../testing/perf-009.md)で受け入れる。
