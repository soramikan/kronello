# PERF-004: 書き出し経路の実測と改善

2026-10-11。PERF-004は受け入れ済み（`done`）。音声単独の映像処理省略、codec別並列度とpacket span、linear専用GPU出力を実装・実測した。playback audio source共有と二組の有界pipelineも実装・実測した。期待hash検証修正を含む最終版のfmt・workspace clippy・workspace test・Swift回帰も成功した。第3段階の受け入れであり、長尺seek・DSP checkpoint・永続VT・GPU範囲/寿命の第4段階はPERF-005へ残る。

## 音声単独出力

[ADR-0148](../adr/0148-audio-only-delivery-without-video.md) に従い、全legが音声なら映像backendとtile描画を省く。映像metadataは空になり、最終checkpointとjob進捗は要求のframe格子を維持する。同期と固定workerの両経路を変更した。

測定はreleaseの `perf_audio_only_export`、同一の48kHz stereo PCM24非無音原音、24fpsの要求 `[1/4,3/4)`、24000出力samples。clipは `[-1/8,7/8)`、source_in=1/8、gain=0.6。source decode・spool・mix・encode・probe・publicationと、変更前のCPU tile描画を計時する。decoded hashは計時外。各caseを初回cold + warm8回で測り、p50は昇順index4、p95はindex7。順序は全case before→afterであり、静穏hostではない探索測定。GPU性能、長尺、RSS、実再生安定性をこの値から推定しない。

| 出力 / 設定pixels | before warm p50/p95 ms | after warm p50/p95 ms | backend呼出/出力 |
|---|---:|---:|---:|
| FLAC / 64×36 | 15.238 / 16.755 | 10.761 / 10.864 | 12→0 |
| FLAC / 640×360 | 383.665 / 388.599 | 9.976 / 10.086 | 24→0 |
| FLAC / 1920×1080 | 3353.517 / 3525.415 | 9.921 / 10.902 | 144→0 |
| MP3 / 64×36 | 17.677 / 18.093 | 12.987 / 13.433 | 12→0 |
| MP3 / 640×360 | 400.266 / 408.416 | 13.964 / 19.255 | 24→0 |
| MP3 / 1920×1080 | 3502.704 / 3540.164 | 13.210 / 15.693 | 144→0 |
| FLAC + ProRes / 640×360 | 490.949 / 516.915 | 484.748 / 490.723 | 24→24 |

呼出数はframe数ではなくtile単位。論理frameは各12。全7case×9回=63比較でdecoded f32 SHA256、encoded audio file SHA256、sample数・range・audio report・probe・streaming bytesが一致し、最終checkpointは12。混合出力の映像metadataは12件を維持した。個別cold値・全trial・runtime/source/binary hash・fixture生成手順は [測定JSON](perf-004-audio-measurements.json) に保存した。

```sh
cargo build --release --locked -p kronello-media --example perf_audio_only_export
target/release/examples/perf_audio_only_export \
  target/perf-improvements/export-tone.wav target/perf-improvements/audio-export-new
```

初回probeはnative MP3読み戻しの `discontinuous audio timestamps` で失敗し、比較値として採用していない。V2では前後ともMP3のdecoded hashを独立したffmpeg CLIで計時外に計算した。調査でdecoder contextの `pkt_timebase` が未設定と判明し、packet time baseを設定する修正を別に行った。skip-sampleのtimestamp補正をFFmpegに渡し、既存のPTS continuity検証は維持する。native decoderへの無音fallbackは追加しない。

別試行の `/usr/bin/time -l` 最大RSSはbefore 62,881,792 bytes、after 63,979,520 bytesで減少していない。全7caseの混合映像出力と子ffmpeg decoderを含むprobeの観測値で、音声単独の保持payloadだけの値ではない。workspaceテストも並行していた。この修正は不要な計算の除去であり、RSS改善を主張しない。

## 検証状態

音声単独MP3/FLACのNoVideo backend、sample数、先頭PTS、最終checkpoint、および同期GPU factory失敗・混合出力の型付き失敗、固定workerの進捗・空metadata・復旧・改変拒否は通常テストで成功した。HDR音声/cancelとstage回収、nonfinite拒否、非無音MP3の24000samples・先頭PTS・lossy最大誤差0.02未満も成功した。

