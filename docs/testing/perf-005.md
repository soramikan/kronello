# PERF-005: 長尺seek・DSP状態・native decoder・GPU面の範囲と寿命

- 状態: done。software動画seek、DSP checkpoint、有界永続VT cursor、GPU範囲/寿命の実装・実測・出力照合と最終縦断検証を完了。
- 日付: 2026-10-11
- 正本: docs/backlog/backlog.json、参照計画「パフォーマンス改善検討」

## 疎な検証済みsoftwareシーク

[ADR-0154](../adr/0154-verified-sparse-presentation-seek.md)の実装を、変更前release binaryと同じローカル素材で測る。raw・全query・oracle hash・native counters・source/binary hash・RSSは[計測JSON](perf-005-seek-measurements.json)に保存した。Macの実環境でquiet_host=false。独立processを順に実行した探索測定で、静かなホスト・無作為交互比較・FPS保証とは扱わない。

60秒320×180素材4件（H264 CFR/VFR/B-frame MOV、負の開始時刻のMPEG2 TS）に42〜58秒の前後往復を14要求×9batch実行した。初回1batch、warm8batch。各位置の独立した空decoderを正解にし、全native planes、正確なPTS/end、dimensions、format/color labelsを要求の計測時間外に比較する。変更前と最終候補・索引なし対照のoracleも全件一致した。各modeの実際の504要求で全画素比較が成功した。runtime/open、fixture生成、source hash、oracle生成はrequest時間に含めない。batch wallは比較と記録を含む。

| 素材 | 変更前p50/p95 ms | 最終候補p50/p95 ms | 同じ候補binaryの索引なしp50/p95 ms |
|---|---:|---:|---:|
| CFR | 37.278 / 51.498 | 0.697 / 1.795 | 30.352 / 43.350 |
| VFR | 19.584 / 33.659 | 1.107 / 2.059 | 16.081 / 22.746 |
| B-frame | 29.705 / 47.870 | 0.609 / 1.483 | 24.865 / 38.426 |
| 負origin | 32.835 / 56.962 | 0.863 / 1.076 | 36.551 / 47.225 |

最終候補の検証済み再訪問は各素材99回、fallbackは0回。負originだけ10件の証明で1秒前の候補を一度探索し、成功したanchorを記録した。全試行のnative復号数はCFR/B-frame85233→10269、VFR42681→6441、負origin85425→10609。未証明の初訪問はoriginから復号する。最初の54秒要求は同じbinaryの対照→索引でCFR44.179→40.984ms、VFR21.267→25.012ms、B-frame31.788→33.709ms、負origin45.864→46.193ms。各1回でcoldの改善や退行を確定しない。cold batchにも以前の点の再訪問があり、純粋な初訪問だけの指標にはしない。

各decoderの索引は10件・論理1040bytes。native current/lookaheadピーク172800bytesは同じ。whole-process最大RSSは変更前40,435,712、最終候補38,256,640、同じbinary対照38,420,480bytes。peak footprintは27,705,920、25,477,696、25,641,536bytes。native allocatorやホスト条件を含む観測値で、メモリ削減の因果やRSS上限を主張しない。

### 4K連続再生の退行監査

同じPERF-003の4K HEVC 3840×2160/30fps/2秒を、fresh decoderで60連続提示×5batch（cold1/warm4）測る。全native streamの画素・PTS/end・dimensions/labelsをhashし、300提示×modeと以前の試作・対照で完全一致を確認した。open/runtime/hash照合はrequest時間外。

全要求を証明する試作はp50/p95が0.924/1.528→23.068/24.210msとなり、採用しない。要求中の実復号数128以上だけを証明する最終候補は、同じbinaryの索引なし0.912/1.472→0.932/1.711ms。索引entryは0件、復号60frames、presentation peak24,883,200bytesで一致した。小さいp95増加を測定誤差と断定しない。whole-process RSSは対照620,249,088→候補610,402,304、footprint608,191,544→598,344,760bytes。独立processの観測で、メモリ削減を主張しない。

