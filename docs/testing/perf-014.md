# PERF-014: GPU/encoder surfaceと直接packet muxの試作

状態: `in_progress`。依頼計画の最終タスクであり、試作だけで受け入れ済みとしない。GPU入力surfaceの実装・比較、対象形式の全体時間改善と色/時刻/寿命/失敗回収、採用範囲の判断が必要である。

## 先行改善後の対象

[PERF-012](perf-012.md)の4K/24frame実workerでは、wait統合後もCPU出力色変換が約1.63秒、GPU host待機が約0.67秒で、画像readbackは約1.59GB残る。[PERF-013](perf-013.md)の短い48frame ProRes/PCM24 MOVではprivate probe2件を除いても全体wallの改善を確認していない。これを一般的なI/O支配の証明と扱わず、まず高pixel数・高bitrateの圧縮packetを中間MOVへ保存/再読込する費用を直接比較する。

## 直接packet muxの初期実装

`kronello-media/export-prototypes`を明示したbuildにだけ試作を含める。通常export/CLI/GUI/MCPのprofileやcapabilitiesは変更しない。対象はSDR ProRes/stereo PCM24/48k/MOV、zero-originの正確なframe span、最大4K/60,000frames/10分/音声整数sample境界である。フレームは一つの再利用RGBA8 buffer、音声はcodec block、native interleaveは一audio demux packetだけを保持する。ProResの既存4threadと遅延packet用256span budgetを変えない。

既存codec設定からfinal MOVを作り、生成したvideo packetをfinal streamへ直接rescale/muxする。audio stageからは同じdemux packetをDTS順で供給し、video intermediateのcompressed payload保存/再読込を省く。最終probe・stream duration/layout・snapshot metadataを照合し、同一volumeのhard-linkによるno-clobber publicationを行う。未公開のencoder/input packet/contextとstage directoryは失敗/取消で回収する。

初期2件の試験は32×16/48framesの既存中間file経路と全decoded RGBA frame/PTS/duration・全PCM・probe/snapshot hashの一致、取消時の全private stage回収、公開直前に生じた競合fileの保護を確認した（`target/perf-014-direct-tests-fixed.log`、2 passed、0.11秒）。初回は試験が存在しないdecoder APIを呼ぶコンパイル失敗で、成功扱いにしない。固定generator・同一binaryの交互3組を使う`perf_packet_mux`を実装し、release buildとClippyを検証中である。

このprototypeはまだ章転送を受け付けず、固定silence音声だけを生成する。通常の共通exportへ未採用であり、非空chapters、audio primingのあるcodec、複数output/実worker、GPU入力surfaceの条件が未確認である。これらを省略してPERF-014をdoneにしない。将来の対応範囲/採用可否は実測と契約比較を根拠に記録する。

## 初期実測とcontainer metadata修正

[初期1080p/4K比較](perf-014-packet-initial.json)は固定binary `b2d38a4a03b683373d2a891b0d2be9b81265dd752b3436bf5b12b21adb17cc10`、各交互3pairs、ProRes/PCM24、固定opaque gradient generator、silenceである。全decoded pixels/PCM・PTS/DTS/duration/layout/priming・snapshot tagsと圧縮packet payloadは一致したが、最終containerは10bytes短かった。MP4 boxを調べると、baselineのProRes sample descriptionにある`0000000a6669656c0100`（progressive `fiel` box）が直接経路にないことを確認した。

中間MOVのdemuxerが推定するprogressive field情報を直接codec parametersへ明示し、修正後の単体は全file bytes比較を含め2 passed（`target/perf-014-progressive-final-tests.log`、0.06秒）。featureを含む全media target Clippyも成功した。修正前のencoded bytes不一致を、decoded一致へ読み替えて成功扱いにしない。

[修正後4K/24frame・交互9pairs](perf-014-packet-progressive.json)は別の固定binary `4b8790f694e66587bb83953c256b60a5fbb6e0394d6b901c77414b0ce297d936`を使い、全18試行のdecoded/packet/time/metadataと最終file bytesが各A/Bで一致した。各baselineのvideo intermediateは42,079,665bytes、directは0である。実測はgenerator、silence準備、encode、mux、最終probe、no-clobber publicationを含み、独立oracleは測定後に実行する。loaded runtimeは共有し、encoderは試行ごとに新しいものを作る。全作品のrenderer・source decodeを含むworker速度や、単一のpersistent encoder warm測定とは区別する。

初期1080pの後続p50は1,175.8→1,151.0msだったがp95は1,178.4→1,632.8msで悪化した。初期4Kの後続p50は1,201.8→1,187.0ms、p95は1,210.7→1,199.7msだった。compile/Clippyと並行した静穏でないhostの探索値であり、改善率を一般保証しない。修正後9pairsの全phaseと分位値はJSONへ保持し、採用判断にはquietな追加比較と残る契約の検証を使う。

再実行は`cargo build -p kronello-media --example perf_packet_mux --features export-prototypes --release --locked`の後にbinaryを固定hashで保存し、`perf_packet_mux NEW_DIRECTORY WIDTH HEIGHT FRAMES PAIRS`を実行する。独立検査は`scripts/perf_packet_mux_oracle.py --evidence NEW_DIRECTORY/evidence.json --binary FROZEN_BINARY --output NEW_DIRECTORY/verified.json`。出力先を既存trial directoryへ再利用しない。

