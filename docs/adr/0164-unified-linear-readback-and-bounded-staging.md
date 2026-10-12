# ADR-0164: linear status/画像の待機統合と有界staging

- 状態: 採用（PERF-012受け入れ進行中）
- 日付: 2026-10-12
- 対象: PERF-012
- 部分置換: ADR-0150の画像とstatusをそれぞれ一回待つ条項。linear専用境界、graph/意味key/halo/crop、temporalの有理数sampleとf64加算順、sticky異常検出、cache公開順、通常frame/display、resident interopは維持する。

## 決定

- Movie/GIF/shutter sampleのlinear境界で、4-byte statusとrow-padded RGBA16F imageを一つのencoderでcopyし、両mapを開始して一回のGPU完了待ちを行う。全callbackとstatus正常を確認してからCPU画素を返し、pending cacheを公開する。異常statusを後段の不透明/opacityで隠さず型付き失敗にする。
- readback bufferはdevice単位の4件/16MiBのmanaged poolへ保つ。貸出中/idleを合わせたquota、size一致、idle退避を用いる。大きい/競合時のbufferは既存の一回限りのallocationとし、poolへ保持しない。queueやGPU in-flight数を増やさない。
- 完了待ちと全map callback成功後だけ再利用可能とする。mapped viewを解放してからunmapし、GPU bufferのCPU consumerが終了するまで返却しない。呼出元には独立Vecを返すため、packing/encoderが画素を保持中でも後続GPU buffer再利用がその値を変更しない。未使用/未完了/失敗bufferはdestroyしてpoolへ返さず、mapしていないbufferへのunmapを避ける。
- allocation trackerはidleを含む実descriptor保持を数え、idle pool byte報告にstagingも含める。cache clearでidle stagingを回収する。readback回数/bytesとwait数、buffer allocation/reuseとprocess RSSを別に測る。
- disk persistenceの追加readbackは独立policyとして維持する。80→40/240→120の条件は追加persistenceなしで確認し、保存転送を含む総waitを同じ数字と扱わない。GPU側量子化やtemporal加算を変更しない。

## 検証

実Metalで全画素bit、SDR/HDR/temporal/halo、mapped ownership、pool競合/byte上限、失敗非再利用、隠れたoverflowとcache非公開、fixed workerの転送/待ち/RSS/全encoded出力を[PERF-012](../testing/perf-012.md)へ記録する。CPU色変換との重なりは現行の同期tile境界とnative consumerを区別し、pool導入だけでasync overlapを主張しない。全task受け入れは検証記録を正本とする。
