# PERF-001 参照シーンと履歴の release 測定

PERF-001の受け入れ条件を確認した。正式目標は利用者承認済みのADR-0093に従う。以下の各参照作品・入口・資源観測の範囲に限定し、全作品の4K30、GUIの物理display FPS、未知のdriver/codec allocationを保証しない。

## 再現方法

配布用 LGPL FFmpeg runtime、固定 Noto フォント、Rust 1.95.0 を既存手順で準備する。build/test/GUI と他の benchmark が停止した区間に測定する。`--build` は release binary に production source manifest の識別子を埋め込み、driver は一致しない binary を測定結果として受理しない。出力先は毎回新規 directory とする。

```sh
# 実際の lower-third snapshot は M3 の実機検証作品から保存したもの。
WGPU_BACKEND=metal python3 scripts/perf_001_history.py --kind render --build \
  --project docs/testing/perf-001-lower-third.project.json \
  --output target/perf-001-evidence/lower-third-final

# 別の基本参照: 1 shape の既存 FFI preview 作品。complex scene の代替とはしない。
WGPU_BACKEND=metal python3 scripts/perf_001_history.py --kind render --build \
  --project examples/ffi-preview.project.json \
  --output target/perf-001-evidence/basic-final

python3 scripts/perf_001_history.py --kind history --build \
  --project docs/testing/perf-001-lower-third.project.json \
  --output target/perf-001-evidence/history-final
```

JSON の作品入力は共有 `project.create` API に渡して独立した `.kronello` を作る。既存の `.kronello` を指定した場合も共有 `project.export` の文書から測定する。元の作品を編集しない。lower-third snapshot は日本語 headline、色入力、横/縦 variant、composition instance、rational time map、アニメーション curve と sequence を持つ。追加の基本参照は `FFI preview` の 1 shape/1 curve であり、作品と source hash を別々に記録する。

## レンダーの分類

- complex lower-third: proxy 960×540、full 1920×1080、tiled final の 3840×2160。native preview は1080でも admissionを超え、`UNSUPPORTED_FEATURE` / n=0を記録した（execution1956×1116、45 surfacesの保守的推定785,842,560bytes、上限536,870,912bytes）。4K native previewも別の admission 境界として typed error と DAG の計算を記録する。
- basic FFI shape: proxy 960×540 と full 3840×2160 について native preview/final を同じ matrix で測る。complex 4K preview の成功を意味しない。
- native preview は共有 `Service::preview_dag`、GPU texture 作成、明示的な完了待ちまで。コンパイル/layout、GPU submission、追加の完了待ちを別々に記録する。GUI presentation、物理 display FPS、音声との同期は測定していない。
- final は共有 `render.frame` API の bounded tile 実行と linear/display 出力境界。全体時刻にはコンパイル、layout、GPU、readback を含むが、PNG、movie encoder/mux、CLI の大きな JSON serialization は含まない。native movie 製品経路は [media 測定](perf-001-media-seek.md)で別に扱う。
- `context_cold_static` は新しい GPU context/pipeline/resource cache を timing 内で作る。OS disk cache と driver cache は消去しない。warm は同一 context を保持し、1回の warmup を除外する。既定 cache は textures/pool 各64MiB。調整済みの大容量 cache を既定値として報告しない。
- `warm_static` は時刻3/2秒の反復であり停止画面の cache 効果である。basic の `warm_animated_30fps` は時刻0..20/30秒の21要求で、実際の linear/display hash が複数になることを必須とする。complex は元作品を変更せず `warm_distinct_times_30fps` とし、実測 hash 数と `actual_pixel_variation` を記録する。layout 制約によって画素が静止する場合は motion の証拠にしない。33.3msとの比較には実際に画素が変わる animated case を用いる。

各 warm/cold case は21 samples。percentile は nearest rank（`ceil(p*n)`、1起点）である。sample数、全 samples、request time、転送/待機/dispatch counters を raw JSON に残す。温度一定や母集団の信頼区間を主張しない。

全時刻の pixel oracle は timing 外の fresh context による共有 final API と比較する。native preview texture は execution halo を含むため、DAG の `crop_origin`/`execution_region`/要求 region で切り取ってから同じ LinearRec709 plane と完全比較する。比較の tolerance を広げない。完了した case は source fingerprint とともに `completed-cases.json` に即時保存し、後続の unsupported case が既存結果を消さない。

