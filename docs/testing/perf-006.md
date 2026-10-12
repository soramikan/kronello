# PERF-006: 固定workerの追加書き出し計測

2026-10-12着手。`in_progress`。依頼計画の全範囲は[保存した計画](performance-export-plan.md)、PERF-006〜014の受け入れ条件はバックログに登録した。

## 計測基盤

[ADR-0158](../adr/0158-scoped-export-execution-measurements.md)に従い、任意の工程観測、worker専用の診断ファイル、明示DB write commitのcounterを追加した。既定のreportには診断を出さず、snapshot/content identityへ混ぜない。

`scripts/prepare_export_perf_cases.py`は固定IDの作品に4Kの動くMPEG-4素材を一つ配置し、0/3/8 shutter samplesのcaseを生成する。sourceは生成後にhash固定し、色tag欠落はnullとして実probeを保存する。SDRの既存明示defaultを適用し、存在しないtagを捏造しない。素材生成は計時外。

`scripts/perf_export_worker.py`は同じcaseとprojectを使い、実`render.submit`→worker→`succeeded`まで計測する。pairごとにA/B順を反転し、両側の実行binary SHA-256、固定case/document/source identity、sample range、独立全画素/音声decodeと有理数PTS/DTS/duration/layoutを照合する。専用state/cacheへ隔離し、DBに登録されたworker PIDのRSSを別観測する。

```sh
python3 scripts/prepare_export_perf_cases.py --output target/perf-006/fixed-cases
python3 scripts/perf_export_worker.py --before /absolute/path/to/before/kronello \
  --after /absolute/path/to/after/kronello \
  --case target/perf-006/fixed-cases/samples0.json \
  --output target/perf-006/comparison-0 --pairs 9
```

0/3/8の各caseを別々に実行する。診断のstage時間は入れ子とthread間で重なり、合計時間やGPU timestampではない。native RGBA→YUVとcodec時間は現在一つのstageで、分離は残件。sourceの全RGBA8変換の回数/bytesはnative入口で数える。

## 初回の再現性確認

同じrelease binaryの両側で4K1frameの再現性を確認した。0 samplesでは素材RGBA8変換40回・累積1,327,104,000 bytes、3 samplesではSceneIR評価121回となり、計画の構造上の重複を実workerで確認した。初回の試行はsubmit応答のPIDがnullのためRSSが未取得だった。DBから登録済みPIDを追跡するよう測定器を修正し、作品外の専用cacheとともに再採取する。初回の時間差を改善率として扱わない。

## 専用state/cacheの基準測定

[全54試行のraw記録](perf-006-worker-baseline.json)。同じ固定release binaryの両側をpairごとに反転した再現性確認。各条件18回、各側の初回1回と後続8回を区別する。hostは非静穏で、同一GPU process内のcold/warmではない。

| 4K1frame | 素材RGBA8変換 | SceneIR評価 | 一方の後続p50/p95 ms | sampled worker RSSの一例 bytes |
|---|---:|---:|---:|---:|
| 0 samples | 40 | 1 | 1,841.2 / 1,895.2 | 500,400,128 |
| 3 samples | 120 | 121 | 5,494.0 / 6,167.3 | 547,782,656 |
| 8 samples | 320 | 321 | 12,730.9 / 13,487.2 | 560,693,248 |

全pairでrender/export snapshot hash、encoded bytes、独立decoded bits、音声layout/sample range、PTS/DTS/durationが一致した。源RGBA8累積生成量は0 samplesで1,327,104,000 bytes/frame。RSSは100 msでのsample最大値であり、cache保持payloadやkernel最大RSSではない。defaultのdisk raster cacheを作品外の専用directoryへ有効にしたため、この素材ケースのreadback/waitにはdisk persistence追加転送も含む。計画の単純shapeケースの80 waitsと直接同一視しない。

## 追加matrixと診断の互換性

[追加ケースの工程・counter・oracle記録](perf-006-worker-matrix.json)。各4試行（2 pair）の再現性確認で、全encoded bytes/decoded bits/PTS/layoutとsnapshot hashが一致した。十分な性能分布の採取は残件。1万frameの全metadataを含む177 MiBのraw artifactはtarget内に保持し、チェックインした記録には正確なcount/byte長/digestと先頭・末尾を残した。これは製品のreportを有界化した変更ではない。