正確性を優先し、1秒前候補がない初期試作では負originの検証不一致10件を全件canonical replayへ戻した。その試作の速度は32.835/56.962→33.112/48.316msで、改善したと扱わない。失敗した候補の画素をfallback時に再利用しない回帰を追加した。

### 再現

```sh
cargo build --release --locked -p kronello-media --example perf_video_seek --example perf_video_forward
# beforeは新規fixture directoryを生成する。各outputは未作成の新規directoryを指定する。
target/release/examples/perf_video_seek OUTPUT_DIRECTORY FIXTURE_DIRECTORY before
target/release/examples/perf_video_seek NEW_OUTPUT_DIRECTORY FIXTURE_DIRECTORY after
target/release/examples/perf_video_seek CONTROL_OUTPUT_DIRECTORY FIXTURE_DIRECTORY origin
target/release/examples/perf_video_forward NEW_OUTPUT_JSON INPUT_4K_30FPS_60FRAME_MOV indexed
target/release/examples/perf_video_forward CONTROL_OUTPUT_JSON INPUT_4K_30FPS_60FRAME_MOV origin
cargo test -p kronello-media --lib video_seek --locked
```

変更前binaryはtarget/perf-improvements/perf005-seek-baseline、候補の実行binaryと正確なcompile-time sourceはperf005-seek-candidate-v3へ保存した。候補の計測後に追加したcheap-work/Unix限定テストはproductionの動作を変えない。最終sourceの一括検証は別receiptへ記録する。generated fixturesを使うテストは既存のfixture生成手順を先に実行する。

## seek部分の回帰

実Metal環境の `cargo test -p kronello-media --locked` とmedia/service all-targets Clippyがexit 0で成功した。source変更・digest不一致・reverse端点・FIFO eviction・cheap forward非捕捉の4unit testsを含む。通常テスト92件成功、9件ignored。詳細は[receipt](perf-005-seek-validation.json)。追加unit testのPath参照を修正する前のcompile失敗は成功数へ数えない。これはDSP checkpoint実装前にcompileされたseek部分の回帰で、後続変更や全workspaceの最終検証を代替しない。

## 正確な音声チェックポイント

[ADR-0155](../adr/0155-exact-bounded-audio-checkpoints.md)の状態共有を同じ300秒PCM/180秒配置で測った。[変更前](perf-005-audio-baseline.json)と[前後/同じbinary対照のraw](perf-005-audio-checkpoints.json)に全trial、sampled output/meter、source/binary hash、状態量、RSSを保存した。独立processでenabledの後にdisabledを実行し、quiet_host=false。copy状態以外のcompiled plan、allocator、active session、PCM/nativeは別の資源として扱う。

| 配置 | 変更前p50/p95 ms | 同じ候補binaryの保持なしp50/p95 ms | checkpointありp50/p95 ms |
|---|---:|---:|---:|
| 二つのstaggered DSP | 4180.631 / 4525.483 | 4535.049 / 4840.617 | 45.288 / 305.331 |
| 3/2 WSOLA + DSP | 5341.034 / 7201.316 | 5403.447 / 5894.440 | 71.260 / 675.624 |

capture+prepare+最初の4096stereo framesまでの時間。cold1/warm8。次の2blockとmeterは時間外で検証した。18要求×3blockの全f32 bits SHAと全meterが変更前/enabled/disabledで一致し、同じdocument/source/revisionを確認した。長尺全出力の比較や音響loopbackではない。core回帰では全DSP（filter/compressor/limiter/delay/reverb/gate/gain）とWSOLAについて、12173sampleの非hop境界から独立二cursorを復元し、jump/rewind/continuous/statelessの全sample/meterを比較した。

coldの137秒prepareはDSP4239.579→4385.943ms、WSOLA5141.021→5335.414ms。既に訪問した範囲のcheckpointがなければ元のprerollが残り、初回の速さを主張しない。最初の151/165秒要求は既存の136/150秒から追加15秒を準備し、p95を押し上げた。以降は137/151/165秒で残48017/48031/48053samples、90秒で7samplesだけをwarmする。<33.3msのseek開始や無条件な瞬時準備を保証しない。