## メモリーと admission の区別

10ms sampling の OS RSS/physical footprint は process 全体の観測 peak であり、取り逃しのない allocator peak ではない。GPU の counted descriptor payload は [GPU fusion 測定](perf-001-gpu-fusion.md)の実所有 guard が計数する graph/cache surfaces、control/readback buffers、resident input、output copy と node別 peak を使う。driver private/native codec 内部の未公開 bytes を0と置かない。host linear/display Vec の capacity bytes は別に記録する。これらをそのまま加算して物理メモリーと呼ばない。

native preview の512MiB guardは `nodes + roots + group extra + effect extra + 3` に execution pixels とRGBA16Fの8bytesを掛ける **保守的な admission surface estimate** である。同時 live allocation の実測値ではない。最終 OutputTransform が指す直前の singleton・opacity=1 の synthetic root は GPU と同じ限定条件で elideし、その nodeとgroup extra2を除く。basic/complex の admission、実際の counted peak、OS観測値を別に比較する。final は512px tile の有限寿命であり、native preview の whole graph と同じ admission 経路ではない。

lower-third/basic には video がなく、text は vector outline を描くため独立した bitmap atlas を使わない。decode current/lookahead、変換 temporary、movie encoder frontend、および codecを含む process RSS は media 製品測定に記録する。single-sample still の accumulation と、temporal/HDRのより大きな資源要求を混同しない。

## 履歴と ADR-0052

同じ実作品に共通 Command API で opacity property を追加し、rename/tag/opacity と template headline の変更を128回、selective undoを32回、history queryを128回行う。巨大な表示名を挿入する合成 root patch 測定ではない。template/property意味検証、materialized instance、共有 edit plan/apply とundoを通す。

復元は connection-cold/warm 各21回で、revision別の保存済み文書と完全比較する。検証用 snapshot 読み出しと benchmark の42復元を別に数える。この頻度は **scripted workflow の回数** であり、人間の利用頻度や製品 telemetry ではない。

macOS は `proc_pid_rusage(RUSAGE_INFO_V4)` の `ri_diskio_bytesread/written`、Linux は `/proc/<pid>/io` の `read_bytes/write_bytes` を生存中のphase境界で読み、実プロセスの物理I/Oを記録する。OS cacheで復元 read_bytes が0でもSQLite復元が実行されなかったことにはならない。SQLite VFSへの syscall別帰属・fsync回数は未計測。DB/WAL file size と、保存された snapshot/mutations/inverse のUTF-8 payload bytesは物理I/Oと別に記録する。

[ADR-0052](../adr/0052-snapshot-policy-evaluation.md)の debug 合成測定を本 release 実作品測定の根拠に置き換えない。opt-in snapshot、数/容量予算、root patch重複削減の採用変更は本測定だけで決めない。実測値と限界に基づく再検討は [ADR-0090](../adr/0090-release-performance-evidence-and-snapshot-policy.md) に記録した。

## 初期実行と未完了の境界

- 最初の native pixel oracle は uncropped execution halo と final plane を比較して失敗した。DAG crop contractを適用した proxy 検査では全画素完全一致した。製品バグや許容誤差変更として扱っていない。
- 次の complex 4K native preview は既存512MiB admissionで `UNSUPPORTED_FEATURE` を返した。元の上限を上げず、作品も置換せず、この境界を保存する。
- 最終 source freeze と root workspace/GUI gate の後に各 matrix・expanded history・GPU fusion の release値を確定した。この時点のOQ-14は下記の最適化前未達結果を示して利用者へ確認した。後続ADR-0093で正式目標を確定し、最適化後の測定へ適用した。

## 最終 release 測定（2026-10-06）

実機は Apple M4 / RAM 32GiB / Metal。GUIを閉じ、他の build・test・測定を停止した逐次実行である。実行時 revision は `4b75d40` + dirty worktree。各 binary・実行時 source manifest・作品の SHA256 は [永続化した測定 JSON](perf-001-measurements.json) に保存した。complex実行後に harnessの省略可能なtexts配列の扱いだけを修正したため、basic/historyのsource IDは別である。production sourceは同じである。全時間 sample、OS counters、要求別 allocation/transfer/cache countersを保持し、tileの同じ node番号に対する peak observationsだけ最大値と件数へ集約した。

