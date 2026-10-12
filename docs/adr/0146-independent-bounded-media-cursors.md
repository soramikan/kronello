# ADR-0146: 独立した動画カーソルと二つの保持予算

- 状態: 部分置換（[ADR-0160](0160-shared-presentations-and-converted-source-cache.md): 不変presentation共有と独立の変換source予算。cursor key/検証/PTSは維持）
- 日付: 2026-10-11
- 対象: PERF-003 の media session / sequential render backend
- 部分置換: [ADR-0140](0140-media-session-resident-preview.md) のdecoder pool key・個数上限。512MiBの保持byte予算、資産fingerprint検証、SDR/HDR契約を維持する。[ADR-0091](0091-exact-forward-decoder-and-bounded-render-scope.md) のexact presentation interval・reverse restart・型付き失敗・native資源の実行スコープを維持する。

## 背景

素材単位で1個のdecoderを共有すると、同じ素材の異時刻配置が順方向再生中でも交互にseekする。2個のLRU枠は3素材以上の同時配置で毎要求の再openを起こす。640x360の生成素材による24フレームの変更前測定では、別素材3/4/8配置でmissが72/96/192、hitが0だった。同一素材の異時刻2配置は1個のdecoderで24回seekした。時間値はホスト負荷のある探索測定として [PERF-003](../testing/perf-003.md) に記録する。

## 決定

`DagNode::VideoDraw.cursor` はScene IRの `SceneKey`（authored NodeId + InstancePath）を引き継ぐ。DAGの配列位置、描画順、表示名、要求時刻からカーソルIDを作らない。同じ定義内のMediaNodeを異なるinstanceへ配置した場合も独立する。

`MediaSession` のpool keyは `(canonical path, stream index, full locked Asset, SceneKey)`。保持するのは順方向decodeのカーソルであり、outputの意味を要求履歴へ依存させない。asset検証を各アクセスで通し、正確なsource時刻・色契約と再start規約は従来のまま。cursor IDはrasterの内容identityに混ぜず、source/PTS/format/color/変換とsamplingの既存キーを維持する。

`DecoderPoolLimits` は個数と保持presentation bytesを独立して設定する。既定は最大8個・512MiB。小さい素材の1/2/3/4/8同時配置を保持でき、8K等ではbyte上限により個数より先にLRU破棄する。countまたはbyteが0なら保持を無効にする。制限超過は高速化資源の破棄であり、sourceやbackendを変えるfallbackではない。

個数上限は新規decoderを開く前、byte上限はdecode後に適用する。呼び出しをまたぐpresentation buffersの予算であり、codec内部のreference buffers、変換中の配列、戻り値、GPU面を含むプロセスRSS上限ではない。hostはsessionへの要求を直列化し、model / eval / semantic cacheへnative資源を入れない。

`decoder_stats()` は現在保持するdecoderのみ、`decoder_stats_total()` はeviction済みの累計も含める。`peak_cached_frame_bytes` の合成は単一decoderの最大値であり、同時保持bytesやRSSと呼ばない。poolの `peak_retained_frame_bytes` は各呼び出しの保持予算適用後の最大値。予算を途中で変更した場合、peakは旧予算での履歴も含む。

## 検証

- ネストした同一MediaNodeの異時刻配置でinstance pathを分け、描画順を変えてもcursorが安定すること、fresh decodeと全RasterInput/identityが一致することを通常テストで確認する。
- 個数1・byte1・個数0・byte0でも同じ出力を得て、eviction済みseek累計が失われないことを確認する。
- 従来の資産hash mismatch、CFR/VFR/B-frame/負のorigin、reverse/flow、resident uploadの各テストを維持する。
- release probeで1/2/3/4/8素材と同一素材異時刻のcold/warm・open/hit/seek・保持bytes・画素hashを比較し、quiet hostと探索値を区別する。
