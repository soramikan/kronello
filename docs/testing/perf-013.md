# PERF-013: 長尺metadata・receipt・mux確定工程

状態: `in_progress`。[ADR-0165](../adr/0165-bounded-export-metadata-and-receipts.md)のmovie/GIF・receiptと[ADR-0166](../adr/0166-bounded-sequence-metadata.md)の画像連番、およびprobe再利用・mux取消を実装した。10,000frame画像連番比較は完了した。以下の初期記録と追加検証を区別し、最終workspace testの完了を待つ。

## 変更前の実worker基準

[基準記録](perf-013-baseline.json)は既存の固定10,000frame GIF fixture（100fps/100秒、2×2 output、CPU reference、disk persistence無効）を`render.submit`→`succeeded`で完走したもの。sourceやdocumentは既存fixtureを使い、binaryは`1f05c4382d8400f22b87883c50ab8104f9ab0c26ae0bcd22acb99b2213a034d4`、新しいSQLite依存へ更新する前の固定buildである。一般的なmovie/GPU性能の基準ではなく、軽いbackendでmetadata経路を露出させる。

源資料は`target/perf-013/baseline-long-metadata/evidence.json`、44,829,795bytes、SHA-256 `aca0332d63fe077b7485b0221784eb255c8db1a73900e122e865fb15792db4d9`。全10,000件のframe metadataを保存している。compact表現のcanonical count/hashと源資料のbyte hashを区別する。

- submitからterminal resultまで約10,848.5ms。
- workerの`publication_receipt_hash_write`は約9,485.1ms、render/色変換は約615.4ms。
- inline frame metadataのcompact JSONは24,177,209bytes、publication receiptは24,184,560bytes。
- workerのsampled RSS peakは565,215,232bytes。source cache/native/driver等を含み、metadataだけの保持量やprocess maximum RSSと扱わない。

この1試行から一般的な速度差を予測しない。変更後は新binaryの固定hashで交互比較し、全metadataの復元、frame range/count、全decoded bits/PTS/packet、reader/query/recoveryの保持量と破損拒否を別々に証明する。小さいheader/chunkへの移行に加え、root receipt自体のartifact一覧とserialization I/Oも検証対象にする。

## 件数を変えた基準

[1,000frame基準](perf-013-baseline-1000.json)は同じ固定binary・document・100fps・2×2 outputで、rangeだけを`[0, 10)`へ変えた別workerである。両documentのclip countは0で、unused source assetは同じidentityのまま保持する。source decodeの費用を長尺metadataの増加と混同しない。

| 条件 | 1,000frames | 10,000frames |
|---|---:|---:|
| submit→terminal result | 1,529.4ms | 10,848.5ms |
| render/色変換 | 65.0ms | 615.4ms |
| receipt/hash保存 | 1,290.5ms | 9,485.1ms |
| inline frame JSON | 2,414,729bytes | 24,177,209bytes |
| receipt | 2,422,012bytes | 24,184,560bytes |
| sampled worker RSS peak | 75,284,480bytes | 565,215,232bytes |

各条件1試行で、workspace検証と並行した静穏でないhostの観測である。RSSをmetadataだけへ帰属させたり、一般的な改善率を算出する証拠にはしない。frame数に比例するserialized bytesと、それを全量保持/JSON化する現在の実装を変更後の保持上限・ページ復元と比較する。

## metadata基盤の実装中

render層へ共通header・最大64frames/4MiBの差分chunk・逐次indexのwriter/readerを実装した。headerは8MiB、pageは64frames/16MiBの上限で、manifest identityはrender hash・有理数frame rate・絶対開始frame・countに結び付ける。writerは順序/time/identityと共通部分を確認し、未完了directoryはindexを閉じてから回収する。

全体検証は全chunkのhash/長さ/count/連続rangeと元の`FrameMetadata`を検証する。ページ取得はroot/header/index全体を認証し、必要chunkだけを復元・hash検証する。未参照chunkのpayload破損検査を全ページで繰り返さず、publication/recoveryの全体検証と区別する。file名はordinalから実装が生成し、indexへ任意pathを保存しない。

