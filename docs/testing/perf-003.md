# PERF-003: revision単位のプレビュー準備・独立decoder・表示索引

2026-10-11。[ADR-0144](../adr/0144-revision-scoped-preview-preparation.md) のsession接続保持、検証済みsnapshot/font/LUT、static sequence lowering、既存semantic cache、revision単位のproject.infoと、[ADR-0146](../adr/0146-independent-bounded-media-cursors.md) の独立decoder cursor・個数/byte予算を実装した。[ADR-0147](../adr/0147-indexed-timeline-and-proxy-preparation.md) のPrefer cache・GUI proxy選択・timeline索引も接続した。修正後のworkspace一括検証を成功させ、バックログのPERF-003を `done` にした。

## 測定条件と結果

Apple M4 / Mac16,10、32GiB、macOS27.0.1（26A434）、Rust1.95.0、release。元からあった未commitの変更を含む作業ツリーを使用した。source hashと全要求値は [測定JSON](perf-003-measurements.json) に保存した。他のbuild/testが動くホストでの探索測定であり、quiet hostの性能目標受け入れではない。

同じCompositionを100/1000/5000クリップへ配置し、64x32のpreview DAGを22個の有理数時刻で準備する。初回をcold、残り21回をwarmとし、p95は昇順21値の20番目。GPU描画、動画decode、GUI表示を含まない。大規模な同一参照の再検証を調べる作品であり、5000本の同時表示でもない。

warm p50（ms）の段階別探索値：

| clips | 変更前 | snapshot/font/LUT保持 | 全体検証の証明を保持 | static sequence loweringも保持 | reachable検証の重複除去後 |
|---:|---:|---:|---:|---:|---:|
| 100 | 4.451 | 1.727 | 1.192 | 1.254 | 0.832 |
| 1000 | 122.467 | 87.767 | 37.835 | 35.849 | 8.057 |
| 5000 | 2551.260 | 1524.153 | 755.216 | 708.391 | 46.006 |

同じ最新版の実行ファイルによるstateless control / session比較（ms）：

| clips | control cold | control warm p50/p95 | session cold | session warm p50/p95 |
|---:|---:|---:|---:|---:|
| 100 | 7.727 | 3.644 / 4.961 | 7.226 | 0.832 / 0.969 |
| 1000 | 35.014 | 35.085 / 38.079 | 27.297 | 8.057 / 8.399 |
| 5000 | 412.721 | 390.890 / 395.132 | 331.849 | 46.006 / 67.331 |

各caseの22時刻すべてでDAGのDebug表現をSHA-256へ渡した比較値が一致した。独立したraw/prevalidated DAGの構造比較も通常テストで行う。これらの値はGPU画素のgolden hashではない。

保持には1準備 / 21hitを観測した。初期のcold値（5000クリップ3075ms）も重複検証の除去により縮小した。一方、残るwarmのcompile/eval費用があるため、大規模作品の30fpsを保証しない。

OSメモリー測定は別試行で `/usr/bin/time -l` を使用した。3サイズの作成から評価までを通したプロセス全体のmaximum RSSはcontrol370,458,624bytes、session369,704,960bytes。peak footprintはそれぞれ221,790,976 / 220,496,640bytes。GPU面やcache自体のpayload量と混同しない。この試行ではホスト負荷が増え、5000クリップのwarm p50がcontrol415.915ms / session151.109msとなった。静かな環境の反復測定なしに安定した改善倍率とはしない。

FFIの16.667ms間隔・購読あり・5000クリップは、通知期限の分離だけのp95 175.683msから、revision再利用込みの探索測定ではp95 4.189msとなった。notification/query自体とOSのqueue待ちをstderr traceで区別する。後者にはホスト負荷による最大72.922msがあり、入力から表示までの保証値ではない。

```sh
cargo build --release --locked -p kronello-service --example perf_preview_prepare
target/release/examples/perf_preview_prepare target/perf-improvements/control stateless
target/release/examples/perf_preview_prepare target/perf-improvements/session
/usr/bin/time -l target/release/examples/perf_preview_prepare target/perf-improvements/rss-session
cargo build --release --locked -p kronello-ffi -p kronello-cli
KRONELLO_FFI_TIMING=1 python3 scripts/measure_ffi_polling.py \
  --interval-ms 16.667 --output target/perf-improvements/ffi-prepared.json
```

## 正しさと残件

