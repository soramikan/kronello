# ADR-0163: 書き出し音声mix共有と検証済みspoolの任意再利用

- 状態: 採用（PERF-011受け入れ済み）
- 日付: 2026-10-12
- 対象: PERF-011
- 部分置換: ADR-0079のper-leg mix/量子化。全量source decode、source layout、sample格子、codec block/priming/padding、型付き失敗、private stage/publicationは維持する。

## 決定

- 固定render hash（target、plan、意味版、source lock、gain/effectsを含む）、audio envelope/mode、ordered clips、sample範囲、layout、clippingを同一性に含め、groupごとにplan/session/mix/PCM24量子化を共有する。codec/container/背景/chaptersは音声評価を変更しない。既存snapshot/hashや共通Command/Query APIを変更しない。
- mix quantumは4096以下かつgroup最小codec block以下にする。codec自身の元block編成は一block以下のadapterで維持し、partialは最後に一回だけ送る。full blockは可能なら借用sliceを直接送る。加算順・DSP/WSOLA状態遷移は同じevaluatorを使い、clipping数を各legへ同じ値で返す。
- codecとstageをgroup内で独立所有し、すべての失敗でstageを回収する。音声映像の新しい並行処理や追加queueを導入せず、先に重複mix削減とCPU/I/Oの残る費用を実測する。
- spool再利用は任意の実行policyとして導入し、既定のprivate spoolを維持する。`KRONELLO_AUDIO_SPOOL_CACHE_ROOT`/Rust resource setterは作品/requestへ保存しない。coldの全量decode/resampleと、元source全文hash/fingerprint・全authored range検査を省略しない。range decodeを追加しない。
- cache keyはdecode cache版、executable bytes、runtime capabilitiesとlibrary bytes、asset全文hash/stream metadata/index。sourceは自分のlayoutの48kHz f32LEで保持し、target layoutへ展開しない。8件/2GiBのPCM payload、各64KiB header、4096framesにつき32bytesのchecksum indexを上限とする。headerを最後に確定し、OS file lockでwriter/validatorを排他する。競合やquota超過はprivate spoolを継続する。
- index全文をhash検証し、参照windowのPCM checksumをsample返却前に検証する。参照外PCMを毎回読み直さない。破損windowは型付き失敗で公開を止め、derivative headerを退避して後続attemptで原点decodeする。入力sourceの全文hash検査と混同しない。stage cleanup後もimmutable cache readerの寿命を維持する。LRU時のlive-reader/volume差とpublication temporary回収を検証する。

## 検証と制限

全codecのsample・layout・clipping・native block・最終partial・priming/padding・PTSと独立exportの比較、fullsourceの短区間・fresh workerのcold/warm、保持payload/RSSとCPU/I/O時間は[PERF-011](../testing/perf-011.md)を正本とする。全PCM再hashの初期案は遅いため採用しない。window検証もcold費用を含めて適用範囲を判断し、この決定だけでtaskの全受け入れ完了としない。