両配置ともcompiled plan1件、compile1/hit8、checkpoint restore8/miss1、82件のcheckpoint。保持状態はDSP25,033,968bytes、WSOLA24,429,440bytes、既定32MiB内で、default probeのevictionは0。PCMは各caseでdecode1/hit8・115,200,000bytesと変更前と同じ。byte/entry eviction・tiny budget・weak bindingは小上限の別回帰で確認した。whole-process RSSは変更前263,503,872、同じbinary保持なし258,392,064、あり287,801,344bytes。footprintは243,991,272、241,812,200、271,123,200bytes。状態保持のメモリ増加を受け入れ、削減と呼ばない。

service playback10件とcore exact checkpoint1件、audio/service all-targets Clippyが同じcheckpoint実装でexit0。audio全87件はallocationなしpreflight分離の前に成功し、その分離後はcore exact/共有serviceを再実行した。compile失敗したcore testの重複mutable borrow配列はselector loopへ修正し、成功に数えない。capture時のcanonical identityにより、producer前のproject close/deleteでも固定snapshotを維持する。Unix同内容ctime変更、新PCM binding、hash mismatch/restore/missing、revision/generation、独立DSP、コピー前予算、weak/no-PCM-cache、native owner/producer threadを回帰確認した。実deviceの受け入れは最終縦断で確認する。

```sh
cargo build --release --locked -p kronello-service --example perf_audio_seek
# beforeは新規fixture directoryに同じ入力WAVをcopyしてprojectを作る。
target/release/examples/perf_audio_seek OUTPUT_DIRECTORY FIXTURE_DIRECTORY INPUT_300S_STEREO_48K_WAV before
target/release/examples/perf_audio_seek NEW_OUTPUT_DIRECTORY FIXTURE_DIRECTORY INPUT_300S_STEREO_48K_WAV after
target/release/examples/perf_audio_seek CONTROL_OUTPUT_DIRECTORY FIXTURE_DIRECTORY INPUT_300S_STEREO_48K_WAV disabled
cargo test -p kronello-audio --test fx008 exact_checkpoints --locked
cargo test -p kronello-service --test playback --locked
```

compile時のsource/binaryはtarget/perf-improvements/perf005-audio-baselineとperf005-checkpoint-candidate-v2に保存した。後続のコメント修正とVT/GPU変更をこのbinary検証に混ぜず、最後に全sourceを固定して一括検証する。

## 有界demuxと永続VideoToolbox

[ADR-0156](../adr/0156-bounded-persistent-hardware-cursor.md)のSDR cursorを、保存した変更前release binary、候補、同じ候補binaryの従来経路で比較した。[raw/資源/検証receipt](perf-005-resident-measurements.json)に全480要求×mode、native planes/PTS/size SHA、source/binary hash、fixture、失敗した試行を保存した。quiet_host=falseで、compile/testが一部の測定と重なった。無作為交互比較やFPS保証にはしない。cold1/warm4batch、各batch12要求。warm分位にも最初のopen/全metadata走査とbackward origin restartを含む。順方向だけの分位と呼ばない。

| 素材/出力 | 変更前p50/p95 ms | 同じbinaryの従来経路p50/p95 ms | 永続cursor p50/p95 ms |
|---|---:|---:|---:|
| cfr / Bgra8 | 494.021 / 510.220 | 494.467 / 520.890 | 0.501 / 275.814 |
| cfr / Nv12VideoRange | 314.442 / 321.896 | 336.928 / 628.846 | 0.282 / 177.880 |
| vfr / Bgra8 | 244.830 / 295.933 | 249.556 / 262.195 | 0.367 / 136.431 |
| vfr / Nv12VideoRange | 165.665 / 203.069 | 161.279 / 170.418 | 0.318 / 94.948 |
| bframes / Bgra8 | 501.023 / 515.478 | 497.756 / 706.769 | 0.954 / 259.458 |
| bframes / Nv12VideoRange | 324.015 / 355.010 | 382.753 / 966.366 | 0.791 / 185.654 |
| 4k-hvc1 / Bgra8 | 306.183 / 316.476 | 306.620 / 316.153 | 5.661 / 39.228 |
| 4k-hvc1 / Nv12VideoRange | 117.866 / 138.881 | 116.388 / 119.921 | 2.607 / 26.326 |