media/audio/audio5/audio10/deliveryの回帰（audio10の実GPU2件はignored）、media/streamingの回帰（streamingの長尺/実GPU4件とmediaの実GPU1件はignored）、映像を含むCLI fan-out、`cargo fmt --all --check` とworkspace全targetのclippyが成功した。初回追加テストの型/fixture/エラーcodeの修正と、初回clippyの定数assert修正後の結果である。

変更前の保存済みrelease CLIで固定FLAC jobを実行し、旧3件の映像metadata付きreceiptを現在のCLIで再開してSucceededへ戻ることを確認した。[旧worker復旧記録](perf-004-audio-legacy-recovery.json)。現在のdebug CLIでもbackend=gpuを固定し、test-only adapter unavailable注入をworkerへ継承した状態で、空metadataのFLAC出力と進捗3/3が成功した。[GPU不要worker記録](perf-004-audio-gpu-worker.json)。注入はreleaseでは無効な既存のテスト機能を用いる。

workspace一括検証の制限環境での初回は、CLI実GPUテストの `ADAPTER_UNAVAILABLE` で失敗した。Metal環境での2回目はCLI実GPUテストを通過したが、通知テストが共通job stateの外部進捗を異なる時点で比較して失敗した。通知テストを専用stateの子プロセスへ隔離し、環境変数を並行テスト内で変更せず、busy worker中のrevision/job通知検証を維持した。