全 measured render case は n=21、nearest rank。native previewのhalo crop後の全 linear画素と finalの linear/display hashを厳密照合した。basicの変化する21時刻は21個の異なるhash。complexの変化する時刻は全て同じhashであり、元作品のlayout制約による静止画素として扱う。

| 作品 / 解像度 / 入口 | context cold p50/p95 ms | warm静止 p50/p95 ms | warm異時刻 p50/p95 ms |
|---|---:|---:|---:|
| basic / proxy / preview | 17.861/20.234 | 2.737/3.130 | 4.525/6.885 |
| basic / proxy / final | 61.847/63.720 | 45.475/48.901 | 49.627/51.663 |
| basic / 4K / preview | 55.751/56.972 | 8.729/9.548 | 43.956/45.782 |
| basic / 4K / final | 433.051/442.446 | 420.831/428.519 | 420.948/433.812 |
| complex / proxy / preview | 420.078/432.978 | 179.324/180.657 | 179.341/180.052 |
| complex / proxy / final | 486.096/488.750 | 226.503/228.953 | 225.906/228.089 |
| complex / 1080 / preview | 未対応 n=0 | 未対応 n=0 | 未対応 n=0 |
| complex / 1080 / final | 1623.050/1698.304 | 1612.105/1648.774 | 1613.674/1684.744 |
| complex / 4K / final | 8073.458/8375.442 | 8037.388/8369.600 | 8027.905/8336.845 |

basic native4Kの動く画素は p50 43.956ms / p95 45.782ms。33.3msを達成していない。停止画面の8.729msを再生性能へ流用しない。これは最適化前の結果であり、当時のOQ-14は未決だった。後続の正式目標は[ADR-0093](../adr/0093-m4-reference-preview-performance-target.md)で利用者が承認した。最適化後の結果は以下の補足に保存する。GUI presentationと実時間再生の計測ではない。

complex native1080は execution1956×1116・45 surfacesの保守的推定785,842,560bytes >536,870,912bytes、native4Kも同じ上限を超える typed unsupported。tiled4K finalは全要求成功した。未対応caseに時間のゼロ値を補わない。

### 資源観測

basic native4K animatedでは要求ごとの tracked owned GPU payload peakは最大398,133,624bytes、idle poolは最大66,355,200bytes。graph/cacheは同じ所有物の部分集合であり重複加算しない。10ms OS samplingの同phase最大RSS44,679,168bytes / physical footprint544,359,000bytesは別の指標である。compile/layout p50は1.139ms、GPU submission（sticky validation待機を含む）42.943ms、追加completion wait0.000791ms。1要求はstatusの4bytes readback・明示backend wait1回に加えconsumer fence1回、pixel upload/copy無し。

complex tiled4K warm静止は tracked owned GPU peak最大123,796,516bytes、idle pool64,726,016bytes、CPU linear/display Vec容量265,420,800bytes。OS sample最大RSS412,745,728bytes / footprint638,043,048bytes。1要求40tilesのtransfer合計はreadback120回 /185,794,720bytes、wait120回、dispatch1520回。全体max RSS426,852,352bytesは別caseの観測であり同時合算しない。

これらはdriver alignment/private allocationを数えないdescriptor payloadで、OS sampleも真の瞬間最大値を保証しない。独立atlasは使用せず文字はoutline、stillのtemporal accumulationは1sample。decode pool / CPU変換clone / codec込みRSSは [実1080 media movie測定](perf-001-media-seek.md)、GPU融合のbefore/afterとnode peakは [GPU融合測定](perf-001-gpu-fusion.md) を参照する。codec private poolとSQLite VFS別のI/Oは未測定。

### 実作品の保存・復元

同じlower-thirdを共有APIで新規作成し、通常のproperty opacity変更32回、template headline変更32回を含む129 edits、32 selective undo、128 history queryを実行した。revision162。別に全revisionの完全参照snapshotを162回取得して正しさを照合した。この参照呼出しを利用者の復元頻度に含めない。historical restoreはscripted42回（cold/warm各21回）、人間のtelemetryではない。