60秒320×180のCFR/VFR/B-frame H264と2秒3840×2160 HEVCを使う。タグが未指定の初回fixtureと`hev1`の4K sample entryは既存契約で拒否されたので、圧縮packetをcopyしたままBT709 colrと対応する`hvc1`に揃え、失敗runを比較へ含めない。native planes、正確なPTS/size/queryは候補対変更前480件、候補対同じbinary対照480件で完全一致した。native RGB検証oracleのclamp欠落により最初の実GPUテスト2件が失敗し、既存SDR shaderに合わせたoracle修正後は5件すべて成功した。production shaderとnative planeは変更していない。

初回30秒位置と逆方向では正確なorigin replayが残る。各batchでsessionは12回ではなく2回、origin restartは1回。未来outputピークはCFR/VFR/4Kが2件、B-frameが4件。320×180 BGRA8のピーク942,336bytes、NV12が371,776bytes、4K BGRA8が66,355,328bytes、NV12が24,900,608bytes。metadataはCFR/B-frame1440、VFR720、4K60samplesを一度走査する。一つの圧縮sampleピークは201,067bytes。queue/native外部所有は独立の予算であり、metadata、asset hash、GPU処理全体の速度をこれだけで保証しない。

最初のcold request（cfr.mov / Nv12VideoRange）は322.683→180.344ms。各1回の観測でcoldの因果を確定しない。

最初のcold request（4k-hvc1.mov / Nv12VideoRange）は118.642→17.124ms。各1回の観測でcoldの因果を確定しない。

whole-process最大RSSは変更前583,778,304、同じbinary従来経路583,614,464、永続cursor649,691,136bytes。queue保持とは別のnative codec、allocatorと検証配列を含む独立processの観測で、メモリ削減を主張しない。

### 36分の上限超過素材

同じ60秒CFRを36回packet-copy concatした163,686,036bytes/2160秒/51840samplesの素材を、一つの診断batchで測った。従来の一発demuxは`compressed sample budget exceeded`を明示して失敗した。新cursorは圧縮sample10,136bytesとoutput2件/185,888bytesを保持し、全7要求のnative planesとPTSを繰り返し元の短い素材に完全照合した。最初の0秒open/全metadata走査は801.981ms、初めて35分へ移動すると11017.577ms、続く3framesは0.354/0.303/0.342ms、30秒へ戻ると164.481ms、続く1frameは0.511ms。hardware random seekの残コストを改善済みと呼ばない。このprocessは先に従来の失敗も実行し、最大RSS199,032,832bytes/footprint187,794,128bytes。streamingだけのメモリ比較ではない。

追加の製品backend回帰で二つのauthored clockが各1sessionを順方向再利用し、10cursor要求で8件に退避すること、同内容書き換え時の新入場、warm後の期待hash/missing拒否を確認した。6件の実機回帰がexit0で成功した。

実機でH264/HEVC、BGRA8/NV12、CFR/VFR/B-frame、負のlocked origin、同区間の再要求、forward/backward、半開end、同内容ctime変更、破棄後の返却frame ownership、既存のHDR/10bit/full-range拒否とCPU pixel upload=0を検証した。framebridge/media/service all-targets Clippyもexit0。最初のClippy item-order/定数assertと長尺probeのpacked CMTime field参照のcompile失敗は修正し、成功に含めない。計測後にはtest moduleの前へ宣言を移し、製品backendのstale source退避前検査と保持statsを追加した。coreの計測をその製品backend時間へ読み替えず、最終source fenceの一括検証を別に行う。