- 同素材を別trackで1倍/2倍に配置: 4K1frame、素材変換80回、独立cursorの構造を維持。
- 3,000 clip、日本語CaptionDocumentと共有Composition、blur: 1080p12frame、SceneIR12回・canonical hash15回・evaluation hash12回。字幕/font/blurを実Metalで実行した。
- 601秒stereo PCM16 sourceの末尾1秒をauthored rangeとし、1/24秒をMOV＋FLACへ出力: source decode1回、audio mix2回。全sourceのspool準備を経て、各出力の最終partial blockとsample rangeを検証。
- 同じ短区間を音声単独FLACへ出力: 映像backend呼出し0、2000 audio frames、checkpoint7,046回。ある試行のcheckpoint host時間は約2.16秒（全worker約3.02秒）。
- CPU reference・2×2・100fps・1万frame GIF: checkpoint20,002回、metadata1万件、ある試行のpublication receipt/hash/write約9.83秒、checkpoint約6.78秒、sampled RSS565,198,848 bytes。時間は入れ子であり加算してwallとしない。Metalと同じ表で性能倍率を比較しない。

`examples/perf-export-identities.json`は事前に割り当てて保存したUUIDを保持する。各clipとblur parameterに別identityを用い、配列番号や表示名からIDを生成しない。追加fixtureは`prepare_export_perf_matrix.py`で作る。

[診断の有効/無効比較](perf-006-diagnostic-equivalence.json)でも4試行のencoded bytes/全decoded bits/PTS/layout、snapshot hashが一致した。既定のreportはperformance fieldを省略する。

計測器の初版はstageごとにmicrosecond未満を切り捨てていた。1万回の短い処理で過小な合計になるため、現在は非公開のnanosecond remainderを繰り越して合計し、診断wire shapeを維持する。固定750 ns×1万回を7,500 usとして観測するテストを追加した。上記の保存済みbaselineは補正前であり、微小stageの時間を正確な累積値として扱わない。回数・大きなstage・画素oracleの証拠とは区別し、最終のA/Bは補正済み計測器で採取する。

## 過去のcodec測定の訂正

`perf-004-codec-delivery.json`の6ケースすべてで`all_encoded_bytes_equal`はfalseだった。54組のdecoded native bits/PTS一致の記録は残るが、本文のencoded bytes一致を訂正した。before/afterのsnapshot hashが異なり、記録されたbinary hashが同じだったため、この資料だけでは固定identityのcodec A/Bを証明できない。元JSONは変更せず保存する。

`perf_codec_delivery`は固定`examples/perf-codec.project.json`を読み、実行binaryとコンパイルに使用したfixture bytesのSHA-256、各trialのrender/export snapshot hashを記録するよう変更した。[再採取raw記録](perf-006-codec-fixed.json)。同じRust source/fixtureを二つのbinaryへ固定し、一時workspace側のnative `media.c`だけProRes/DNxを1threadへ戻した。SVT lp=1は両側で維持する。before→after→after→beforeの4batch、6case×9trial×2比較=108 pair（合計216出力）で、全render/export snapshot hash、全decoded native bits/音声layout、全PTS/DTS/duration、sample range/checkpoint/backend call数とencoded bytesが一致した。今回のbefore/after binary SHA-256はそれぞれ`a17d62e27668e18b5837b0b0a105180bfc035f38540a3af782ff4291301218ca`と`d45aca072ab76b2f5f11f85691d3106fb6880b285f2261692602edf743a9346e`で異なる。batch内のcold1/warm8は保存するが、全caseをtrialごとに交互実行した比較ではなく非静穏hostであり、時間差を厳密な因果倍率として採用しない。旧資料は今回の新しい証拠で置換せず、訂正履歴を残す。

## 残る受け入れ

- 全matrixの補正済み計測器と十分な反復数による再採取、異開始点/reverse/HDR/flowの対照、長尺完走・復旧と定常メモリ観測。
- native RGBA→YUVとcodec、GPU実行と完了待ち、spool/window保持量、hash/metadata増加を含む工程内訳の補強。
- codec A/Bをtrial単位で交互にする追加比較、同一process内cold/warmとfresh workerの分離、十分な反復数のp50/p95、長尺完走/定常RSSの観測。
- 生成schema/Swift型、fmt、workspace Clippy/test、実Metal・worker取消/復旧・診断無効時の回帰確認。

上記を確認するまでPERF-006をdoneにしない。PERF-007〜014の改善も未受け入れであり、この計測基盤だけで依頼全体を完了と扱わない。

## 段階的な回帰検証

初版の`cargo fmt --all --check`、workspace Clippy（warnings denied）、workspace testは成功した。通常/Doctest合計1,338 passed、47 ignored、200 suites。外部fixtureのrootを明示し、実Metalと実workerの通常テストも含む。生成schemaとSwift型を更新し、生成checkも通過した。時間精度補正後のdiagnostic unit4件は成功した。初版の全体成功を補正後や後続改善の全体成功と混同せず、受け入れ時に最終ソースで全体を再確認する。