| API | n | p50/p95 ms |
|---|---:|---:|
| edit_plan_apply | 96 | 8.069/8.867 |
| restore_connection_cold | 21 | 12.820/23.717 |
| restore_connection_warm | 21 | 11.510/23.263 |
| selective_undo | 32 | 5.947/6.897 |
| template_set_input | 32 | 8.908/9.383 |

固定64revision周期の完全snapshotは3件/35,791bytes、events162件のmutations105,134bytes / inverse87,424bytes。root置換は初期importの1回だけで、通常編集をroot Setへ置換していない。close後DB851,968bytes、WAL0bytes。これらは論理payload/file sizeである。実OS process diskioはediting read4,096/write40,759,296bytes、cold restore read0/write688,128bytes、warm restore read0/write0bytes。OS disk cacheはpurgeしておらず、cached read0はDBアクセス無しを意味しない。SQLite/WAL VFS個別byte・fsync回数へ帰属させない。

ADR-0052のdebug合成・root置換中心の候補比較とはworkload、build、percentileの統計量が異なり、その速度値を直接before/afterとして扱わない。今回の実作品releaseでは固定周期の最悪63patchを含む復元p95約23.7ms、DB約0.81MiBを観測した。自動追加snapshot候補の物理I/Oを今回比較しておらず、全作品の既定変更を正当化する根拠は不足する。既存64revision周期/明示compact/履歴自動削除禁止を維持し、頻繁な大型作品復元や確定した遅延目標が生じたら候補の容量・物理I/Oを同じrelease workloadで比較する。

最終harness `cargo clippy -p kronello-service --examples --locked -- -D warnings` は成功した。production全workspace/実Metal/Swift/GUIの統合検証はrootの受け入れ記録を正本とする。

## 被覆範囲最適化後の補足（2026-10-06）

[ADR-0093](../adr/0093-m4-reference-preview-performance-target.md) の正式基準はM4 Mac mini32GB / Metal /既定64MiB texture cache+64MiB pool、基本4Kの画素が変わる21時刻のwarm native preview p95≤33.3ms。coldは別報告であり、この合否を適用しない。動画decodeとGUI presentation/物理display FPSは含まない。

GPUの保守的なcoverage範囲外計算省略後、同じbasic matrixの12cases×21samplesが厳密fresh oracleに合格した。全44組の解像度/正規化有理時刻のlinear/display hash mapとsource document SHA256は最適化前と完全一致した。[追加測定JSON](perf-001-coverage-temporal-measurements.json)に新しいbinary/source manifestを保存した。新manifestはWGSL/Metal/Objective-C++も含み、build前後・実行後のsource一致を検証する。最適化前のraw JSONは変更しない。

基本4K warm animatedはp50 **17.163458ms** /p95 **22.138833ms**、21時刻で21個の異なるhash。最適化前43.955666/45.782250msから短縮し、正式warm目標に合格した。cold staticは31.440709/34.166000ms、warm静止は8.910916/9.462167ms。後者を動く画素の性能へ流用しない。

rootの最適化後workspace/native/Swift/GUI gateとapp終了後、別のexclusive quiet windowでcomplex matrixと4sample temporal資源観測を完了した。全production sourceは凍結し、driverが実行終了時にもsource一致を確認した。historyは保存層が変わっていないため元のrelease実作品記録を維持する。

### 実4sample temporal accumulationの資源観測

```sh
python3 scripts/perf_001_history.py --kind temporal --build \
  --project examples/ffi-preview.project.json \
  --output target/perf-001-evidence/temporal-1080-coverage-after
```

同じ動く基本shapeを**1920×1080の共有render.frame**、time1/2、24fps、shutter180°、phase−1/4 frame、4samplesで1回処理した。Serviceのsnapshot callbackは公開されていないため、4Kstreaming APIを追加せず既存のsupported1080経路を測る。CPUaccumulatorはwhole regionであり512tileのaccumulatorと呼ばない。sampleのGPU描画は各12tiles、4samples計48sample tiles。whole4K要求は測定外で`UNSUPPORTED_FEATURE: temporal accumulation budget exceeded`を確認し、count×96=796,262,400bytes >536,870,912bytesの既存guardを維持した。未対応4Kを資源測定成功として扱わない。

