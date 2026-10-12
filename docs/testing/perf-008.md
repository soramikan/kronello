# PERF-008: 素材presentationと変換結果の共有

2026-10-12着手、`in_progress`。[ADR-0160](../adr/0160-shared-presentations-and-converted-source-cache.md)に従い、独立decoderを維持したまま、不変native presentation、変換前source cacheとSwsContext保持を追加した。

## 予備A/B

改善前はPERF-007の`788edfeee37d1b99e83cecfd462678ed3b40f38f52e36eaa24c8a6792ce5cc98`。予備改善側は`8ba65fa067d7cb59681731e054211975888f6478b118c750ea9ac269a94ec6a7`。固定4K 0/3/8 samplesと同素材異開始点/速度の4case、各9 pair、全72試行でsnapshot identity、全decoded bits/layout/PTS/DTS/duration、encoded bytesが一致した。

| case | 全素材RGBA変換 before → after | 後続fresh worker p50 before → after ms |
|---|---:|---:|
| 4K 0 samples | 40 → 1 | 1,824.6 → 1,747.7 |
| 4K 3 samples | 120 → 2 | 5,427.1 → 5,117.9 |
| 4K 8 samples | 320 → 2 | 12,390.7 → 11,643.0 |
| 同素材 source_in 0/1/4・speed 1/2 | 80 → 2 | 3,591.1 → 3,409.5 |

3/8 samplesの要求は2つの実presentationに分かれ、それぞれを共有した。0 samplesのRGBA8累積生成は1,327,104,000→33,177,600 bytes。SwsContextは1回作成した。入力/出力のpixel/hash/PTSと独立cursorの意味を変更して回数を減らしたものではない。

予備binaryのSDR cache weightはLUTのArc本体だけを数え、2048-byteのLUT配列を漏らしていた。最終ソースでこれも含めたため、この予備binaryの保持counterを最終予算の証拠として採用しない。最終binaryを再採取する。速度値も非静穏hostの観測として区別する。

## 検証の進捗

lib/media/compositionの回帰32件と追加lib16件は成功した。native ownership・LRU、全画素/identityのworking/reverse/HDR設定対照、独立cursor、期待hash変更/物理書換拒否、fresh converterの8/10bit/RGBA・サイズ/range/matrix・反復を確認した。実PQ/HLGとflow warm cache、最終binary A/B、全workspace/fmt/Clippy・schema/Swift checkは受け入れの残件。PERF-009〜014を含む全計画はまだ未完了。
