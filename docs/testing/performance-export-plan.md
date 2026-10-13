# 追加書き出し性能改善の依頼計画

2026-10-12受領。以下は656ecc3時点の調査と提案の保存であり、実装・実測完了の証拠ではない。実装と受け入れはバックログPERF-006〜014および個別の検証記録で追跡する。

最新の main を前回の調査時点と比較しました。**次に優先したいのは、①同じ素材フレームのタイル間再利用、②書き出し中の検証・SceneIR 評価の共有、③実 worker の進捗保存の軽量化です。** この3点は、現在のコードから重複処理を具体的に確認できます。

今回は書き出しを中心に、素材読み込みから描画・色変換・エンコード・音声・mux・結果保存まで追いました。調査対象は **2026年10月12日に確認した [`656ecc3`](https://github.com/soramikan/kronello/commit/656ecc3a471464f65d93f152f200eb33c2a3f653)** です。以下の時間はリポジトリに保存された測定値で、新しい実機ベンチマークは実施していません。コードから計算できる処理回数・データ量と、実測値を区別して記載します。

## 1. 前回の改善は、次の計画の前提にできる状態です

前回提案した主要項目は、PERF-002〜005として実装されています。書き出しでは、音声単独時の映像処理省略、linear 専用出力、ProRes/DNx の並列度調整、二組のバッファによる描画とエンコードの並行処理まで入っています。プレビュー準備、独立した動画カーソル、seek、DSP checkpoint、GPU面の寿命管理も改善済みです。[実装・測定の一覧](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/performance-improvements.md)

例えば、保存済みの同一バイナリ比較では、1080p ProRes の書き出しが warm p50 で **1,326.6 → 761.1 ms** になっています。ただし、これは「動く図形1個・12フレーム・無音音声」の比較です。実動画のデコード、大量の字幕、長尺の音声、実 worker の進捗DB更新を含む一般的な作品の速度とは分けて評価する必要があります。[PERF-004 の測定条件](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004.md)

したがって次の計画では、**既に並行化した producer／consumer の内部に残る仕事と、その前後の直列処理を減らす**ことを中心にします。

## 2. 最優先：同じ素材フレームの変換を、タイル間で共有する

### 現在残っている処理

通常の動画書き出しでは、出力を最大512×512のタイルに分割します。各タイルでメディアを解決し、その後にGPU描画へ渡しています。このメディア解決では、decoder cursor を再利用できても、SDR素材に対する **素材解像度全体のRGBA変換**が再実行されます。[タイル実行](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/output.rs#L186-L220)、[メディア解決の呼び出し](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/render.rs#L1541-L1553)、[RGBA変換](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/render.rs#L469-L516)

さらに、同じ提示フレームを返す場合にも画素バッファを `clone()` し、native側ではRGBA変換のたびに `SwsContext` を作成・初期化・解放しています。[フレーム再利用](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/video.rs#L525-L536)、[変換contextの寿命](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/native/media.c#L424-L454)

4K素材1本を4K全面へ描画し、temporalとhaloを使わない場合、コード上の処理量は次のようになります。

| 項目 | 1出力フレームあたり |
|---|---:|
| 出力タイル数 | 8列 × 5行 = **40** |
| 素材全体のRGBA8変換 | **40回** |
| `SwsContext` の作成・解放 | **40組** |
| 生成されるRGBA8データの累積量 | 33,177,600 bytes × 40 = **約1.33 GB** |

これは**コピー・変換で生成するデータ量の計算**です。ピークメモリや実測帯域を表すものではありません。decoderの提示区間再利用は機能しており、追加デコード数は素材と要求時刻に依存します。

### 改善案

まず、**出力フレーム／shutter sample の処理中に使う、容量制限付きの素材フレームキャッシュ**を追加します。

- 同じ素材・同じsampleの変換結果をタイル間で共有する。
- native画素やpacked RGBAを `Arc` などの読み取り専用所有で共有する。
- `SwsContext` を入力・出力形式、サイズ、matrix、range、filterが同じ間は保持する。
- キャッシュに件数とbyte数の上限を設ける。
- 既存の独立したdecoder cursorと、素材のfingerprint／期待hashの再検証を維持する。

キャッシュを置く位置が重要です。現在のGPUキャッシュ判定は素材解決より後なので、**素材変換前に再利用を判断できる境界**が必要です。

SDRでは現在のpacked RGBA8と必要画素だけの色変換を維持でき、素材全体をlinear f32へ展開して保持する必要はありません。また、optical flow、reverse、HDRでは要求時刻や色設定が結果に影響するため、これらをキャッシュの同一性に含めます。

**受け入れ条件：** 上記の4K・素材1本のケースで、全素材変換を **40回から1回**へ減らすこと。画素・提示PTS／終了時刻を一致させ、保持量とRSSを別々に計測します。速度の改善率を予想する前に、この回数削減を確認できる施策です。

## 3. 最優先：書き出し用の準備を保持し、temporal のSceneIR評価を共有する

ここには、二つの独立した改善余地があります。

### 3.1 検証・静的準備・hash計算がフレームごとに残っている

movie producer は毎フレーム `render_frame_linear_tiles()` を呼び、その入口は毎回無効設定の `RenderCache` を作り、通常のSceneIR構築を行います。通常入口は `snapshot.validate()` を実行し、metadata作成でもsnapshot全体をJSON化してhashを再計算します。[書き出しループ](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1668-L1729)、[検証経路](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/snapshot.rs#L1100-L1125)、[hash計算](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/snapshot.rs#L518-L530)

既に `ValidatedRenderSnapshot` と `build_scene_ir_prevalidated()` があるため、これを利用した**書き出し寿命の準備済みコンテキスト**を設けるのが自然です。仮に `ExportSession` とすると、保持するものは以下です。

- 検証済みの不変snapshotと、静的なsequence lowering。
- canonical hashとevaluation hash。
- 検証済みのfont／LUTと、そのidentity。
- 上限付きの意味データ用キャッシュ。
- フレームmetadataの共通部分。

同期exportと固定workerが同じコンテキストを使う設計にします。時間依存の評価は各時刻に実行し、静的な準備だけを再利用します。

### 3.2 モーションブラーでは、SceneIR評価が「タイル数 × sample数」になる

現在のtemporal処理は、タイルのループの内側でshutter sampleを処理します。その各sampleが再びSceneIR構築入口を呼びます。[temporalのループ](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/temporal.rs#L272-L347)、[sampleごとのSceneIR構築](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/output.rs#L223-L253)

したがって、4K・3 samplesでは、

- sample用のSceneIR構築：40タイル × 3 samples = 120回
- 外側の中央時刻用：1回

で、**1出力フレームにつき121回**の構築入口呼び出しになります。これは構造上の回数で、121倍遅いという測定結果ではありません。

各一意なsample時刻のSceneIRを先に準備し、タイルごとのDAG構築へ渡せば、通常の3 samplesでは**中央時刻分を含め最大4回程度**を目標にできます。中央時刻がsampleと共通なら、さらに共有できます。

**受け入れ条件：**

- snapshotの構造検証・静的準備・不変hash計算がフレーム数に比例しない。
- 通常の3／8 samplesで、SceneIR評価回数がタイル数に比例しない。
- sample時刻・weight・cut policy・ROI／haloを維持する。
- 現在のtile単位のf64積算と加算順を保ち、画素bit比較を通す。
- sample数が大きい場合も、SceneIRを無制限には保持しない。

大量のクリップ、字幕、テンプレート、モーションブラーを含む作品で、特に効果を確認したい項目です。

## 4. 最優先：実 worker の進捗保存を、処理ブロックの頻度から切り離す

現在、固定workerから渡すcheckpoint callbackは、毎回 `JobStore::checkpoint()` を呼びます。その処理はconnectionを開き、`BEGIN IMMEDIATE`、レコード更新、commitを行います。connectionには `PRAGMA synchronous=FULL` が設定されています。[worker callback](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-service/src/jobs.rs#L1633-L1678)、[DB更新](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-jobs/src/lib.rs#L558-L569)、[checkpoint](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-jobs/src/lib.rs#L706-L718)

このcallbackは、素材音声のdecode chunk、音声encode block、動画フレームなどから呼ばれます。音声準備中は、進捗値が同じ `0` でもDB更新になります。例えば48kHz・1時間・4096 sample blockなら、**音声mixのループだけで約42,188回**です。[音声処理ループ](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1574-L1607)

一方、既存の `perf_video_pipeline` のcallbackは進捗を配列へ追加する処理なので、この実worker固有の費用は含まれていません。[benchmarkのcallback](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/examples/perf_video_pipeline.rs#L115-L130)

### 改善案

**キャンセル・実行権の確認と、表示進捗の永続化を分離**します。

進捗保存は、まず100〜250ms程度の時間間隔でまとめる案を比較します。同じ進捗値の更新も抑えます。専用heartbeatと、最終ファイル確定前のlease／cancel確認は維持します。

**受け入れ条件：** 実DB commit数が「音声block数・frame数」から「経過時間に応じた回数」へ減ること。最終進捗の正確性、キャンセル応答、実行権喪失、DB競合、クラッシュ後の復旧を確認します。キャンセル確認の間隔と、native処理を含めた実際の停止時間は別々に測ります。

## 5. 優先度高：出力色変換の一時バッファと、複数出力の重複を減らす

### 5.1 変換結果を既存のフレームバッファへ直接書く

現在はタイルごと・出力ごとに `hdr_rgba()`／`bt709_rgba()` が新しい `Vec<u8>` を作り、それを二組の再利用バッファへ行単位でコピーしています。HDR→SDRでは、その前に中間の `Vec<[f32; 4]>` も作ります。[変換とコピー](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1681-L1717)

ここは `*_into(destination, stride, offset)` のような入口にし、**変換しながら書き込み先へ直接出力する**変更が適しています。4KのSDR出力なら、出力ごとに約33.2MB／frameの追加コピーを削減する対象になります。

最初の変更では、現在のtransfer関数、alpha処理、範囲検証、丸め順を維持します。HDR→SDRの中間配列を除く際も、現在のf32への丸め境界を保つ必要があります。[現在の色変換](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1961-L2004)

### 5.2 同じ変換条件の出力をまとめる

現在は、各出力legにバッファとencoderを用意し、一つのconsumerが各 `encoder.frame()` を順に呼びます。[consumerの処理](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export_pipeline.rs#L299-L317)

次の順で共有を進めるのがよいです。

1. 背景、transfer、tone map、packed formatが同じ出力の変換結果を共有する。
2. codec設定・解像度・時刻・bitrate・runtimeまで同じ場合は、動画ストリームのencodeを共有する。
3. 異なるencoderへの呼び出し待ちが支配的な場合に、出力別の有界workerを検討する。

最終containerの音声、chapters、export hashは出力ごとに維持します。また、出力ごとにqueueを増やす場合も、**全体の保持量**を管理する必要があります。

### 5.3 nativeのRGBA→YUV変換を個別に測る

現在はハードウェアencoderを選んでも、入力前にCPUの `sws_scale()` が走ります。ここを、codec自体の時間と分離して計測します。[native encoder入力](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/native/media.c#L620-L631)

変換側の並列度を1／2／4で比較する余地はありますが、ProRes4・DNx2のcodec内部threadsと合わせた予算で評価します。なお、**SVT-AV1の `lp=1` は、前回の試作で出力の再現性・画素一致が崩れたため維持された設定**です。ここは既存の判断を引き継ぎます。[codec並列度の検証](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004.md#codec並列度の予備比較)

## 6. 優先度高：書き出し音声のmixを共有し、開始前の処理量を減らす

PERF-004のPCM共有は再生準備向けです。書き出しでは、現在も選択された音声素材を全量decodeして一時PCMファイルへ保存し、その後、**各出力legについて個別にplan／sessionを作り、全範囲をmix・量子化・encode**します。動画処理はこの音声処理の後に始まります。[音声sourceと出力別処理](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1513-L1623)、[PCM spool](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/streaming.rs#L50-L99)

素材decodeの共有は既にありますが、例えば同じ音声をMOVとFLACへ出す場合でも、mix／DSPの共有は残っています。

### 改善案

最初は、**同じ音声結果になる出力をグループ化し、mixと量子化を一度だけ行う**変更を推奨します。

同一性には、音声plan、意味版、素材lock、出力範囲、channel layout、gain／effects、clipping方針を含めます。生成したPCM blockをcodec別に供給し、codecごとのblock長の違いは小さいバッファで吸収します。DSP／WSOLAの加算順・状態遷移は維持します。

その後、長い素材から短い区間を書き出す場合の準備時間を改善します。現行方式では、48kHz stereo f32の1時間素材は**約1.38GBの一時PCM**になります。まず検証済みspoolの再利用を検討し、範囲decodeは、resampler履歴、DSP／WSOLAのpreroll、全authored source rangeの検証を整理してから進めます。

音声と動画の処理並行化も候補ですが、最初にmixの重複を減らし、CPU・I/Oの競合を測ってから導入する順番が適切です。

**受け入れ条件：** 同一音声の複数出力でmix回数が出力数に比例しないこと。全sample、layout、clipping数、最終部分block、codecのpriming／padding、PTSが一致することです。

## 7. 優先度高：GPUのstatusと画像readbackを、一度の完了待ちにまとめる

linear-only化の後も、現在はGPUの異常検出statusを読み戻して待ち、その後にlinear画像を読み戻して再度待っています。[status readback](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-gpu/src/scene_gpu.rs#L1101-L1151)、[linear画像の取得](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-gpu/src/scene_gpu.rs#L1598-L1618)

保存済みの4K測定でも、この構造が計数に表れています。

| 条件 | GPU readback量 | readback回数 | wait回数 |
|---|---:|---:|---:|
| temporalなし | 66,355,360 bytes | 80 | 80 |
| 3 samples | 199,066,080 bytes | 240 | 240 |

出典は [`perf-004-linear-measurements.json`](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004-linear-measurements.json) の4Kケースです。GPUから読むlinear面はRGBA16F、8 bytes／pixelで、CPU側のf32配列のサイズとは区別します。

### 改善案

まずstatusと画像のcopyを同じ完了単位にまとめ、両方のmapを開始してから一度だけ待ちます。wgpuの `PollType::Wait` は指定submissionの完了までブロックするため、どこで待つかが処理の重なりに影響します。[Rust](https://docs.rs/wgpu/30.0.1/wgpu/type.PollType.html?utm_source=chatgpt.com)

構造上の目標は、上記の **waitを80→40、240→120へ減らす**ことです。画像とstatusのcopy自体は必要なので、readback回数や転送byte数まで同じ割合で減るとは限りません。

次に、容量制限付きのstaging buffer poolで、GPU処理とCPU側の色変換を重ねる案を検討します。その際は、GPU完了・map解除・consumer終了までbufferを再利用しないこと、異常statusの確認前に結果やキャッシュを公開しないことを維持します。

GPU側での最終量子化やtemporal積算は、その後の別タスクにします。現在のCPU色変換やf64積算と数値仕様が関わるため、待機・allocation削減とは分けて受け入れるべきです。

## 8. 長尺で重要：フレームmetadataと結果保存を有界にする

動画payloadは二組のバッファに収まっていますが、metadataは全フレームを `Vec<FrameMetadata>` に保持しています。snapshot hash、font locks、semantic versions、画像形式など、共通情報も毎フレーム繰り返します。その後、workerは全reportをJSONの `Value` に変換し、publication用にcloneします。[metadata生成](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-render/src/output.rs#L288-L335)、[reportのJSON化・保持](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-service/src/jobs.rs#L1817-L1880)

保存済みの1080p ProResの測定JSONから元のmetadataを復元すると、あるwarm試行では12フレームで36,553 bytes、**約3,046 bytes／frame**でした。この密度を1時間・60fpsへ単純に延ばすと、フレームmetadataだけで**約658MB**になります。これは長尺書き出しを実行した結果ではなく、既存データからの推計です。[元の測定JSON](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004-video-pipeline.json)

一方、publication receiptには**512MiBの上限**があり、読み戻す際もファイル全体をメモリへ読み込みます。[保存時の上限](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-jobs/src/recovery.rs#L459-L476)、[読み戻し](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-jobs/src/recovery.rs#L688-L720)

### 改善案

**版管理付きの共通header＋分割metadata**へ移行する計画を立てます。

- 不変情報は共通headerへ一度だけ記録する。
- frameごとの差分は固定上限のchunkにする。
- 小さいroot receiptに、chunkのhash、frame range、count、identityを記録する。
- status取得はsummary、詳細はページ単位で読む。
- resume／reconciliationもchunkを逐次検証する。
- 既存receiptを読む互換経路を残す。

これはreport writerだけでなく、reader、結果API、復旧処理まで含む設計変更です。既存ADRもmetadataの外部manifest化を後続課題としているので、その次段階として扱えます。[ADR-0079](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/adr/0079-bounded-streaming-movie-export.md)

**受け入れ条件：** 長尺でもmetadataの生成・読み戻しに必要なメモリが全frame数に比例しないこと。元のmetadataを復元でき、chunk欠落・破損・順序違いを拒否し、旧receiptからの復旧を維持することです。

## 9. 書き出し終盤：mux・再検査・確定も独立して改善する

現在は動画と音声をそれぞれ中間ファイルへ書き、最後に圧縮packetを読み直して最終containerへmuxします。同じ中間ファイルへのprobeも、export本体、`mux_movie_stage()`、native mux内部で繰り返されています。[export終盤](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1790-L1836)、[mux準備](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export.rs#L1168-L1204)

長尺・高bitrateのProRes／DNxや複数出力では、この最終工程が効く可能性があります。

改善は次の順がよいです。

1. 同じprivate stageに対する既取得のprobe結果を受け渡し、重複open／検査を減らす。
2. mux中にも、時間またはpacket数に応じたキャンセル確認を入れる。
3. 計測でI/Oが支配的なら、生成した動画packetを最終stageへ直接muxする経路を検討する。

最終containerの検証、PTS／DTS／duration、audio priming、chapters、snapshot metadata、失敗時のstage回収は維持します。GIFはglobal paletteを作る二段階処理なので、別の処理として扱います。

さらに先の候補として、**GPU出力をencoder入力面へ渡す経路**があります。現行reportにも `cpu_rgba_to_hardware_encoder` と明記されており、hardware codecを使ってもCPU色変換・転送は残っています。[現在の入力経路](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/src/export_pipeline.rs#L356-L379)

これはmacOSのVideoToolbox連携など、対象backendを絞った後段の試作が適切です。色差のsampling、range、丸め、surface寿命を含むため、前述のコピー削減・待機統合の成果を測ってから着手します。

## 10. 次の測定計画

### 最初に測定記録を一か所整理する

今回、codec比較資料に不整合を確認しました。

`perf-004.md` は54組のencoded bytes一致を記載していますが、対応するraw JSONは全6ケースで `all_encoded_bytes_equal: false` です。decoded bitsとPTSは一致しています。また、比較前後のsnapshot hashが異なり、記録されたbefore／afterのbinary hashは同一でした。[本文](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004.md)、[raw JSON](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/docs/testing/perf-004-codec-delivery.json)

benchmarkが実行時に新しいIDを作ることはsnapshot差の一因です。現段階では測定資料の整合性問題として扱い、**固定fixture・snapshot identity・実行binary hashを揃えて再採取**する項目を最初に入れます。[fixture生成](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/crates/kronello-media/examples/perf_codec_delivery.rs#L61-L81)

なお、二組のpipelineの比較JSONでは81件のencoded bytes／decoded bits一致を確認できており、こちらは別の証拠として利用できます。

### 実 worker を通すケースを追加する

次は `render.submit` またはGUIからの投入を起点に、`succeeded`までを計測します。

| ケース | 主に確認すること |
|---|---|
| 4K実動画1本、temporalなし | 素材変換の重複、decode、色変換、encodeの割合 |
| 同じ動画、3／8 shutter samples | SceneIR共有、sample source共有、GPU待機 |
| 数千clip＋日本語字幕・blur | 書き出し準備、hash、静的処理の再利用 |
| 同じ素材の複数配置、異なる開始点・速度 | 独立cursorと共有frameの両立 |
| ProRes／H.264等＋FLACの複数出力 | 変換・音声mix共有、consumer待ち、総保持量 |
| 長い音声素材の部分書き出し | spool準備、preroll、開始待ち |
| 長尺・高frame数 | metadata増加、mux、receipt生成、復旧 |

記録する時間は、準備、音声decode／mix／encode、SceneIR／DAG、素材変換、GPU実行／待機、出力色変換、encoder、mux、最終検証・hash・結果保存です。加えて、buffer返却待ち、DB commit数、変換回数、全出力のstageサイズ、保持payload、RSSを分けます。

短尺比較は同一条件のA/Bを交互に実行し、coldとwarm、p50とp95を残します。長尺は完走確認と定常処理・メモリ増加の観測を分け、metadata経路は軽いbackendでも境界を検証します。GPU変更はまず既存の基準機・Metalで評価し、他backendの結果を混ぜずに記録します。

## 11. 推奨する実装順

以下のIDは、**追加するバックログ項目の提案**です。

| 提案ID | 優先度 | 実装単位 | 主な完了条件 |
|---|---|---|---|
| **PERF-006** | P0 | 実workerの全工程計測・固定fixture・測定記録の整合 | 同じ入力で再現でき、stage時間・待ち・回数を追える |
| **PERF-007** | P0 | 書き出し用の準備済みコンテキスト、sample SceneIR共有 | 不変処理がframe数に比例せず、通常temporalの評価がsample数程度になる |
| **PERF-008** | P0 | 素材frame共有、変換結果cache、SwsContext保持 | 4K素材1本の全素材変換40→1、画素一致、容量上限 |
| **PERF-009** | P0 | workerの進捗・取消確認・heartbeatの整理 | block／frameごとのDB書き込みを減らし、取消・復旧を維持 |
| **PERF-010** | P1 | 色変換の直接書き込み、同条件の複数出力共有 | 一時buffer・行コピー・重複変換の削減、変換bytes一致 |
| **PERF-011** | P1 | 音声mix共有、長尺source準備の改善 | 同一音声のmixが出力数に比例せず、sample一致 |
| **PERF-012** | P1 | GPU status／画像待機統合、staging再利用 | wait回数削減、数値異常の検出・buffer寿命を維持 |
| **PERF-013** | P1 | 長尺report／receiptの分割、確定工程の改善 | メモリ上限、旧receipt互換、長尺完走・復旧 |
| **PERF-014** | P2 | GPU色変換・encoder surface連携、packet直接muxの試作 | 対象形式で全体時間の改善、色・時刻・寿命の契約を満たす |

**着手順は、PERF-006で基準を作り、PERF-007・008・009を独立した変更として進める案を推奨します。** PERF-010の直接書き込みも比較的狭い変更なので、その後すぐに進められます。GPU専用経路や大きな並列化は、ここまでの改善後に残る費用を見て対象を絞ります。

書き出し以外では、TimelineIndex導入後も全track／clipのView構築が残っているため、可視範囲と少し外側のclipだけを描くviewport対応が次候補です。ただし今回は、上記の書き出し改善を先に進める方が、調査で確認できた重複処理へ直接対応できます。[現在のtrack描画](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/apps/macos/Sources/Kronello/EditPage.swift#L757-L766)、[clip描画](https://github.com/soramikan/kronello/blob/656ecc3a471464f65d93f152f200eb33c2a3f653/apps/macos/Sources/Kronello/EditPage.swift#L898-L907)
