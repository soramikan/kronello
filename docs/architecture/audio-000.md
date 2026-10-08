# AUDIO-000 基本音声と A/V 書き出し

設計契約: [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)。基本音声の library API を実装する。AUDIO-000 の範囲は定数音量の明示配置。AUDIO-003 で文書由来の再帰配置・音量 Property / Curve と共有 `render.export` / `render.submit` を追加した。AUDIO-004 は明示 resample・audio clip Gain effect・Generator・crossfade を movie profile 3 へ追加した。AUDIO-002 は buffered realtime playback を追加した（実デバイスの受け入れは検証文書に記録）。pitch-preserving stretch は後続。RENDER-003 で movie export の長尺 streaming を追加した（ADR-0079）。AUDIO-007/008 は音声用 clip effects（EQ・HPF/LPF・compressor・limiter）と LUFS 計測・ノーマライズ、AUDIO-009 は再生メーター・スクラブ・ミキサーを追加した（ADR-0117）。

## 依存境界

```text
固定 RenderSnapshot + 音声配置
  → AvExportSnapshot（owned / hash / schema 1）
  → kronello-media: 素材解決・hash 検証・FFmpeg decode / swresample
  → kronello-audio: absolute sample grid / Gain / Bus / PCM24 quantization
  → kronello-render: render_frame + 呼出側の RenderBackend
  → kronello-media: ProRes / PCM24 encode / MOV mux / probe / publication
```

`kronello-audio` は model / time / animation の意味的な型・評価と serde を利用する純粋層で、workspace の unsafe 禁止を継承する。具象 FFmpeg resource は media の C shim と `ffi.rs` だけに閉じる。AUDIO-000 は render / model / time / service の API と RenderSnapshot schema を変更しなかった。AUDIO-003 は model の Clip volume / Media node と service API を拡張し、純粋 audio compiler は model / time / animation に依存する。RenderSnapshot の構造版は維持する。

## 公開 API

| crate | 型・関数 | 契約 |
|---|---|---|
| audio | `Gain::new` / `Gain::UNITY` | 非負で有限の線形振幅倍率 |
| audio | `AudioBuffer::new` / `frames` | 48 kHz stereo、有限の left/right f32、headroom を保持 |
| audio | `AudioClip` / `AudioSources` | AssetId + stream_index の source と、絶対 placement / source_in / Gain |
| audio | `sample_index` / `sample_range` | 絶対 rational 時刻から checked 整数演算で floor |
| audio | `apply_gain` / `mix` | 定数 gain の適用、clip 順に Bus へ加算。入力と要求は変更しない |
| audio | `Bus::quantize_pcm24` | Reject / 明示 Saturate。PCM24 の S32 payload と飽和した channel sample 数 |
| media | `MediaRuntime::decode_audio` | 明示 local file / stream の decode + swresample drain |
| media | `decode_asset_audio` | 固定 Asset の locator 解決と decode 前後の hash 照合 |
| media | `encode_audio` | Bus を 48 kHz stereo PCM24 の MOV に符号化 |
| media | `probe` / `MediaProbe::verify_av` | libavformat で stream metadata を確認。ffprobe subprocess は使わない |
| media | `mux_av` | zero-origin ProRes / PCM24 の packet を MOV へ mux、検証後に確定 |
| media | `AvExportSnapshot::new` / `content_hash` | RenderSnapshot と音声配置を一つに固定、strict serde envelope |
| media | `export_av` / `AvExportRequest` / `AvExportReport` | 同じ snapshot から音声と既存映像経路を実行、両 hash・frame metadata・sample range・codec report・probe を返す |

## 時間と配置

48 kHz の `floor(start × 48000)..floor(end × 48000)` を共有する。24 fps では 2,000 samples/frame、30000/1001 fps では最初の 5 フレームが 1601 / 1602 / 1601 / 1602 / 1602、60000/1001 fps では 800 / 801 / 801 / 801 / 801。各境界を直接計算し、隣接バッチに欠落・重複を作らない。負時刻も数学的 floor で扱う。

`source_in` は最初のデコード済み source sample に対する trim。source の container start PTS は `DecodedAudio::source_start` へ保持するが、配置に二重加算しない。placement 開始・終了と source_in を絶対に floor し、source sample offset は配置先 sample index の差で決める。足りない source、素材欠落、hash 不一致、非連続 PTS、未対応 channel layout は型付きエラー。speaker mask のない 1 / 2 channel は mono / left-right と明示解釈する。

