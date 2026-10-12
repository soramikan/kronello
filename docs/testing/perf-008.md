# PERF-008: 素材presentationと変換結果の共有

2026-10-12受け入れ済み、`done`。[ADR-0160](../adr/0160-shared-presentations-and-converted-source-cache.md)に従い、独立decoderを維持したまま、不変native presentation、変換前source cacheとSwsContext保持を追加した。

## 予備A/B

改善前はPERF-007の`788edfeee37d1b99e83cecfd462678ed3b40f38f52e36eaa24c8a6792ce5cc98`。予備改善側は`8ba65fa067d7cb59681731e054211975888f6478b118c750ea9ac269a94ec6a7`。固定4K 0/3/8 samplesと同素材異開始点/速度の4case、各9 pair、全72試行でsnapshot identity、全decoded bits/layout/PTS/DTS/duration、encoded bytesが一致した。

| case | 全素材RGBA変換 before → after | 後続fresh worker p50 before → after ms |
|---|---:|---:|
| 4K 0 samples | 40 → 1 | 1,824.6 → 1,747.7 |
| 4K 3 samples | 120 → 2 | 5,427.1 → 5,117.9 |
| 4K 8 samples | 320 → 2 | 12,390.7 → 11,643.0 |
| 同素材 source_in 0秒と1/4秒・speed 1/2 | 80 → 2 | 3,591.1 → 3,409.5 |

3/8 samplesの要求は2つの実presentationに分かれ、それぞれを共有した。0 samplesのRGBA8累積生成は1,327,104,000→33,177,600 bytes。SwsContextは1回作成した。入力/出力のpixel/hash/PTSと独立cursorの意味を変更して回数を減らしたものではない。

予備binaryのSDR cache weightはLUTのArc本体だけを数え、2048-byteのLUT配列を漏らしていた。最終ソースでこれも含めたため、この予備binaryの保持counterを最終予算の証拠として採用しない。最終binaryを再採取する。速度値も非静穏hostの観測として区別する。

## 検証の進捗

lib/media/compositionの回帰32件と追加lib16件は成功した。native ownership・LRU、全画素/identityのworking/reverse/HDR設定対照、独立cursor、期待hash変更/物理書換拒否、fresh converterの8/10bit/RGBA・サイズ/range/matrix・反復を確認した。実PQ/HLGとflow warm cache、最終binary A/B、全workspace/fmt/Clippy・schema/Swift checkは受け入れの残件。PERF-009〜014を含む全計画はまだ未完了。

## 最終受け入れ

[最終72試行の記録](perf-008-worker-comparison.json)。改善側を`c7e639b65cec202c56726f5dabf6ba0d8a50e5899d17f11d245dcb8b49518aa1`へ固定して4caseを各9 pairで再採取した。全snapshot identity、全decoded bits/layout/PTS/DTS/duration、encoded bytesが一致した。変換40→1・120/320→2・80→2を再確認した。

| case | p50 before → after ms | p95 before → after ms | retained converted payload bytesの一例 | sampled worker RSS before → after bytesの一例 |
|---|---:|---:|---:|---:|
| 4K 0 samples | 1,878.1 → 1,781.1 | 1,903.8 → 1,803.0 | 33,180,690 | 500,170,752 → 488,914,944 |
| 4K 3 samples | 5,636.1 → 5,301.1 | 5,836.0 → 5,915.1 | 66,361,380 | 559,071,232 → 588,627,968 |
| 4K 8 samples | 12,506.9 → 11,537.8 | 13,608.7 → 12,077.3 | 66,361,380 | 559,120,384 → 575,946,752 |
| 同素材の異開始点/速度 | 3,648.9 → 3,462.4 | 3,688.7 → 3,546.8 | 66,361,380 | 834,650,112 → 855,965,696 |

cache保持はpixel/sampler/LUT/keyのlogical payload。RSSは100ms標本でkernel最大RSSではない。保持を増やして累積生成を減らしたため、RSSが必ず減る変更ではない。hostは非静穏で、warm frameとfresh workerを分離し、一般作品の速度倍率を約束しない。

frame cacheのLRU/zero/byte超過とreader寿命、native Arc pointer同一性とcopyゼロ、独立cursor、working space/reverse/HDR設定、期待hash変更・書換拒否、fresh converterのformat/range/matrix/サイズと全bytes対照が成功した。追加の実PQ/HLG warm cacheとflow合成時刻の対照も成功した（lib16、HDR2、flow8）。

最終runtimeのworkspace testは1,350 passed、0 failed、47 ignored、200 suites、終了コード0（`/private/tmp/kronello-perf008-workspace.log`）。Clippy（warnings denied）、fmt、生成Swift check、backlog checkも成功。PERF-008の3受け入れ条件を確認しdoneにした。PERF-009〜014は未完了であり、依頼全体は継続する。