```sh
cargo build --release --locked -p kronello-media --example perf_resident_decode --example perf_resident_long
# 計測receiptのmanifestを使い、SDR tagsとhvc1 sample entryを持つ同じfixtureを渡す。
target/release/examples/perf_resident_decode NEW_OUTPUT_JSON SEEK_FIXTURE_DIRECTORY INPUT_4K_HVC1_MOV streaming
target/release/examples/perf_resident_decode CONTROL_OUTPUT_JSON SEEK_FIXTURE_DIRECTORY INPUT_4K_HVC1_MOV legacy
target/release/examples/perf_resident_long NEW_OUTPUT_JSON INPUT_36MIN_MOV ORIGINAL_60S_MOV
cargo test -p kronello-service --test gpu_resident --locked -- --ignored
```

## GPU局所面と共有寿命

[ADR-0157](../adr/0157-local-coverage-and-last-consumer-surfaces.md)の局所coverageと最終consumerでのmemo解放を測った。[raw/fixture/source/binary/検証](perf-005-gpu-measurements.json)に不採用試作も残す。最終方式はEffect/EffectMap直接入力を全面に保ち、trusted fill-only leafだけをguard付き整数pixel面へ縮小する。32以上の2の冪bucketでpool再利用を保ち、追加の全面復元copyは行わない。事前面予算は従来の安全側上界を維持する。

同一device/contextでFullCanvasと最適化を毎iteration逆順に交互実行した。cold1組/warm21組、画素が変わるfractional position、semantic texture cache無効、既定有界pool。全linear/display readbackとsticky statusをtimerに含め、hash照合は時間外。最終paired/projectのGPU実行中に他のGPUテストを重ねていない。quiet_host=falseで、一般のFPSやquiet-host基準の受け入れに転用しない。

| シーン | FullCanvas p50/p95 ms | 最適化p50/p95 ms | 保持GPU資源ピーク bytes: full→最適化 |
|---|---:|---:|---:|
| 480×270、200図形/10effect/10group | 40.842 / 43.258 | 38.428 / 42.208 | 5,409,092 → 5,196,596 |
| 960×540、200図形/10effect/10group | 59.486 / 63.595 | 53.027 / 57.364 | 20,961,092 → 20,748,596 |
| 960×540、32共有図形/32mask、64roots | 29.287 / 30.566 | 23.875 / 28.211 | 149,363,668 → 16,891,744 |
| 日本語project、960×540、234 DAG nodes | 73.390 / 82.925 | 68.847 / 74.310 | 52,168,052 → 39,316,052 |

最初の3ケースは66組のf32 bits SHAが完全一致し、保存した変更前binaryの最初の9points×3ケースとも27組一致した。shader/status/readbackの追加画像copyは各trial 0件。最後は保存したinspectable projectの図形48、日本語text16を実model/font/layoutで構築し、導出SceneIrへ8 Gaussian effectsと動くfractional transformを明示処方してDAG実行した22組。全f32 bits一致、source project/font SHA固定。maskは共有DrawSceneで検証した。layout/compiler時間は別記し、GPU時間へ混ぜない。font/runtime準備を含むcold layout911.818msの後、cold first full358.739ms/同device最適化63.576msだった。実行順の影響がありcold改善の因果を主張しない。

GPU保持面そのもののピークは共有case149,299,200→16,850,944bytes。これとdescriptor control/readback等を含むowner peakを、idle pool・allocator・driver・process RSSと区別する。paired3case最大RSS782,090,240/footprint1,031,718,424bytes、日本語project492,912,640/683,459,856bytesは両modeを一processで動かした値で、mode間のRSS削減ではない。固定before3caseは別process486,047,744bytesで、直接の因果比較にしない。

部分面を全面へ2copyで復元した初期試作は全画素が一致したが、既存transfer assertionが失敗した。assertionは緩めずcopyを撤去した。同じdeviceのv2交互試作では960×540 effect p50が67.238→169.774msへ悪化し、不採用。最終は直接effect入力の全面保持とsize bucketでこの回帰を避けた。