frame に整列した range を元の絶対時刻で render し、encoded PTS は range.start を引く。音声も同じ range から作って出力原点をゼロにする。rational 映像 duration と整数音声 sample count の差は 1 sample 未満までを許容する。最終 mux の stream duration は stage stream と厳密に一致しなければ公開しない。

## 不変入力と出力

AUDIO-000 時点で Project に timeline audio placement の正本型がなかったため、`AvExportSnapshot` は純粋な明示入力 envelope として固定する。独立した編集状態や暫定の unknown Project field は作らない。audio asset は owned RenderSnapshot の Project.assets からだけ引く。

RenderSnapshot の既存 hash は映像文書・素材 lock を識別する。export snapshot hash はさらに audio placement / stream / source_in / Gain と envelope schema を含む。clip gain だけ変えると export hash は変わり、RenderSnapshot の hash は変わらない。MOV の `kronello_render_snapshot_hash` / `kronello_export_snapshot_hash` と report の両 hash でこの境界を明示する。export request の range / fps / region / background / clipping policy は report に別途保存する。

最終映像は explicit background を使う SDR linear Rec.709 → BT.709 encoded RGB → RGBA8 → ProRes。音声は stereo f32 → 明示 clipping policy → PCM24。stage の audio / video の sample 数・duration を要求と照合し、mux 後も codec / start PTS / duration / metadata を確認する。最終ファイルは既存成果物を上書きせず同じ volume の stage から確定する。

保守的な memory budget と対応 codec / layout / timing の範囲は ADR-0049 に記載する。movie export の有界 source / Bus と協調 cancel は ADR-0079 を使う。hardware / GPU 常駐転送、resume は後続。実時間 playback は下記 AUDIO-002 の別契約で扱う。検証は [AUDIO-000](../testing/audio-000.md) を参照。

## AUDIO-003: 文書音声と音量

契約は [ADR-0063](../adr/0063-document-audio-and-clip-volume.md)、結果は
[AUDIO-003](../testing/audio-003.md)。`DocumentAudioPlan::compile` は owned RenderSnapshot の Project から
Sequence audio tracks と Composition Media / nested instances を確定配置へ変換する。
Video CompositionClip も同じ音声を一度継承し、Audio track の Composition も対応する。
Media は明示 Asset / stream と node-owned `kronello.audio.volume` Property を参照する。
Audio Media は描画内容を持たない。COMP-002 の Image / Video 視覚 stream は document audio の配置から除外し、Video asset の別音声 stream は従来どおり明示 MediaNode で配置する。

`JobOutput::ProResMov` の省略値は `profile_version: 1, audio: explicit`。version 2 の document は
文書音声、explicit は clips（空なら無音）、silence は意図的な無音。document / silence と
非空 clips は拒否する。同期 `render.export` と job は同じ envelope / mixer / exporter を使う。
report の `audio_source` / `audio_profile_version` にも選択を記録する。

Clip の optional volume Property（省略は unity）は source-local time、Media の volume Property は
Composition-local time で定数 / Curve を sample ごとに純粋評価する。負・非有限・f32 範囲外の Gain を拒否する。
文書音声は合成済み rational offset の逆写像を一度 floor する affine sample mapping と祖先 active 区間の交差を使い、
fractional trim でも phase を保持する。明示 clips の既存二境界 floor は変更しない。
1024 placements / 64 nested scopes を上限とし、使用 Property / Curve は compiled plan が所有する。
movie profile 1/2 の retimed audio / 音声経路 effects / Generator は `UNSUPPORTED_FEATURE` のまま。

## AUDIO-004: 版付き stateless 音声

契約は [ADR-0069](../adr/0069-versioned-stateless-audio.md)、証拠は
[AUDIO-004](../testing/audio-004.md)。`DocumentAudioPlan::compile_version(..., 1)` / `compile`
は AUDIO-003、version 2 は AUDIO-004。`AvExportSnapshot::with_audio_profile(..., 3)` と
共有 movie `profile_version: 3` が evaluator 2 を固定する。省略 profile 1 / explicit と
profile 2 の音声は変えない。音声の semantic version は export envelope の profile に固定し、
RenderSnapshot の映像 SemanticVersions / hash の構造を変更しない。