## compileを重ねない同一binary追加比較

[同じ修正後binaryの4K/24frame・交互9pairs](perf-014-packet-uncontended.json)は、このagentのcompile/Clippy/testを並行実行しなかった比較である。host全体の静穏を保証せず、user/OSの処理を除外したという意味ではない。全18試行で全decoded pixels/PCM・時刻・packet payload・snapshot metadata・最終file bytesが一致した。後続p50は1208.954→1176.189ms、p95は1254.193→1192.783ms。各試行のvideo intermediate 42,079,665bytesの保存/再読込が不要になった。小さな差の一般保証や、GPU/renderer/source decodeを含む実worker全体の速度改善へ拡張しない。非空chapters・実音声・複数出力/worker・GPU surface比較はなお残り、標準exportへは未採用である。

## Metal encoder surfaceの実装と最終実測

`kronello-framebridge/encoder-prototypes`を明示したmacOS buildにだけ、native MetalのRGBA16F fixture生成・GPU BT.709色変換・BGRA8 IOSurface・hardware VideoToolbox H.264入力を実装した。対照は同じMetal sourceをCPUへ読み、同じhalf→f32 transfer/rounding LUTを使って同じCVPixelBufferへ書く経路である。productionのwgpu renderer/workerを通した測定ではない。範囲と採用判断は[ADR-0167](../adr/0167-experimental-export-delivery.md)を正本とする。

[4K/24frame・交互9pairsの最終記録](perf-014-surface.json)はmacOS 27.0.1/arm64、固定binary `502144bfbb2ab1d8fbc44ad53c42089c8d07fb375a24e5ff1718b86c9c9dcbfa`、固定fixture hash `3d780c44195b284fd13c03c9f41df8d5c6f26dad38687201daf3d317f8cb5160`。各試行は新しいMetal/VT資源を作り、LUT準備、source生成、変換、GPU完了、encoder完了、file syncとno-clobber公開までを計時する。独立oracleとこのagentのcompile/testを計時に重ねていない。host全体の静穏は保証しない。process起動時間とprivate stage destructorの後片付け時間は計時に含めない。

| 計測 | CPU読戻し | GPU surface |
|---|---:|---:|
| first trial | 1162.834ms | 622.216ms |
| 後続p50（nearest-rank） | 963.608ms | 636.344ms |
| 後続p95 | 983.015ms | 658.304ms |
| 変換phase後続p50（source生成/完了を含む） | 445.097ms | 86.222ms |
| RGBA16F画像readback/trial | 1,592,524,800bytes | 0bytes |

4bytesの異常status確認はGPU経路にも残る。baselineのpacked CPU half bufferは66,355,200bytes、共通source textureは同サイズ、CVPixelBufferはBGRA8で一枚だけ保持する。row padding・native encoder内部保持・RSSはこのpayload計算と同一視しない。encoder callbackは一圧縮frameを最大64MiBに制限して逐次書き、各frameのcomplete後にinputを再利用する。

全18試行で全decoded YUV420画素、BT.709 primaries/transfer/matrixとvideo range、24frame件数、bitstreamから読めるpacket durationが各A/Bで一致した。全CMSampleBufferの実PTSとdurationをnative callbackで`frame/24`・`1/24`へ照合する。Annex Bにはcontainerのauthored PTS/DTSがないため、MOVの最終timestamp検証を済ませたとは扱わない。全picture NALとSPS/PPSは一致し、圧縮file/packet bytesはVTのtype5/45bytes private SEI（UUID `47564adc5c4c433f94efc5113cd143a8`）のsession値だけが異なった。差のhexをJSONへ保存し、bytes一致とは記載しない。

実機テスト2件は65536要素のtransfer table（subnormal/0/1/NaN/Inf/範囲外）、66×18のrow paddingとalpha 0.5/1のCPU/GPU入力BGRA一致、8frame PTS/duration、3frame後の取消、NaNの型付きnative失敗、stage回収、既存出力の保護を確認した。VT session invalidateをcallback state/pixel/cache/Metal textureの解放より先に実行し、失敗時もGPU command completion後に解放する。同期一枚方式のため、overlap/複数surfaceで同じ寿命契約を満たしたという主張はしない。

再実行は`cargo build -p kronello-framebridge --example perf_encoder_surface --features encoder-prototypes --release --locked`、binaryをhash付きで固定後に`perf_encoder_surface NEW_DIRECTORY 3840 2160 24 9`、`python3 scripts/perf_encoder_surface_oracle.py --directory NEW_DIRECTORY --binary FROZEN_BINARY --output NEW_DIRECTORY/verified.json`。テストは`cargo test -p kronello-framebridge --features encoder-prototypes --locked --lib encoder_prototype`。

## 採用判断と受け入れ範囲

依頼計画の「backend/formatを絞った後段の試作」として、GPU経路は上記SDR/H.264 fixture、直接muxはSDR ProRes/生成silence PCM24/MOV fixtureで全体時間改善を確認した。標準exportへの採用は保留する。非空chapters・実音声・priming codec・複数leg/worker、wgpu renderer/tile/temporal/HDR/他背景/他OSを今回の対応範囲へ広げない。本文の先行段階に残していたこれらの条件は、既定経路への統合・対応範囲拡張に必要な検証であり、済ませた扱いにしない。PERF-014の受け入れは試作・比較・範囲/採用判断の完了を指し、これらの製品機能の完了を指さない。
