# ADR-0162: 直接色変換と同条件の動画delivery共有

- 状態: 採用
- 日付: 2026-10-12
- 対象: PERF-010
- 部分置換: ADR-0152のper-leg packed buffer/encoder所有。最大二組のframe ownership、独立runtime thread、PTS/span、GIF第二段階、失敗join、publicationは維持する。

## 決定

- tileのpremultiplied linear画素からbundleのstride/offsetへ直接書く。SDRは旧f32 composite/BT.709/byte丸め、HDRは旧f64 composite/transfer/u16丸め、HDR→SDRは旧中間Vecのf32保存境界を維持する。checked pixel count/stride/offset/長さ検査を全書込みより先に行う。失敗した部分frameは送信・公開しない。
- 背景のf32 bit列・SDR/HDR transfer・tone-map有無・packed formatが同じlegを最初のownerへ寄せ、ownerだけに二組のframe bufferを確保する。aliasは空のslotで、consumerがownerを参照する。queueを増やさず、全producer/consumer/recycle/finishを合わせた二組の保持を維持する。payloadとcodec/driver資源/RSSは別に測る。
- 同じpipelineに属するMovieのnative video encoderを、変換owner、video codec、HDR transfer、DNx native profileが同じ場合に共有する。pipeline全体でraster・全PTS/span・frame count・bitrate/codec thread/preset policy・runtime/capabilitiesを固定し、constructorへ渡すper-leg変数をすべて照合する。新しい可変encoder設定を追加する場合は共有keyも更新する。異なるvideo profileを推測で同一扱いしない。
- 不変private video stageを各legの最終muxへ渡す。音声、container、chapters、export snapshot hash、最終probe/hash、no-clobber、lease/cancel、全leg失敗時回収を共有判定へ混ぜず、各legの既存処理で維持する。shared video reportは既存pathへのsuffixと追加transfer bytes=0で実作業を表す。
- GIFは動画encodeを共有せず、palette/spool/第二段階を独立させる。同条件のpacked色変換だけ共有し、第二段階の読み戻しはowner bufferを再利用する。native pointer/Rc/error bufferをthread間で渡さない。
- native RGBA→YUVのconverterを明示option設定・初期化へ変更し、source converterは変更しない。変換threadsの比較は閉じたcompile-time値1/2/4に限り、暫定既定1とする。codec threadsはProRes4/DNx2、SVT `lp=1`を維持する。threads2/4の採否とleg別workerの必要性は実測後に記録し、本決定だけで採用済みと扱わない。

## 検証と制限

旧独立color oracleを使い、transfer分岐の隣接f32、alpha、非有限/範囲外入力、stride/offset/padding/overflowをbyte比較する。共有keyの差異、native重複stage、transfer二重計上防止、GIF第二段階、cancel、typed consumer失敗を通常テストで検証する。実Metal workerの固定fixture・独立decode/packet/chapters・binary hash・工程時間・RSSと残る受け入れは[PERF-010](../testing/perf-010.md)を正本とする。採用したownership契約と全taskの受け入れ完了を区別する。