単体3件で1,000framesの全復元、63件ページのchunk境界、可変transfer情報、最大保持件数/serialized bytes、未完了回収、欠落/破損/完全再hashした順序違いの拒否を確認した。旧receipt読み込みやworker publicationを確認した意味ではない。

movieの全時刻・PTS Vecは定数サイズの有理数`FrameSamplePlan`とordinal PTSへ置換した。既存のnegative/NTSC/empty/overflow grid試験とmedia streaming比較（3 passed・4 ignored）が成功し、render/media全target Clippyも成功した。frame metadataのVec自体はexport/receiptへの接続が残るため、まだ全体の有界化や速度改善を主張しない。

残件はinline上限/manifestのexport接続、publication root/indexと小さいreceipt、summary/page Query、旧receipt復旧、probe再利用・mux取消、長尺のbefore/after実worker比較と全必須検証である。PERF-014も未着手である。

## movie/GIFの接続と初期比較

movie/GIFのexportは64件/1MiBを超えるmetadataをmanifestへ保存し、reportはinline空配列とdescriptorを返す（schema_version2）。小さい既存reportはversion1のまま。失敗時は未確定metadataを回収する。workerは出力legとmetadata sidecarを同じpublication root群へ含め、逐次artifact indexのhash/count/bytesを小さいreceiptへ保存してfinal fenceで確定する。

`job.frames`は同じCommand/Query registryを使うread-only操作で、job/attempt、snapshot、range/frame rate/countとmetadata rootの束縛を検査する。旧inline receiptもページ単位に返せる。新receiptのreconciliationは全artifactを逐次検証し、旧receipt分岐を維持する。buffered JSON writerはfilesystemへtokenごとの小さいwriteを発行しない。

追加のbundle復旧2件を含む既存recovery unit9件、既存CLI worker37件（2 ignored）、65frameのページ/破損/prune試験1件が成功した。最初の実装の単一file rootに空relative pathをjoinする不具合を試験で検出し、修正後の別ログで成功を確認した。失敗ログ`target/perf-013-cli-jobs.log`を成功扱いにしない。schema/Swift生成整合、jobs/media/service/CLI全target Clippyも成功。

[交互3pairs実測](perf-013-measurements.json)と[保持packet再検証](perf-013-packet-identity.json)は同じ固定10,000frame fixtureで、before=`1f05c438…`、after=`3d0a5d95631daa3f7ed19fa5c46defb0e7c91101fd389bc0cc49dfa0ca0170f0`。全decoded pixels/PTS/layout/chapters、packet/file bytes、全semantic frame metadataが一致した。metadataは全ページから復元して順にhashし、transfer/cache/backend/input-pathの実行観測だけをsemantic比較から除いた。

| 観測 | inline before | manifest after |
|---|---:|---:|
| 後続worker p50 | 11,659.4ms | 1,836.2ms |
| 後続worker p95 | 16,543.3ms | 2,212.2ms |
| sampled RSS peak（各3試行） | 約565MB | 約25〜27MB |
| root receipt | 約24.18MB | 8,025〜8,154bytes |
| artifact index | root内の一覧 | 23,053bytes |
| receipt/hash保存 | 約10.02〜14.83秒 | 約15.7〜18.9ms |
| 最大metadata chunk serialized payload | 全frame Vec | 9,168bytes |

sourceはclipを持たない軽いCPU fixtureであり、静穏hostを保証しない。movie全般の速度改善率ではない。index/chunk読込・保持の上限は実装試験とRSSで別に確認する。新binaryにはSQLite修正も含まれるため、時間差をmetadata変更だけの因果へ分解した数値とは扱わない。

残件はimage sequenceの全frame report保持、長尺のcancel/kill/旧復旧とquery保持の追加条件、probe再利用/mux取消、最終必須検証と設計採用である。PERF-013をdoneにしない。

## 画像連番とmuxの追加実装・検証（2026-10-13）

[ADR-0166](../adr/0166-bounded-sequence-metadata.md)で画像連番へ同じinline/chunk境界を適用した。version2は個別frame JSONと全frame reportを保持せず、差分chunkにnumeric/displayの生成名・bytes・SHA-256を含める。`job.frames.files`はmetadataと同順で参照を返し、publication/recoveryは両画素fileを64KiB bufferで逐次検証する。最終pathへrelocateしたsequence JSONをreceipt hashより先に保存し、manifest directoryはprimary outputと一緒に公開する。