- Audio track の Asset Clip は `audio_retime: resample_v1` で positive Linear / PiecewiseLinear
  TimeMap に対応する。absolute floor sample index から有理数 source position を求め、f64 の
  two-tap linear interpolation を f32 へ丸める。speed に比例して pitch が変わり、anti-alias
  filter / pitch preservation は提供しない。`reject` は従来の unity-speed のみ。
- floor 格子の最初の部分 bucket は元の fractional start より前にある。resample_v1 は
  PiecewiseLinear の最初の segment をこの一 bucket だけ明示外挿する。negative source
  position や不足した interpolation tail はエラー。trim は新しい先頭 segment を用いるので、
  fractional breakpoint で trim した最初の bucket にはこの規約が適用される。
  unity / Reject の旧 affine phase と旧 export の sample bits は維持する。
- Audio track の `kronello.audio.gain` effect version 1 は `AudioGain {gain: PropertyId}` と
  clip-owned `kronello.audio.volume` Property を参照する。Constant / Curve を Sequence time
  で評価し、volume（source-local）→ authored effects 順 → crossfade → mix の順で乗算する。
- `kronello.audio.silence` / `kronello.audio.tone440` Generator version 1 は audio track のみ。
  tone440 は source time 原点の440 Hz sine、stereo amplitude 0.25。固定入力は generator id /
  version、TimeMap、source_in、volume。共通 SourceRef の color は既定 opaque black だけを
  受理し、音声パラメータへ転用しない。未知 id / version / parameters を silence に代替しない。
- Transition Crossfade version 1 は音声でも linear amplitude。sample range `[a,b)` で
  `u=(n-a)/(b-a)`、outgoing `1-u` / incoming `u`。a / b は絶対 floor 境界。equal-power に
  置換しない。audio track と audible Video CompositionClip の再帰音声に対応する。
- Video CompositionClip の transform / opacity Property は継承音声に影響させず、
  evaluator 1 と同じ sample bits を保つ。映像用 Property の存在だけで音声を拒否しない。
- plan は必要な clip / Property / Curve を所有する。評価には Project / file / clock / random /
  codec state を与えず、source は hash 確認済み immutable AudioSources。 arbitrary-order batch
  と一括は同じ順序で加算する。source 範囲は要求 batch / mute と無関係に検証する。
- 1024 tracks / authored clips / transitions / flattened placements、16 effects / clip、1024 map
  points、effect curve 4096 keys / curve・65536 keys / plan、Bus 28,800,000 frames、batch あたり
  100,000,000 sample operations。overlap samples の source / gain / fade work に加え、
  legacy 継承 mixer ごとに request 全体の Bus 初期化・有限検査（`2 × request frames`）を
  overlap の有無と無関係に加算する。出力を確保する前に budget を検証する。
  cost の式は ADR-0069 を参照する。

Composition 内部の retime / node effects、retimed CompositionClip、audio effect を持つ video clip、
Protected / hold / loop map、任意 Generator、外部 effect、pitch-preserving stretch は型付き未対応。
Composition target の profile 3 は既存の recursive unity audio を使う。typed error は
`UNSUPPORTED_FEATURE`、`AUDIO_BUDGET_EXCEEDED`、`INVALID_AUDIO_INPUT`、
`AUDIO_SOURCE_TOO_SHORT`、`AUDIO_OVERFLOW` と既存 asset / time / clipping codes。

## MEDIA-002: ALAC 圧縮音声

`av1_mp4` / `h264_mov` / `hevc_mov` version 1 は evaluator 2 を固定し、既存 Bus と
PCM24 quantizer の出力を native ALAC へ lossless encode する。48 kHz stereo S32P / 24 bit、
zero initial_padding、4096 sample coding frame と正確な部分最終 frame、drain 後の全 packet。
音声 stage は明示 MP4、最終は選択した MP4 / MOV へ packet copy。旧 PCM24 stage はMOV。
追加 `AvExportSnapshot::with_movie_profile` は MovieProfile を envelope hashへ含める。
`MediaRuntime::encode_alac` / `mux_movie` / `MediaProbe::verify_movie` は閉 profile の契約を使う。
`AvExportReport.movie_profile` は追加形式だけに出現し、audio_profile_version は3。
既存 constructors / report はこの optional field を省略し、旧 hash / sample bits を維持する。