既存scene39件が成功し、4K/anisotropic scale/Evenodd/implicit close/negative coordinates、fractional clipping、stroke/gradient fallback、dirty pool、sticky失敗/cache非公開、warm cache一致と共有面の最終consumer後解放を対象の実機5件で確認した。共有root/maskの全画素とeffect halo、実peak半減以上、memo空/lease解放、追加画像copy0を回帰assertする。広すぎるdebug filterが旧release専用計測を含めた失敗は成功扱いせず、対象filterでexit0を得た。複雑project probeのAffine2 field typoも測定前に修正した。

```sh
cargo build --release --locked -p kronello-gpu --example perf_scene_surfaces --example perf_complex_project
target/release/examples/perf_scene_surfaces NEW_OUTPUT_JSON paired
# raw receiptのproject_fixtureをJSON保存して同じfont fixtureを解決する。
target/release/examples/perf_complex_project PROJECT_JSON NEW_OUTPUT_JSON
cargo test -p kronello-gpu --lib coverage_bounds_tests::gpu_ --locked -- --ignored
cargo test -p kronello-gpu --test scene --locked
```

## 最終一括検証

[最終receipt](perf-005-final-validation.json)に589 sourceのSHAを固定し、終了時にも全件一致した。fmt、workspace all-targets Clippy、workspace testがexit0。通常1325件とdoctest2件が成功、200 sections、47件ignoredは成功数へ含めない。最初のcomplex probeのconstant debug assertionはClippyが拒否し、同じif/panicへ修正して最終all-targets Clippyでcompileした。測定releaseの動作は変えず、production sourceには変更がない。失敗した先行runを最終成功へ混ぜない。

最終production sourceで製品hardware backendの実機6件もexit0。局所coverage/共有解放の実機5件と既存scene39件は各receiptの実行scopeを参照する。選択Swift回帰36件は最終release FFI/CLIを原子的配置して別job state・並列3workersで成功した。Rust/SwiftともreleaseでKronelloAudioHarnessを構築した。FFI dylibのinstall nameは`@rpath/libkronello_ffi.dylib`、計測に使った全binary hashを保存する。

[実音声デバイスとMetalの全trace](perf-005-native-audio.json)は、PERF-004と同じ300秒PCM/180秒の動くcompositionを使い、各40秒、frame137/777 seek、stop/resumeを行った。初回3rateはworkspaceのrenderer/model/service（GPU effectを含む）と一部重なった。物理scanoutと音響loopbackは測定していない。

| rate / 条件 | 観測device秒 | Metal提示回数 | 最大frame格子ずれ | 最大A/V offset ms | underrun / missing audio frames |
|---|---:|---:|---:|---:|---:|
| 24/1 / workspace負荷下 | 38.626 | 930 | 0 | 35.021 | 0 / 0 |
| 24000/1001 / workspace負荷下 | 38.568 | 926 | 1 | 75.271 | 0 / 0 |
| 30000/1001 / workspace負荷下 | 38.650 | 1161 | 2 | 74.942 | 0 / 0 |
| 24000/1001 / workspace終了後 | 38.510 | 926 | 0 | 30.146 | 0 / 0 |
| 30000/1001 / workspace終了後 | 38.603 | 1161 | 0 | 33.362 | 0 / 0 |

全5回で正確な有理数frame/sample計算、2seekとstop/resume sample位置が一致した。初回の1〜2frame遅延を捨てず、検証終了後に同じbinary/inputで2rateを再観測した。後者は自分のbuild/GPUテストを重ねていないが他host負荷は管理せずquiet_host=falseのまま。単発の条件差から因果的な遅延改善や常時の同期/FPSを保証しない。owner peakと各process RSS/footprint、presentation間隔をrawで別々に記録した。PERF-004のgateを本変更へ流用しない。

全条件を確認した後にPERF-005をdoneへ更新し、backlog render/checkを行った。参照計画4段階の集約は[実測まとめ](performance-improvements.md)。