- sessionの5テスト成功：time/regionの変更とraw DAG一致、外部commit後のrevision fence/hash更新、未知意味版のvalidation proof拒否、同size/mtimeのfont書換えと欠落、LUT書換えと欠落、file replacement拒否、安全モードのunwind/排他。
- `perf003_prevalidated_sequence_preserves_reverse_boundaries_and_request_order` 成功：有理数NTSCの先頭/途中/終端と逆順・反復要求でraw DAGと完全一致。
- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets --locked -- -D warnings` 成功。`perf_decoder_pool` exampleもrelease buildとClippyに含めた。
- GPUにアクセスできる環境の `cargo test --workspace --locked` では通常テスト1,289件が成功（44件ignored、受け入れ成功に数えない）。この実行中の別作業による `ObjectDiagnostic` 追加と古いmodelバイナリの不一致でstoreのdoctestが失敗し、コマンド全体はexit 1だった。現在のsourceを再buildした `cargo test --workspace --locked --doc` は成功した。先の実行もsource変更によるdoctestの依存版不一致があったため、どちらもworkspace一括成功とは扱わない。同一sourceでの一括再実行は後続変更の最終gateに残す。
- 以下のPrefer cache・GUI proxy切替・timeline索引を追加した。測定は探索値であり、quiet hostでの反復と動画/フォント/多数effectを含む実時間表示の受け入れは後続性能検証に残す。

## decoderの変更前測定

`kronello-media/examples/perf_decoder_pool.rs` は640x360 / 30fps / 4秒の生成MPEG-4素材を使用する。1/2/3/4/8クリップを同時配置し、各clipのsource_inを1/4秒ずつずらした24要求を測定する。別素材caseは同じ生成ファイルの独立したlocal copy、同一素材caseは1つのAssetを共有する。DAG準備を計測外に置き、software decode・色変換・64x36へのsamplingを含める。GPU/GUI経路ではない。初回をcold、残り23回のwarm p50は昇順12番目、p95は21番目とする。

変更前のwarm p50/p95（ms）、ホスト上では全体testが動いており探索測定：

| clips | 別素材 p50/p95 | 同一素材の異時刻 p50/p95 |
|---:|---:|---:|
| 1 | 1.756 / 4.405 | 6.026 / 8.621 |
| 2 | 6.681 / 20.676 | 14.815 / 29.003 |
| 3 | 41.446 / 86.680 | 14.466 / 33.595 |
| 4 | 30.237 / 52.766 | 25.060 / 47.050 |
| 8 | 76.443 / 223.357 | 40.695 / 83.403 |

別素材3/4/8ではpool hitが0、missが72/96/192となり、2個のLRU枠による毎要求の再openが確認できた。同一素材2/3/4/8では1個のdecoderを共有し、残っているdecoderのseekが各24回だった。eviction済みdecoderのseekはこの一覧に含まれないため総seek回数と呼ばない。各時刻のRasterInputの全f32 bit hashを保存し、後続変更のsoftware経路の比較に使う。生成コマンド・実行ファイル/source hash・全要求値は [decoder測定JSON](perf-003-decoder-measurements.json) に記録する。

```sh
cargo build --release --locked -p kronello-media --example perf_decoder_pool
target/release/examples/perf_decoder_pool \
  target/perf-improvements/pool-source.mov target/perf-improvements/pool-before
