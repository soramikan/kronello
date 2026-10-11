# ADR-0154: 検証済み提示区間による有界の疎なシーク索引

- 状態: 採用（PERF-005受け入れ済み）
- 日付: 2026-10-11
- 対象: PERF-005のsoftware動画シーク
- 部分置換: [ADR-0091](0091-exact-forward-decoder-and-bounded-render-scope.md) の逆方向を常にoriginから復号する方針だけ。PTS/区間・色・資産lock・カーソル・所有権・エラー境界は維持する。

## 決定

同じsource/streamのcanonical originから正確に復号した要求の終点に、提示区間の証明を作る。証明は正規化したPTS/end、dimensions、native formatと5個のformat/color labels、全native planesのSHA-256を識別する。作品・snapshotへ保存せず、各VideoDecoder内で最大4096件、FIFOで保持する。native画素は索引に保持しない。通常の連続再生で全フレームをhashする案は4Kで大きく退行したため採用しない。既定ではその要求中に128以上のnative framesを実際に復号した場合だけ証明を作る。短い順方向要求ではhashしない。

証明が覆う通常`[pts,end)`またはreverse`(pts,end]`への再訪問は、記録したPTSのstream ticksをfloorしてnative timestamp seekの候補にする。候補から正確に復号した区間、全native画素、dimensions/labelsのdigestを比較してから戻り値を公開する。demuxerが対象より後へ着地した場合等には、一度だけ1秒前の候補も同じ完全検証に掛ける。これはGOP/nominal FPS/packet key flagから正しいframeを推定する規則ではない。成功した候補時刻を同じ証明へ記録し、後続の再訪問でも毎回完全検証する。

両候補のseek/decode/比較が失敗した場合は、証明の高速再開を無効化する。候補のcurrent/lookaheadを捨て、同一file/stream/codecを新しく開いてcanonical originから正確に復号する。負のoriginは先頭packetから開始し、0に丸めない。証明がない位置を候補復号から学習しない。検証済みseek後、未証明位置へ進む要求もoriginへ戻す。failed pointはFIFOから除去されるまで再試行しない。候補への明示した探索と同一経路の正確な再開であり、別asset/backend・近似時刻へのfallbackはない。

Unixのctime/inodeを含むFileFingerprintをdecoder open前後と要求前後に確認する。変更時は提示キャッシュと索引を捨て、DECODE_ERRORにする。次要求は新しいoriginから復号する。製品経路では既存の期待content hash/stream lock検査を先に通す。metadata変更を十分に識別しない他OSでは既定索引を無効にし、明示した有効化もUNSUPPORTED_FEATUREで拒否する。Unixの残余制約はADR-0140/0153のまま。

索引上限はpresentation bytesの512MiBとは別である。論理descriptor/FIFO timestamp量、allocator/capacity、native codec、戻り値、RSSを区別する。診断用VideoSeekIndexLimitsで0件を選べば元のorigin方針、明示上限は65536件まで。実行時の方針であり作品の時刻や純粋評価結果を変えない。Rc/native decoderをthread間へ移動しない。

## 検証

[PERF-005](../testing/perf-005.md)と[raw計測](../testing/perf-005-seek-measurements.json)に、保存済み変更前binary・同一binaryの索引なし対照・全native画素/PTS/end/color照合を記録する。60秒CFR/VFR/B-frame/負originの長尺往復と4K連続再生を分ける。初訪問のorigin workを高速化したとは扱わない。source書換え、reverse端点、強制digest不一致、候補破棄後のcanonical replay、FIFO上限、範囲外エラー後の回復を回帰検証する。縦断検証の完了前にPERF-005をdoneにしない。