N decoded samples の start は0、last PTS は(N-1)/48000、exclusive endはN/48000。
映像の先頭・最後と duration 差1 sample未満を検証し、ALAC roundtrip は量子化後の全 channel sample
を bit比較する。AAC-LC の priming / padding / edit list / iTunSMPB と配布・特許・品質レビュー、
AV1 + Opus の Web 配信は未採用。詳細は [ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md)、
CPU / host の証拠を分けた [MEDIA-002](../testing/media-002.md)。

## AUDIO-002: buffered realtime playback

[ADR-0076](../adr/0076-buffered-device-clock-playback.md)、[検証](../testing/audio-002.md)。
`kronello-service::PreparedAudio` は revision を照合した owned evaluator-2 plan と immutable
decoded sources を持つ preview runtime resource。共有 registry の operation ではない。
`render_block` は absolute `[start_sample,start_sample+frames)`、最大4096 frames の binary stereo f32。
同じ入力の export evaluator と PCM24 前の bits を比較する。source 合計は既存28800000 frames、
decode は残 budget を append 前に確認し `AUDIO_BUDGET_EXCEEDED`。

macOS の preparation / producer queues は作品・Metal worker と別。`AVAudioSourceNode` callback は
32768-frame C11 lock-free SPSC ring の copy / silence だけ。Rust FFI / JSON / disk / evaluator を
callback に入れない。snapshot 変更は producer block 境界で公開する。buffer の編集待ちは最大
682.667 ms + output latency + preparation。source の再生に独立した GUI 編集状態を作らない。

master は callback の output sampleTime / hostTime。exact integer math で latency を補償して
frame floor を求める。seek は既存 rational floor 格子へ flush、stop/resume は整数 sample を保持。
underrun は silence / count、遅着 sample は破棄し device clock を継続する。
producer / timestamp / device change の typed failures を表示する。音声なし / mute / デバイスなしは
理由付き host clock。実 engine + Metal の40秒×3-rate harness と解析手順は検証文書を参照し、
CPU / export / synthetic callback checks を実時間同期の受け入れに読み替えない。

## RENDER-003 の有界 movie 音声

`decode_audio_stream` は finite / PTS continuity を検査して native chunk を順に渡す。
movie export は hash lock を decode 前後で確認し、destination volume の f32 stereo spoolを
`AudioSourceReader` から4,096 framesの窓で読む。純粋 mixer に file / codec を渡さず、
全 source を同時に `AudioBuffer` に展開しない。source / aggregate / movie全Busの10分上限を外し、
既存の `AudioBuffer` / 単体 decode 上限と batchあたりsample operations予算は維持する。
Bus は codec block（PCM24:4,096、ALAC: native block）ごとに絶対sample格子から生成する。
最後の partial blockだけを許し、sample count とzero-origin PTSを従来と一致させる。
`AudioSources` を使う既存 `mix` / `DocumentAudioPlan::mix` は同じ reader経路の互換wrapper。
[ADR-0079](../adr/0079-bounded-streaming-movie-export.md)、[検証](../testing/render-003.md)。

## AUDIO-007/008/009: フィルタ・ダイナミクス・LUFS・再生メーター

契約は [ADR-0117](../adr/0117-audio-filters-dynamics-loudness.md)、検証は
[AUDIO-007](../testing/audio-007.md) / [AUDIO-008](../testing/audio-008.md) /
[AUDIO-009](../testing/audio-009.md)。evaluator 2 の clip effects 契約を拡張し、
`clip_set_effects` の既存の流れで 5 つの v1 エフェクトを受理する。

- `kronello.audio.eq`（`AudioEq {bands}`、DataTable `kind`/`freq_hz`/`gain_db`/`q`、1..=8 行）、
  `kronello.audio.hpf` / `kronello.audio.lpf`（`cutoff_hz`、`order` 1..=4）は
  `dsp.rs` の biquad（RBJ peaking/shelf・Butterworth カスケード、奇数次は一次 section）。
  `kronello.audio.compressor`（threshold/ratio/attack/release/makeup）は
  ステレオリンク peak detector + dB 域 gain computer、`kronello.audio.limiter` は
  瞬時 attack + 指数 release の ceiling。係数は `resolve_audio` の検証済み定数から
  決定的に導出し、パラメータは Constant source 限定。
- ステートフルチェーンはプレースメント先頭から評価するため、要求範囲の任意分割は
  連続ミックスと bit 一致する。リアルタイム再生と書き出しは同一コード経路。
  非 Constant・映像トラック・範囲外値・不正テーブルは型付きエラー。
