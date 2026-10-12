# ADR-0158: 固定workerの有界な工程観測

- 状態: 採用（PERF-006の実装・受け入れは進行中）
- 日付: 2026-10-12
- 対象: PERF-006〜014

## 決定

`KRONELLO_EXPORT_DIAGNOSTICS=1`を実行時に指定した場合だけ、固定workerと同期deliveryの工程時間・回数・明示payloadを観測する。作品、要求、固定snapshot、意味版、codec設定、cache keyへ診断policyや時計を含めない。式・通常評価に時計を入力しない。既定は無効で、`AvExportReport.performance`も省略する。

観測器は実行所有の`Arc`であり、固定名のstage/count/peakだけを保持する。stageごとの時間sampleも最初の64個までとし、frame別の無制限traceや全画素を保持しない。caller threadでは一時的なthread-local scopeを使用し、nested scopeのreturn/unwindで元の観測器を復元する。guardはthreadを越えて移動できない。専用encoder threadには観測器だけを渡し、自分のscopeを作る。native handleや評価状態は共有しない。

stageはhostのmonotonic elapsed timeであり、GPU timestampや排他的CPU時間ではない。producer/consumer・入れ子stageの時間は重なるため合計をwall timeと呼ばない。readback/waitは従来の実transfer statsを併記する。native変換とcodecの分離など、未計測の内訳を推定で埋めない。

JobStoreのclone間で、実際に変更した明示transactionのcommit成功を共有counterへ記録する。fresh connectionの`total_changes`で書込みの有無を判定し、read-only commit、rollback、busy失敗を数えない。checkpointとheartbeatの成功も別に数える。観測範囲はworkerのJobStoreとcloneで、別processのsubmit/cancel・schema初期化・autocommit INSERTは含めない。

worker終了時には専用job directoryの`performance-attempt-N.json`へ小さい診断を記録する。成功DB commitより後の補助出力であり、書込み失敗はlogへ残して確定済みjobを失敗へ変更しない。report内の観測はreport生成時点、worker側の観測は最終publication/heartbeat終了後であり、時点を区別する。診断は復旧に必要なpublication receiptの代わりにならない。

## 測定の契約

固定fixtureとsource hashを各trial前に照合し、実際の実行binaryをhashする。`render.submit`からterminal結果取得までをwallとして記録し、独立decode/hash/probeは計時外とする。caseごとのstate rootと作品外のraster cacheを隔離する。実worker PIDをDBから取得し、100 msのRSS sample最大値をkernel peak RSS・payload・GPU資源と区別する。

同じbinaryを両側に置いた試行は再現性検査であり、改善前後の性能差の証拠として使わない。新workerを毎trial起動する場合は「初回/後続fresh worker」と表記し、同一GPU processのcold/warmと混同しない。短尺のA/Bはpairごとに順番を反転する。長尺完走、定常メモリ、metadata境界、Metal基準と他backendは独立に受け入れる。

## 検証

scopeの無効化・thread共有・unwind復元と、DBのrollback/read-only/write commitを通常テストで検証する。全工程とfixed fixture matrixの受け入れは[PERF-006](../testing/perf-006.md)で追跡し、個別の計数確認だけで完了としない。
