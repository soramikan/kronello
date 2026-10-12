# ADR-0160: 不変presentation共有と変換前source cache

状態: 採用（PERF-008受け入れ済み）
日付: 2026-10-12
部分置換: ADR-0146のpresentation bufferの所有を不変共有へ変更する。独立cursor key、個数/byte予算、資産再検証、正確なPTS/endとreverse/flow/HDR契約は維持する。

## 決定

- `VideoDecoder`は現在のpresentationを`Arc<DecodedVideoFrame>`で保持する。内部render/flowは共有所有を返し、同一提示区間へのアクセスでnative画素をコピーしない。既存の公開`decode_at`/`decode_at_reverse`はowned `Vec`の互換入口として明示的にコピーする。copy counterは実際のコピーを記録する。
- `MediaSession`はdecoderを各SceneKeyごとに進め、presentationと色/stream lockの検証後、RGBA変換の直前にsource cacheを判定する。独立cursorの状態を共有cacheへ置き換えない。
- keyはcanonical path、stat fingerprint、runtime/decoder、full locked Asset、stream、実presentation PTS/end、サイズ/format/color policy、working space、HDR設定、reverse、interpolationを含む。flowだけは合成要求時刻も含む。通常sourceは同じpresentation内の異なる要求時刻で共有できる。出力ROI/crop/transformはfull source画素を変えないため、従来どおり後段のraster identityへ加える。
- 各アクセスの前後で素材を再検証し、変換/reuse中のfingerprintまたはcanonical path変更を拒否する。期待hashの変更・書換・欠落・decoded streamの不整合をcache hitで隠さない。
- `VideoSource`のsample callbackを不変`Arc`所有とする。SDRはpacked RGBA8と256-entry f64 LUTを保持し、必要な出力画素だけlinearへ変換する。HDR/flowは既存のlinear f32 imageを共有する。
- source cacheは最大8件・256MiBのconverted pixel/sampler/key payload。key/pathとSDR LUTもweightへ含める。超過・zero予算では保持を省き、意味を変えず再変換する。decode presentationの8 cursor/512MiBと独立した予算であり、実行中の返却source、codec/converter内部、GPU/allocator/RSSは別に観測する。
- native runtimeはRGBA8/RGBA64のconverter slotを各1個だけ保持する。src/dst format、サイズ、matrix、range、filterの一致でSwsContextを再利用し、違えば解放・初期化する。filter=BILINEAR、threads=0は従来どおり。runtimeのRust `Rc`でthreadに閉じ、km_closeでlibraryのunload前に解放する。

## 検証

native Arcの同一性とcopyゼロ、count/byte LRUとeviction後のreader、CFR/VFR/B-frame/negative origin/seek、独立cursor、working space/reverse/HDR/flow時刻、期待hash/書換拒否を確認する。converterはfresh runtimeを対照に8/10bit/RGBA、サイズ・range・matrix・反復で全bytesを比較する。固定Metal workerは[PERF-008](../testing/perf-008.md)に、保持payloadとRSSを分けて記録する。