- `audio.loudness` クエリは clip / sequence / asset 対象を共有 plan で render し、
  `loudness.rs` の ITU-R BS.1770-4 K 重み付き integrated / momentary / short-term LUFS と
  4x オーバーサンプル true peak（dBTP）を返す。`audio.normalize` は integrated LUFS から
  目標ゲインを `kronello.audio.gain` として `edit.plan` / `edit.apply` で 1 イベント追記し、
  Undo 可能・冪等。
- `mix_metered` はミックスと同一ループでトラック別・マスターの peak/RMS を計測する。
  `PreparedAudio::render_block_metered` と FFI `kronello_audio_render_metered` が
  同じ評価経路の PCM とメーター JSON を返し、GUI の VU メーター（トラックヘッダ /
  ミキサー）へ publish する。オーディオスクラブは既存 realtime パイプラインの
  有界短区間ランとして実装し、専用評価経路は作らない。
- トラックゲインの UI マッピング: モデルにトラックレベルのゲイン欄がないため、
  ミキサーフェーダーはトラック上全クリップの `kronello.audio.volume` へ
  `clip_set_volume` で同一ゲインを 1 イベント書き込む代理方式（Undo 可能）。

## AUDIO-010: ピッチ保持リタイムとマルチチャンネル

契約は [ADR-0124](../adr/0124-pitch-preserving-retime-and-multichannel.md)、検証は
[AUDIO-010](../testing/audio-010.md)。

- `ChannelMask`（model の版付き u64 speaker bitset）は mono / stereo /
  5.1(side) / 5.1(back) / 7.1 の closed set だけを受理し、未知マスク・
  チャンネル数不一致は `UNSUPPORTED_CHANNEL_LAYOUT`。position ベースの
  任意配置は扱わない。
- `ChannelBuffer` / `ChannelBus` / `ChannelSources` / `ChannelSourceReader` が
  decode → bus ミックス → エフェクト → encode の全段で `channel_mask` を保持する。
  エフェクトはチャンネルごとに適用（compressor/limiter のみ LFE 除外規則）。
  `mix_channels` は出力 `channel_mask` への明示変換のみを行い、暗黙の
  fold-down はしない。ダウンミックスは ITU 係数（center -3dB、surround -3dB、
  LFE 既定除外）の決定的係数表で、mono 出力・legacy stereo reader も同じ経路を通る。
  `into_stereo_bus` は非 stereo を拒否する。
- `AudioRetimePolicy::PitchPreserveV1`（wire 値 `pitch_preserve_v1`）は
  `wsola.rs` の決定的 WSOLA（窓 1024・合成ホップ 512・探索半幅 256、
  正規化相互相関・固定候補順・f64 蓄積）でピッチを保持する。乱数・外部
  プロセス・プラットフォーム DSP を使わず、リタイム比は有理数タイムマップから
  導出する。正速度の Linear / PiecewiseLinear のみ、piecewise slope 0 の hold
  は明示的な無音、speed 1 は `resample_v1` と同一経路。WSOLA の hop anchor は
  placement 先頭に固定されるため、分割レンダーは連続レンダーと bit 一致する。
  非対応マップ・reverse 組合せ・ソース不足は型付きエラー。
- media の decode は FFmpeg `AVChannelLayout` のネイティブ mask を公開し、
  >2ch で mask 未指定の素材は `UNSUPPORTED_CHANNEL_LAYOUT`（mono / stereo の
  未指定は documented default layout に正規化）。encode / mux は ch 数と mask を
  受け、宣言と実レイアウトの不一致は型付きエラー。probe は `channel_mask` を報告し、
  `MediaProbe::verify_movie_layout(profile, layout)` が codec・ch 数・mask・
  0-origin PTS を出力検証する。`encode_audio_channels` / `mux_movie` は
  `AvExportSnapshot::with_audio_layout` が pin する audio envelope 3 と対になる。
- `JobOutput` のムービー出力は `audio_layout: Option<ChannelMask>`（省略 = stereo の
  後方互換）を持ち、非 stereo は `profile_version` 3 を要求する。submit・worker の
  双方が同じ `movie_snapshot` / `verify_movie_layout` を通すため、CLI・MCP の
  共有経路は同じ検証を受ける。