[1,000frame・交互3pairs](perf-013-sequence-1000.json)はmovie/GIFと同じ固定document、2×2・100fps・CPU reference・clipなしの画像連番である。before=`1f05c438…`、after=`65129ba16048c26bff6592fc0c406b2f50b53ebfa51cd0aa19eaf18b974e2289`。全semantic metadataと各frameのnumeric/display file名・bytes・hashを逐次比較して一致した。movieのencoded bytes/packet oracleは画像連番へ適用せず、比較欄はnullにした。timed workerの後で行う全payload検証はwall/RSSの測定範囲外である。

後続worker p50は13,168.9→8,447.0ms、p95は16,350.7→8,746.2ms。静穏hostではなくworkspace検証も並行し、before/afterのSQLite依存差もある。一般的な描画性能やmetadata変更だけの因果へ帰属させない。初回測定器はSequenceMetadataに存在しないmovie用identity fieldを最終比較で参照して失敗したが、6worker自体は完走済みだった。元の試行記録を保持し、全semantic metadataによるidentity比較を修正した`evidence-validated.json`を別fileへ生成した。元資料のhashも記録し、workerを再実行した成功と混同しない。

renderの65frame全復元/両payload試験、実CLIの65frameページ/最終JSON/結果prune試験が成功した。66frame workerでは65frame目のpushで最初の64frame chunkをflushし、その後にcancelまたは実process強制停止して同一snapshotのattempt1から復旧した。cancelはprivate stageを回収し、killの残骸はresumeで所有identity照合後に回収した（`target/perf-013-sequence-interruption-final.log`、1 passed、8.53秒）。rename後/DB commit前の復旧試験は旧3frameと新65frameのmovie/sequence計4caseを通し、新chunk破損を拒否してから復元・reconcileした（`target/perf-013-long-reconciliation.log`、1 passed、4.89秒）。途中の試験fixtureのAPI/flush位置/private stage path誤りによる失敗ログは成功扱いにしない。

muxはprivate stageの既取得probe2件を受け渡し、Rust側の再probeを除いた。native demuxのopen/stream infoと最終container検証は残す。C→Rustの同期callbackは開始前・64packetごと・trailer前に呼び、Rust panicをC ABI境界へ漏らさず、元のcallback errorを返す。直接取消試験はpacket境界で停止し、temp movie/metadataが一切残らないこととprobe再利用2件を検証した。全delivery試験は9 passed（`target/perf-013-delivery.log`、122.49秒）で、DNx MOV/MXF・チャプター・複数出力・音声のみ/HDRの取消を含む。

[実worker mux比較](perf-013-mux.json)はbefore=`3d0a5d95…`、after=`65129ba1…`、64×64・48frames ProRes/PCM24 MOV、CPU、交互3pairsで、全decoded bits/PTS/layout/priming/packet payload/file bytes/snapshot metadataが一致した。afterの全試行はprobe再利用2件・native取消callback5回を記録した。後続wall p50は247.4→273.9ms、p95は268.1→287.5msで、この短いfixtureでは全体速度改善を確認していない。重複検査の削減と取消境界の実装を証明する記録であり、直接muxの採用根拠へ読み替えない。チャプターの非空往復はdelivery試験で別に確認した。

最終workspace testは実行中。workspace/all-target Clippyとfmtは成功した。受け入れはまだ`in_progress`で、PERF-014の試作も残る。

最初の全workspace実行はservice API網羅fixtureに`job.frames`がないため2件失敗し、exit101で停止した（`target/perf-013-workspace-tests.log`）。成功済みのCLI40件/2 ignored・delivery9件・render77件/1 ignoredなどと、workspace全体の失敗を区別する。request fixtureと実連番成果物を正規FIFO/receipt/publicationで確定したpage responseを網羅試験へ追加したところ、Rust `ResultData`の`job_frames`デシリアライズ分岐漏れも検出した。分岐を補完した後、service API全12件が成功した（`target/perf-013-api-roundtrip-fixed.log`、18.26秒）。登録名を追加するだけで網羅集合を緩めず、全request/result schemaとstrict JSON往復を実行している。