修正後に `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、専用state・Metal環境での `cargo test --workspace --locked` を成功させた。通常テスト1305件・doctest2件が成功し、44件はignoredのままで未実行と区別する。実行開始時に固定したRust/C/header/Cargo等398ファイルのhashは終了時と一致した。[検証receipt](perf-004-audio-validation.json)。PERF-004全体の残件は冒頭のとおりである。

## Codec並列度の予備比較

製品設定を変える前にnative `media.c` の一時コピーでProRes/DNxのthread_countとSVTのlpだけを1/2/4/8へ変更し、同じRuntime・640×360・12frame・24fpsの変化するRGBA patternをencodeした。bitrate=2Mと既存requested presetを維持し、生成/Runtime load/独立oracleは計時外、encoder open/frame/flush/closeだけを計時した。SVTのrequested presetは12、Runtimeのnative logではeffective preset=11で、全parallelismで同じ補正が行われた。cold1 + warm8であり、静穏hostではない。これは製品のexport全体・GPU時間の値ではない。[予備測定JSON](perf-004-codec-prototype.json)。

| Codec | 1 thread p50/p95 ms | 2 threads | 4 threads | 8 threads |
|---|---:|---:|---:|---:|
| ProRes KS | 120.648 / 139.793 | 46.031 / 53.940 | 35.942 / 42.210 | 35.716 / 38.741 |
| DNxHR HQX | 56.367 / 61.812 | 26.834 / 27.788 | 29.786 / 32.350 | 69.132 / 84.566 |
| SVT-AV1 | 47.407 / 56.133 | 26.018 / 27.611 | 30.962 / 38.629 | 98.047 / 139.935 |

ProRes/DNxの全parallelismで各9出力のfile hashが同じで、独立ffmpeg decoderのraw画素が1threadとbit一致した。packetのPTS/durationをstream time baseで有理数へ変換し、各 `[i/24,(i+1)/24)` と全12frame・duration1/2秒を確認した。raw ticks=512を仮定した初回oracleはNative MOVのtime_base=1/24で失敗したため、固定tick値を仮定しない比較へ修正した。

SVT-AV1の2/4/8では9出力間のfile hashも一定にならず、初回decodeも1threadとbit一致しなかった（raw 8bit sample最大差52）。現在のlp=1を維持する。libaom-av1はこのRuntimeではencoder unavailableで、代替codecの結果として数えていない。ProRes4/DNx2を候補に、製品経路でpacket flush・memory・品質と所要時間を検証する。ProResの4/8差は小さく、この1試行から8threadを既定にしない。

## Codec設定とpacket spanの実装

[ADR-0149](../adr/0149-codec-parallelism-and-packet-spans.md) に従い、ProRes4/DNx2を初期設定にした。SVT・hardware/GIF等は従来のまま。以下に製品経路の性能・memoryを記録する。PERF-004全体の受け入れは未完了である。

packet durationの最後のsubmitからの流用を、最大256件のPTS/span metadataによる照合へ修正した。実験用wrapperでmux直前を観察すると、非等間隔tick0/1/3/6/10、span1/2/3/4/4に対し、旧shimのProRes4/SVT1は最初の3packetへ誤ったspan4を渡した。修正後は両codecの全5packetが入力spanと一致した。[packet照合記録](perf-004-codec-spans.json)。wrapperのtraceは製品には導入していない。

通常Rustテストで非等間隔ProRes/AV1の各 `[pts,end)`、区間中点とstream durationを確認した。初回テストはcontainerのmicrosecond丸め値と厳密な7/12秒を比較して失敗したため、有理数のstream durationを比較するよう修正して成功した。codec回帰ではdelivery全8件、audio11件、audio10の5件、audio5の3件（2 ignored）が成功した。HDRの2件・streamingの3件（4 ignored）は実Metalで成功した。codec変更後のworkspace clippyも成功したが、冒頭の音声単独substepのworkspace成功を新しいcodec設定の全体成功として扱わない。


## 製品delivery経路のcodec比較

[測定JSON](perf-004-codec-delivery.json)。同じRust `perf_codec_delivery` とPTS/span修正を両側に固定し、workspaceのコピー側だけProRes/DNxを1threadへ戻してrelease binaryを作った。製品sourceを一時的に書き換えていない。synthetic linear tile patternを注入する製品delivery APIで、12frame・24fps・1/2秒・silent stereoをencodeした。timed scopeはpattern生成/音声encode/映像encode/mux/検査/publication。Scene描画・GPU・素材decode・Runtime load・hash/oracleは含まない。

| Codec / pixels | before warm p50/p95 ms | after warm p50/p95 ms |
|---|---:|---:|
| ProRes 640×360 | 171.209 / 210.871 | 159.039 / 162.859 |
| DNxHR HQX 640×360 | 106.059 / 190.436 | 99.783 / 103.367 |
| SVT-AV1 640×360（設定維持） | 105.925 / 156.794 | 98.848 / 101.651 |
| ProRes 1920×1080 | 1703.341 / 1864.541 | 1629.316 / 1670.621 |
| DNxHR HQX 1920×1080 | 833.101 / 907.719 | 813.148 / 913.344 |
| SVT-AV1 1920×1080（設定維持） | 930.037 / 939.291 | 926.222 / 928.230 |

各cold1 + warm8、全before→全after、静穏hostではない。設定不変のSVTにも時間差があり、これだけでcodec並列化の因果的倍率を保証しない。native encodeだけの予備測定より製品全体の差は小さく、tile変換等を含む時間を区別する。全54組でencoded file bytes、独立ffmpeg decodeのnative画素bit、全12frameの有理数PTS/span、sample range、progress/checkpoint、backend call数が一致した。初回はProRes legacy envelopeへ新movie constructorを使ってtyped拒否されたため、正規のlegacy constructorへ修正し、成功したv2のみ採用した。

全codec連続probeのmaximum RSSは371.16→371.51 MB、peak footprintは278.27→280.94 MB（decimal）。個別1080p probeはProRes RSS71.06→80.69 MB、DNx134.22→142.82 MB。thread内部bufferが増えるため、shimの画像一枚/packet所有からRSS低下を主張しない。個別probeも各9encodeのプロセス全体であり、encoderだけのallocation測定ではない。

## Linear専用出力

[ADR-0150](../adr/0150-linear-only-delivery-and-temporal-samples.md)に従いMovie/GIFはlinear tileだけを消費する。CPU、GPU、software/sequential/resident media adapterでplane選択を伝播する。通常frame/PNGの両面出力と最終display変換、sticky shader validation、halo/crop、disk policy、locked media入力を維持する。temporal中間sampleもlinearだけを取得し、float64 accumulationと有理数sampleを保つ。linear streamingは両面temporal cacheを参照/更新しない。

CPUのSDR/HDR/temporal全画素と両面cache cold/warmを確認し、unused displayを呼ぶとpanicするbackendでもlinear exportが成功した。pixel数不整合/NaN/sink失敗は拒否した。実Metalのhalo付きDAGで両面とlinear画素がbit一致し、row-padded画像readbackとstatusを合わせて3→2操作・wait3→2になった。GPU cache/同時観測/temporal hitの7件、renderの実GPU8件、INSPECT-002の4件も成功した。検査Queryは通常frame二面・temporal sample一面を実測計数へ整合した。最初のbyte期待値はrow paddingを含めていなかったため失敗し、実256-byte pitchへ修正した。制限環境でのrenderの8失敗はAdapterUnavailableで、実Metalで8件すべて再確認した。


### 同一binaryのGPU境界比較

[測定JSON](perf-004-linear-measurements.json)。`perf_linear_delivery` の一binaryで、旧GPU両面境界を強制する互換backendとlinear専用backendをiterationごとに交互実行した。frame比較は各caseに独立GPU context二個を作り、Runtime/GPU作成を計時外、shader compileを初回へ含める。同じ形状・有理数時刻・temporal planでcold1 + warm8。tile sinkのf32 bit hash時間を差し引いたrender時間であり、wall/sink時間もrawに残す。静穏hostではなく、同時の検証workloadとcontext切替の影響を含む。

| pixels / temporal | paired warm p50/p95 ms | linear warm p50/p95 ms | image+status operations |
|---|---:|---:|---:|
| 640×360 / off | 11.672 / 25.246 | 5.517 / 5.916 | 6→4 |
| 640×360 / 3 samples | 56.656 / 234.242 | 26.921 / 35.388 | 18→12 |
| 1920×1080 / off | 81.180 / 85.560 | 204.108 / 245.472 | 36→24 |
| 1920×1080 / 3 samples | 463.361 / 494.284 | 234.271 / 388.900 | 108→72 |
| 3840×2160 / off | 486.478 / 529.209 | 342.406 / 362.147 | 120→80 |
| 3840×2160 / 3 samples | 1417.582 / 1540.162 | 796.039 / 907.924 | 360→240 |

54組すべてでlinear f32 bit hashが一致した。画像転送byteは全条件で一面分を削減し、status 4 bytes/tile/sampleは維持した。4K・3 samplesでは398,131,680→199,066,080 bytes/要求、360→240 readback/waitだった。GPU copyは0、単一graphの意味keyを維持する。

1080p/offはlinearの奇数iterationが52〜73ms、偶数が204〜245msで、単一のp50から改善を採否しない。[同一GPU contextの診断](perf-004-linear-shared-diagnostic.json)ではpaired p50/p95 202.874/282.805ms、linear39.241/53.455msとなった。context共有ではpairedの直後にlinearの初回を測るためcold同士は比較できない。転送とbit一致は同じだが、遅延の原因をdriver/context/OS scheduling等へ確定していない。静穏windowと製品の一context条件で追加確認する範囲として残す。

MOV比較は640×360、12frame・24fps・[1/2,1)を出力し、ProRes4・silent PCM24・同じsnapshotとcolor変換を固定した。旧GPU境界の強制側も新linear tile入口を通るので、temporalの最終CPU display変換は既に省かれている。従来export binary全体との比較とは区別する。

| temporal | paired warm p50/p95 ms | linear warm p50/p95 ms | operations / movie |
|---|---:|---:|---:|
| off | 351.153 / 507.015 | 304.973 / 572.900 | 72→48 |
| 3 samples | 659.110 / 721.040 | 500.291 / 587.302 | 216→144 |

全18組のencoded MOV bytesが一致し、計時外の独立ffmpegでvideo/audio decoded bits、video全12 `[i/24,(i+1)/24)`、24000 stereo audio samplesを確認した。通常movieのp95は悪化した試行があるため、転送削減を安定したlatency改善保証と読み替えない。probe全体のmaximum RSSは98.47MB、peak footprintは394.40MB（decimal）で、両context/全caseを含む一プロセスの値。modeごとのmemory改善や物理GPU消費の差とはしない。


### Codec/linear substepのworkspace確認

sourceを固定してfmt、workspace clippy、専用state・実Metalのworkspace testを成功させた。通常1308件、doctest2件が成功し、45件はworkspaceではignoredと区別する。Rust/C/header/Cargo等402ファイルのhashは開始/終了で一致した。[検証receipt](perf-004-linear-validation.json)。別途実行したGPU cache/linearの7件とINSPECTの4件を記録した。この版のsubstep検証であり、PERF-004の有界pipeline・playback source共有を完了と扱わない。


## Playback音声source共有

[ADR-0151](../adr/0151-bounded-playback-pcm-sharing.md)に従い、session/Service所有の32entry・128 MiB PCM cacheを追加した。immutable sourceだけをArc共有し、snapshot/planとDSP cursorは再生ごとに独立する。Unix warmのlocator/stat二重照合、cold前後full hash、非Unix fresh decode、全source残frame budgetとtyped失敗を維持する。native captureはediting threadでstore snapshotを固定し、cache ownerをprepare threadへ渡す。

[測定JSON](perf-004-audio-preparation.json)。同一release `perf_audio_preparation` binaryでcache容量0と既定容量を別プロセスで順番に実行した。48k stereo PCM16の決定的sine、cold1+warm8、iteration4の映像だけのrevision更新を含む。source生成/probe/hash、project create/importは計時外。capture+compile/decode+最初の4096-frame blockまでを計時する。各trialで4block（sample0/4096/48000/最終素材の最後の1秒先頭）のf32 bitsを比較し、全36組で一致した。全素材を丸ごとsample比較した結果ではない。静穏hostではなく、device callbackやDSP checkpointの改善をこの測定から推定しない。

| source構成 | uncached warm p50/p95 ms | cached warm p50/p95 ms | cached decode数 / 9回 | retained payload |
|---|---:|---:|---:|---:|
| 30秒 × 1 | 53.201 / 57.023 | 2.072 / 2.351 | 1 | 11,520,000 bytes |
| 15秒 × 4 | 152.522 / 205.119 | 2.795 / 3.809 | 4 | 23,040,000 bytes |
| 300秒 × 1 | 302.617 / 319.592 | 3.020 / 5.763 | 1 | 115,200,000 bytes |
| 300秒 × 2 | 600.708 / 628.920 | 595.690 / 623.981 | 18 | 115,200,000 bytes |

300秒×2は両素材を128 MiBに保持できず、17eviction・0hitとなった。無制限に拡大せず上限を守るため、この条件の準備高速化は小さい。process全体のmaximum RSSは1,300,316,160→1,300,430,848 bytes、peak footprintは1,277,691,464→1,279,788,640 bytesで減少していない。source生成と全case・計時外block検証も含む最大値で、PCM retained payloadと区別する。

通常テストのmedia cache3件でlayout/source clock/bits・Arc sharing・entry/byte eviction・active owner・budget/stream拒否・同長上書きとmtime復元・atomic replace・missing/nonfile/relative wrong hashを確認した。service/playback6件で独立DSP、映像revisionでのPCM再利用、音声編集とold plan、不一致revision、native sessionのtransient Service・別thread prepare・capture後のproject削除を確認した。最初の追加serviceテストはasset stream metadataの欠落をtyped拒否され、実probe metadataをfixtureに追加して成功した。実device安定性と全workspaceの新版確認はまだこれらの部分テストで置き換えない。

linear substepのworkspace receipt後、CPU haloのdisplay-before-cropを維持する修正、tile-sized temporal sampleの余分なVec copy除去、非空resident入力のlinear parity/zero-upload/copy検証を追加した。実Metal linear3件・INSPECT4件は成功した。前節のreceiptはこれらの追加修正前のソースを対象とする。


## 二組のdelivery pipeline

[ADR-0152](../adr/0152-two-frame-delivery-pipeline.md)に従い、callerのGPU tile render/色変換と、独立Runtimeの専用thread encodeを重ねた。per-leg RGBA8/RGBA64を二組だけ循環させ、encoderへの追加pending frameは持たない。GIF第二段階も既存bundleを読み戻しbufferとして使い、callerのcheckpoint gateを待つ。全失敗でdisconnect/join後にstageを回収する。FFmpegのRc/native pointer/error bufferをthread間で共有せず、capability不一致を拒否する。音声単独はthread/bundleとも省く。

[測定JSON](perf-004-video-pipeline.json)。同一release `perf_video_pipeline` binaryのSerial（一組）と既定TwoFrames（二組）を別プロセスで順番に実行した。両側とも独立Runtimeのencode threadを使うため、従来の単一thread binary全体との比較ではない。実Metalのanimated shape一個、12frame・24fps・[1/2,1)、silent PCM24、ProRes4/DNx2、任意3 shutter samplesとGIF/FLAC fan-out。Runtime/GPU初期化・hash/oracleは計時外、coldには最初のshader compileを含む。Scene/media/fontsを含む複雑な長尺作品は別の測定であり、静穏hostの倍率を保証しない。

| case | serial warm p50/p95 ms | two-frame warm p50/p95 ms |
|---|---:|---:|
| ProRes 640×360 / off | 126.144 / 221.854 | 78.184 / 98.617 |
| ProRes 640×360 / 3 samples | 380.685 / 428.795 | 329.441 / 357.616 |
| DNxHR HQX 640×360 / off | 88.238 / 197.391 | 79.234 / 112.473 |
| DNxHR HQX 640×360 / 3 samples | 318.635 / 386.617 | 335.464 / 405.123 |
| ProRes 1920×1080 / off | 1326.616 / 1383.915 | 761.110 / 1115.149 |
| ProRes 1920×1080 / 3 samples | 2852.507 / 3314.177 | 2281.654 / 2526.773 |
| ProRes + GIF + FLAC 640×360 / off | 380.034 / 423.875 | 183.834 / 232.598 |

各cold1+warm8。DNxの3-sampleではp50/p95とも悪化したため、全条件の高速化やcodec別の安定した倍率を主張しない。追加の静穏window・同一contextでの交互比較の対象とする。全81 file組でencoded bytesと独立ffmpegのnative画素/audio f32 bitsが一致した。Movie全12frameの有理数PTS/span、24000 stereo samples、sample range・checkpoint・論理streaming bytes・GPU転送計数も一致した。GIFの12frameはcentisecond spanと既存のduration許容差を確認し、Movieの厳密1/24秒とは区別する。原FrameMetadataはJSONの `frames_common` と各frame差分をmergeするとlosslessに復元できる。

全case連続processのmaximum RSSは122,748,928→143,720,448 bytes、peak footprintは288,523,032→293,552,920 bytes。既存一組のdiagnostic serialへもう一組を足すため増加し、RSS削減とはしない。1080pのconverted bundle payloadは8,294,400→16,588,800 bytes、640pのMovie+GIFは1,843,200→3,686,400 bytes。allocator・native codec thread buffers・GPU pools/driver、tile scratchはこれらpayloadと別である。

通常テストで二つのper-leg allocationが32frameとGIF第二段階を越えて循環し、caller threadのcallback/backend、GIF cancel gate、consumer errorのtyped伝播とpublish stage回収を確認した。最初の故障注入はbuffer長の検査不足で想定errorにならず、nativeへ渡す前のleg数/byte長検査を追加して3件成功した。実Metal/FFmpegでdelivery8件・HDR2件・streaming3件が成功し、streaming4件はignoredのまま。最初のcompileはmutable binding不足、最初のprobe buildはformat argument不足を修正した。fmtとworkspace全target clippyは成功し、全実装後のworkspace testはsource406ファイルを固定して実行中である。

## 検証メモの期待hash監査

全実装後の固定406ソースでfmt、workspace all-targets clippy、実Metal・専用job stateのworkspaceテストが成功した（通常1316件・doctest2件、45件ignored）。[修正前の一括receipt](perf-004-complete-before-hash-validation.json)は、その後のhash修正を含まない。

監査で既存のruntime検証メモがpath/fingerprintだけを比較し、期待content hashを比較しないことが分かった。実MOVを一度decodeしてからファイルを変えず期待hashだけを64個のzeroへ変更すると、以前の検証を再利用して同じ画素を返した。SequentialVideoRenderBackendの回帰テストも修正前に成功描画を返して失敗した。メモの値へ期待hashを加え、再利用はfingerprintと期待hashの両方一致へ限定した。Unix以外は弱いmetadata fenceによるwarm再利用をやめ、毎アクセスの全量hashを通す（[ADR-0153](../adr/0153-expected-hash-verification-memo.md)）。

修正後の実Metalでaudio11件・composition7件が成功した。誤った期待hashはpool counterを増やす前にASSET_HASH_MISMATCHとなり、その後の正しいhashによる描画も保つ。既存alias、forward/reverse、実ファイル書換え、音声共有の再検証も成功した。独立release probeもASSET_HASH_MISMATCHを返した。[再現・修正記録](perf-004-warm-hash-audit.json)。初回追加テストのPartialEq/RenderedFrame型のコンパイル修正、制限環境のcomposition実GPU3件のAdapterUnavailableは成功数へ含めず、実Metal再試行と区別する。修正後の全workspace確認も成功した。末尾の最終receiptを参照する。

## 更新したrelease FFIのnative再生確認

最新のrelease CLI/FFIをnative link directoryへatomic置換し、Swift releaseの音声/Playback・EditEditor・PrecisionEdit・SourceMonitor・TimelineIndexの36件を専用job state・3独立workerで成功させた。先行するunfiltered逐次Swift試行は旧FFIをlinkしていたため途中で停止し、検証成功として数えない。

実AVAudioEngineをaudio device masterにする既存のKronelloAudioHarnessで、非無音300秒stereo PCM16素材を180秒の動くcompositionと配置し、24/1・24000/1001・30000/1001で各40秒観測した。frame137/777へのseekとstop/resumeを行い、全rateでsample計算とstop/resume位置が一致し、underrun・missing audio framesは0だった。[全trace・入力・binary記録](perf-004-native-audio.json)。

| frame rate | 観測device秒 | Metal提示回数 | epoch内提示間隔 p50/p95 ms | 最大frame格子ずれ |
|---|---:|---:|---:|---:|
| 24/1 | 38.626 | 930 | 42.409 / 54.788 | 1 frame |
| 24000/1001 | 38.650 | 930 | 42.704 / 53.755 | 0 frame |
| 30000/1001 | 38.696 | 1164 | 34.574 / 45.433 | 1 frame |

提示間隔は同じepochのnative Metal submission時刻の差であり、frame処理時間や入力から画面までのlatencyではない。物理scanout・speaker loopback・start要求から音の開始まで・native cache hit counterは測っていない。workspace検証と他host作業が並行し、24fps試行にはSwift回帰も重なる。静穏hostでの保証、前後の因果的改善、一般作品のFPSへ読み替えない。素材共有の開始待ち・内容一致は別節のservice probeで照合した。

## DNx temporalの交互順による追加診断

前述の640×360 DNx temporalでp50が318.635→335.464msへ増えたため、同一binary・同一GPU contextで直列/二組の試行順をpairごとに反転し、初回pair + warm21pairを追加測定した。warm p50/p95は直列385.740/509.705ms、二組371.209/468.978msだった。今回は時間増加を再現しなかったが、workspace検証と他host作業が続き、静穏hostではない。前回値を上書きせず、全条件での改善や因果的倍率を主張しない。[全44試行・prototype source](perf-004-dnx-interleaved.json)。

44出力のfile hashはすべて一つで、先の独立decode済みDNx temporal出力のbytesとも厳密一致した。したがってその画素/sample/PTS oracleの内容を維持する。最初の直列試行はshader初期化を含み、最初の二組試行は同じcontextの準備を引き継ぐため、二つを独立cold比較としない。両mode同居processの最大RSS83,607,552 bytes、peak footprint251,740,928 bytesは、独立した前後memory比較ではない。productionソースの固定を保ち、ignored target内の一時prototypeだけを変更して実行した。


## 最終受け入れ

期待hash修正を含む固定406ソースで `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、実Metal・専用stateの `cargo test --workspace --locked` が成功した。通常1316件・doctest2件成功、45件はignoredのまま。開始と終了の対象source hashは一致した。更新したrelease CLI/FFIによるSwift回帰36件、実音声deviceとMetalの3 frame rate確認も成功した。[最終検証receipt](perf-004-final-validation.json)。

音声単独のNoVideo・sample/time/progress、linear-only転送とcodec設定の正しさ、二組pipelineとPCM共有の時間/開始待ち/メモリーというPERF-004の全受け入れ条件を実測または再現可能なテストで確認した。中間版の成功を最終版の成功と混同しない。DNx temporalの最初の時間増加、shared-context追加診断、128MiBのPCM thrash、RSS増加、非静穏host、物理scanout/loopback未計測という限界を維持する。一般FPS保証や性能計画全体の完了ではない。