```

## 独立decoder cursorの変更後比較

SceneKeyをcursorとして渡し、既定8個・512MiBのLRU保持に変更した。初回の前後比較は各24要求×10case、全240のRasterInput bit hashが一致した。比較を前→後の順で3回交互に反復し、各試行の全240比較値も一致した。1素材はカーソル競合がなく、期待どおり大きな改善はない。

各試行のwarm p50/p95をさらに3試行の中央値でまとめた値（ms）：

| clips | 別素材 before p50/p95 | 別素材 after p50/p95 | 同一素材 before p50/p95 | 同一素材 after p50/p95 |
|---:|---:|---:|---:|---:|
| 1 | 0.406 / 0.449 | 0.459 / 0.555 | 0.421 / 0.508 | 0.397 / 0.458 |
| 2 | 0.799 / 0.991 | 0.901 / 1.120 | 2.112 / 2.411 | 0.792 / 1.038 |
| 3 | 6.514 / 7.686 | 1.231 / 1.520 | 2.576 / 2.912 | 1.214 / 1.383 |
| 4 | 9.069 / 10.514 | 1.603 / 1.885 | 3.883 / 4.330 | 1.582 / 1.927 |
| 8 | 24.531 / 29.260 | 3.264 / 3.483 | 7.472 / 9.407 | 3.172 / 3.699 |

別素材1/2/3/4/8のafterはmiss1/2/3/4/8、hit23/46/69/92/184、seek累計1/2/3/4/8、eviction0。同一素材の異時刻caseも同じ値になった。8素材の変更前miss192が8になり、同一素材の共有カーソル24seekが独立8seekとなった。afterのseekはeviction済みdecoderも含む新しい累計から取る。byte制約が先に当たる大きな素材ではevictionを許容し、同じ性能を保証しない。

OSメモリーは10case全体を通した別プロセスの `/usr/bin/time -l`。beforeのmaximum RSS76,972,032bytes / peak footprint62,259,824bytesに対し、afterは138,117,120 / 123,437,752bytes。8カーソル保持にはcodec内部を含むメモリー増加がある。8素材の保持native presentation bytesは1,382,400→5,529,600bytesで、RSSと同一ではない。afterのRSS測定は交互試行1の開始と重なったため、所要時間は性能受け入れ値にしない。

プロセス観測ではsyspolicyd105.9%・Python53.2%・WindowServer41.1%等の負荷があり、quiet hostではない。初回と交互試行の絶対値の差を無視して倍率を主張しない。全試行・source/実行ファイルhash・RSS logを [decoder測定JSON](perf-003-decoder-measurements.json) に保存した。

通常テスト `perf003_nested_media_cursors_survive_order_changes_and_retention_limits` が成功。同じauthored MediaNodeを独立instanceへ配置し、root描画順を毎回反転してもcursorが安定し、5時刻すべてでfresh decodeのDAG全体とinput identityへ一致した。count1・byte1・count0・byte0の保持制限でも画素が一致し、累計seekを失わない。既存の資産hash変更拒否とpool2個/128MiBテストも成功した。media composition / media / track003の回帰は24件成功、1件ignored（受け入れ成功に数えない）。

## タイムライン表示索引とGUI proxy

`TimelineIndex` / `TimelineAssetIndex` は共有Queryのclip/track/sourceを索引化する読み取り専用の派生値である。EditorModelで文書またはQueryが変更された時に置換し、既存の編集API・revision conflict・選択検証を使う。Motion/Programとsource monitorのpreview request/keyへOff/Preferを接続し、optionalなUI stateに保存する。transportは[900pxの1段](perf-003-transport-900.png)と[420pxの2段](perf-003-transport-420.png)をNSHostingViewで描画し、時刻・操作・proxy選択の収まりを目視した。

releaseの `KronelloTimelineBenchmark` は16素材・8track・100/1000/5000clipの生成QueryをMainActorへ採用し、duration/selection/track/lock/label/missingの全参照を12回行う。初回cold、残り11回のp50は6番目、p95は10番目。実際のSwiftUI layout、FFI、GPU、動画描画を含まない。変更前と全12回のchecksumが一致した。最終probeではQuery採用/索引構築費を別に測る。

| clips | before cold / warm p50/p95 ms | after query update ms | after cold / warm p50/p95 ms |
|---:|---:|---:|---:|
| 100 | 23.277 / 18.926 / 19.097 | 0.178 | 0.567 / 0.037 / 0.040 |
| 1000 | 190.587 / 189.955 / 196.077 | 1.370 | 1.381 / 0.353 / 0.377 |
| 5000 | 945.257 / 948.512 / 954.177 | 4.356 | 6.900 / 1.784 / 2.480 |

通常Swiftテスト2件で外部文書更新・素材名・欠落状態・lock・rate/尺・clip削除・選択解除・旧UI stateのdecode・Prefer保存・未知モードのOff化・未知SourceRef fieldの非同一扱いを確認した。これは全SwiftUI描画費がこの値になるという主張ではない。

## 原本/proxy比較と検証済み準備の保持

同じ作品の4K HEVC（3840x2160 / 30fps / 2秒）から実際に1920x1080 ProRes proxyを作り、320x180へsoftware samplingした。22時刻の初回coldと残り21回warmを測る。p50は11番目、p95は20番目。GPU/GUI表示は含まない。両モードで選択streamの寸法と全出力floatの有限性を検査し、各出力のSHA-256を記録した。proxyは品質が異なるため原本とのbit一致を受け入れ条件にしない。

| mode | prepare cold / warm p50/p95 ms | decode cold / warm p50/p95 ms | retained native bytes |
|---|---:|---:|---:|
| Off | 1.760 / 0.569 / 0.981 | 121.577 / 20.209 / 33.319 | 24,883,200 |
| Prefer | 61.242 / 0.526 / 0.778 | 121.398 / 17.407 / 19.217 | 16,588,800 |

Preferのcoldにはproxy全量hash検証を含む。各モードで準備1回/hit21、decoder miss1/hit21/seek1を確認した。保持native bytesはcodec内部/GPU面/プロセスRSSと同じではない。絶対値にはホスト負荷が入り、30fpsを保証しない。全要求・試行・生成コマンド・実行ファイル/source hashは [timeline/proxy測定JSON](perf-003-timeline-proxy-measurements.json) に保存した。

Preferはprune前のproxy descriptorとcanonical locatorのfingerprintを準備keyに追加する。通常テスト `perf003_prepared_proxy_fences_missing_reappearing_and_changed_files` が成功し、ready→missing→復帰→同size/mtimeで破損→復帰→Offの各DAGがstatelessと一致した。既存proxy_trackingの計6件とsessionの5件も成功した。

proxy置換の確認で、元streamの非zero start PTSと原本pixelでのcropをdecodeへそのまま渡す既存不整合を修正した。選択proxyのstart PTSへ有理数で平行移動し、decode cropだけを各軸の実寸比で変換する。authored extent/時刻/プロジェクトは変えない。Composition・Sequence・Multicamの正/負start PTSとcropの意味的比較が成功し、既存reframe計4件も成功した。上の測定素材は原本/proxyともzero start・cropなしであり、この後続修正で計測対象のmappingは変わらない。さらに実際の640x360動画をstart PTS=2秒・duration=2秒で作り、zero起点proxyの生成後、Off/Prefer各22時刻のDAG準備・software decodeがすべて成功した。全出力floatの有限性とsource寸法も確認した。これは非zero PTSの縦断回帰であり、この追加試行の所要時間を性能受け入れ値とはしない。

GPU software-upload resident対CPU previewのignoredテストを明示実行し、5時刻の最大誤差0.0087890625が既定0.02以内で成功した。残るignored native residentテスト4件は実行成功に数えない。固定GPU画素goldenではない。

再実行例（Xcode SDKを選び、release最適化を維持する。debug info省略はこのsandboxでのdSYM生成制限への対応）：

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
CLANG_MODULE_CACHE_PATH=/tmp/kronello-perf-clang-cache \
SWIFTPM_MODULECACHE_OVERRIDE=/tmp/kronello-perf-swift-cache \
swift build --package-path apps/macos --disable-sandbox -c release -j 3 \
  --cache-path /tmp/kronello-perf-spm-cache -debug-info-format none
apps/macos/.build/out/Products/Release/KronelloTimelineBenchmark
apps/macos/.build/out/Products/Release/KronelloTimelineBenchmark \
  --render-controls target/perf-improvements/ui-controls-hosted
cargo build --release --locked -p kronello-service --example perf_proxy_preview
target/release/examples/perf_proxy_preview \
  target/perf-improvements/proxy-source-4k.mov target/perf-improvements/proxy-4k-new
```