上記固定測定binaryはwire修正前の実装である。Rustクライアント側Response読込の補完であり、測定したworkerのexport処理・JSON出力・manifest形式を変更していない。後続の修正後workspace必須検証は別ログで追跡する。途中のfixture用FIFO枠不足とwire往復失敗のログも成功扱いにしない。

## 10,000frame画像連番の完走

[交互3pairsの全記録](perf-013-sequence-10000.json)は1,000frame比較と同じ固定before/after binary・document・region・rateでrangeだけを10倍にした。6workerすべて成功し、全10,000件のsemantic metadataと20,000個のnumeric/display payloadの名前・bytes・hashが各A/Bで一致した。frame metadataはmanifestページから順に復元し、comparisonのmovie packet欄はnullである。

| 観測 | inline before | manifest after |
|---|---:|---:|
| 初回worker wall | 163,003.1ms | 83,043.3ms |
| 後続worker p50 | 151,304.8ms | 97,567.7ms |
| 後続worker p95 | 153,645.7ms | 101,597.6ms |
| sampled RSS peak（各3試行） | 662.7〜669.3MB | 23.4〜24.4MB |
| root receipt | 30,847,943bytes | 1,279bytes |
| 外部artifact index | root内の全一覧 | 2,893,652bytes（逐次writer/reader） |
| metadata chunk最大serialized保持 | 全10,000frame Vec | 27,024bytes |

1,000frameのafter RSSは23.7〜24.2MB、receiptは1,271bytesだった。10倍のframe数でもafter保持量は増大せず、共通headerと一chunk/一index entryの実装上限、別Queryのkernel maximum RSS、全frame復元と併せて受け入れを判断する。indexのdisk容量・全file hash処理時間はfile数に比例するが、index全量をroot receiptやmemoryへhydrateしない。今回も全workspace検証と並行した静穏でないhostで、SQLite依存差と個別frame JSON削減を含む。GPU/実素材性能の一般的な改善率とは扱わない。

[ページQueryの別プロセス計測](perf-013-query-memory.json)は成功済み1,000/10,000frameジョブの先頭/末尾から各64件を取得した。Darwin `/usr/bin/time -l` のkernel maximum RSSは15,777,792〜15,859,712bytes、応答は約171KBで、frame総数の10倍化に比例しなかった。chunk/header/indexを逐次読む実装上限と併せて評価する。これは測定fixtureの4プロセスであり、全作品のRSS上限を保証せず、workerの100ms sampled peakとも区別する。

## 再実行

[固定10,000frame連番case](fixtures/perf-013-sequence.case.json)と[固定MOV case](fixtures/perf-013-mux.case.json)を保存した。documentは既存の固定UUIDを使い、連番caseのunused source locatorはhashに含むデータとして維持する。clipを持たないため、そのlocatorをfetch/デコードしない。小さいregionや空sceneでmetadata経路を測る条件を変えない。before/after binaryのSHA-256を上記記録と照合してから、未使用output directoryで次を実行する。

```sh
python3 scripts/perf_export_worker.py \
  --before target/perf-012/binaries/fixed/kronello \
  --after target/perf-013/binaries/sequence-mux-v2/kronello \
  --case docs/testing/fixtures/perf-013-sequence.case.json \
  --output target/perf-013/sequence-rerun \
  --backend cpu-reference --pairs 3 --disable-raster-cache
```

1,000frame版はcaseを複製し、`submit.render.range.end`を`10/1`、`expected_frames`を1000へ変更したもの。MOV版はbeforeを`target/perf-013/binaries/manifest-v1/kronello`、caseを上記MOV case、outputを別の未使用directoryへ変える。`job.frames`のkernel RSSは成功済みafter試行のstate rootで、`/usr/bin/time -l`から同じbinaryのCPU CLIへ`{operation: job.frames, job, offset, limit: 64}`を渡す。offsetは0および`total_frames - 64`。responseのcount/total/file refsを検査し、stderrのmaximum resident set size（Darwin byte単位）を記録する。