実metadataは時刻63/128、191/384、193/384、65/128、weight各1/4を返し、4個の異なるlinear hashを確認した。測定外のfresh contextで共有APIの各瞬間frameを描き、独立したf64 weighted mean全画素と、平均後のdisplay変換をSHA256で厳密照合した。fresh共有temporal再描画もlinear/displayとも一致し、time1/2の瞬間frameとは異なる。

測定phaseの10ms OS sample最大RSSは**181,698,560bytes**、physical footprintは**354,828,984bytes**。これは実temporal処理を含む観測値であり、瞬間の真のphysical peakを保証しない。untimed oracleの大きな参照配列はこのphaseに含めない。GPUのtracked owned descriptor payload peakは77,416,824bytes。CPU f64x4 accumulatorの**requested payload**は1920×1080×32=66,355,200bytes（sourceからの計算）であり、allocator capacity/overheadや実physical peakと同一視しない。4samplesは同じaccumulatorへ逐次加算する。返されたlinear/display Vecの観測capacityは合計66,732,032bytesで、瞬間sample出力・cache挿入前のtemporary clone・GPU/readback面は別の寿命を持つ。単純に各分類を足して同時physical peakと呼ばない。

実転送合計はdispatch240、readback144回/132,710,592bytes、backend wait144回、pixel upload0/copy0。control upload116,160bytes/768回。1回496.915msは資源条件の確認用でありp50/p95や正式preview性能のサンプルに流用しない。動画decode/atlas/encoderはこのshape処理で使わず、decode/encoder込みprocess観測は既存movie測定を参照する。driver private memoryと内部accumulatorのallocator peakは未知。追加raw JSONはrequest、全sample/weight、tile plan、transfer/allocation/cache counters、OS観測、source/binary SHAと独立oracle結果を保持する。

### 最適化後の全matrix

各verified caseは21samples。complex native1080/4Kのtyped unsupportedはn=0で維持する。complex全66組のresolution/time linear/display hash mapとsource document SHA256も最適化前と完全一致した。異時刻のcomplexは全て同じ画素であり、表の異時刻列をmotionの合否に使わない。

| 作品 / 解像度 / 入口 | context cold p50/p95 ms | warm静止 p50/p95 ms | warm異時刻 p50/p95 ms |
|---|---:|---:|---:|
| basic / proxy / preview | 14.645/15.485 | 2.907/3.248 | 3.420/4.119 |
| basic / proxy / final | 56.781/58.146 | 42.683/45.499 | 45.201/47.432 |
| basic / 4K / preview | 31.441/34.166 | 8.911/9.462 | 17.163/22.139 |
| basic / 4K / final | 401.917/410.666 | 387.716/395.254 | 385.822/398.357 |
| complex / proxy / preview | 191.258/194.585 | 169.162/170.599 | 170.361/171.025 |
| complex / proxy / final | 251.568/255.526 | 216.757/221.623 | 216.749/222.796 |
| complex / 1080 / preview | 未対応 n=0 | 未対応 n=0 | 未対応 n=0 |
| complex / 1080 / final | 376.781/383.443 | 366.979/378.572 | 362.973/371.949 |
| complex / 4K / final | 848.706/875.630 | 836.819/858.174 | 837.334/864.614 |

最適化後basic warm動作4Kのtracked GPU peakは398,133,624bytes、idle pool66,355,200bytesで前と同じ。OS sampled RSS45,449,216bytes / footprint537,035,376bytes。complex tiled4K warm静止のtracked GPU peak123,796,516bytes /idle pool64,726,016bytesも同じで、sampled RSS417,398,784bytes /footprint642,728,848bytesだった。資源使用量の改善は主張しない。既定budget・surface/admission guard・型付き数値失敗契約は維持した。

最適化後basic/complexを合わせ24verified cases×21=504samples、1080/4K nativeの4typed unsupported境界、独立temporal資源1runを完了した。GPU fusion before/after252samples、media forward/backward/repeatedと実movie、通常編集の実作品履歴/物理I/Oを別のraw記録とともに保持する。正式warm基準と資源条件を満たしたことは、これらの対象経路内に限定する。