proxy probeの出力ディレクトリは既存project/outputを置換しない新規パスを使う。入力生成コマンドは測定JSONに保存した。

Swiftのrelease回帰は、最新のrelease CLI/FFIを `apps/macos/Libraries` へatomic置換した後、EditEditor/PrecisionEdit/SourceMonitor/TimelineIndexの28件を成功させた。最初の逐次試行はsandbox外の既定job stateへのアクセスで11件が `JOB_STORAGE_ERROR` となったため、受け入れ成功には数えない。再試行は `/tmp` の `KRONELLO_STATE_ROOT` と3 workerの独立test processを使用し、各testのsetenv/unsetenvが次のtestへ漏れない条件で確認した。

```sh
KRONELLO_STATE_ROOT=/tmp/kronello-perf003-swift-state \
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
CLANG_MODULE_CACHE_PATH=/tmp/kronello-perf-clang-cache \
SWIFTPM_MODULECACHE_OVERRIDE=/tmp/kronello-perf-swift-cache \
swift test --package-path apps/macos --disable-sandbox -c release -j 3 \
  --cache-path /tmp/kronello-perf-spm-cache -debug-info-format none \
  --skip-build --parallel --num-workers 3 \
  --filter 'EditEditorTests|PrecisionEditTests|SourceMonitorTests|TimelineIndexTests'
```

修正後の一括試行でも、MODEL-004の既知不正オブジェクト早期検証に未追従だった旧renderテスト3件が失敗した。無効gradient/dashのモデル型付きエラー、対象collection/UUID/reasonの `InvalidObject`、snapshot構築時の早期拒否を確認するようテストを更新した。動的なopen alignment/budgetと未知versionの後段診断は従来どおり確認する。3件の個別再試行は成功した。一括試行中にRust/native/Cargoソースの変更がないことを確認し、変更前の失敗をbinaryの不一致とは扱わない。productionの診断経路を緩める変更ではない。更新後ソースの一括再試行はdoctestまでexit 0で成功した。

## 最終受け入れ

`cargo test --workspace --locked` が同一のRust/C/header/Cargoソースで成功した。通常テスト1301件成功・44件ignored、doctest2件成功・0件ignored。ignoredは成功数へ含めない。実行開始/終了の対象ソースhashに変更がないことを確認した。fmt・workspace all-targets Clippy・Swift release回帰28件も成功した。コマンド・対象hashの集約・release実行ファイルhashと実行範囲は [検証receipt](perf-003-validation.json) に保存した。

これはPERF-003の準備再利用・独立cursor・GUI proxy/表示索引の受け入れである。書き出し転送・音声source共有はPERF-004、長尺seek・DSP checkpoint・永続VT・GPU面の範囲/寿命はPERF-005に残り、性能改善計画全体の完了とはしない。
