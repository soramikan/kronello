# AUDIO-000 基本音声と A/V 書き出し

設計契約: [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)。基本音声の library API を実装する。AUDIO-000 の範囲は定数音量の明示配置。AUDIO-003 で文書由来の再帰配置・音量 Property / Curve と共有 `render.export` / `render.submit` を追加した。AUDIO-004 は明示 resample・audio clip Gain effect・Generator・crossfade を movie profile 3 へ追加した。pitch-preserving stretch は後続。RENDER-003 で movie export の長尺 streaming を追加した（ADR-0074）。

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

保守的な memory budget と対応 codec / layout / timing の範囲は ADR-0049 に記載する。movie export の有界 source / Bus と協調 cancel は ADR-0074 を使う。hardware / GPU 常駐転送、resume、実時間 playback は別の契約で昇格する。検証は [AUDIO-000](../testing/audio-000.md) を参照。

## AUDIO-003: 文書音声と音量

契約は [ADR-0063](../adr/0063-document-audio-and-clip-volume.md)、結果は
[AUDIO-003](../testing/audio-003.md)。`DocumentAudioPlan::compile` は owned RenderSnapshot の Project から
Sequence audio tracks と Composition Media / nested instances を確定配置へ変換する。
Video CompositionClip も同じ音声を一度継承し、Audio track の Composition も対応する。
Media は明示 Asset / stream と node-owned `kronello.audio.volume` Property を参照する。
Audio Media は描画内容を持たず、Video / Image Media の描画は COMP-002 まで型付き未対応。

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

## RENDER-003 の有界 movie 音声

`decode_audio_stream` は finite / PTS continuity を検査して native chunk を順に渡す。
movie export は hash lock を decode 前後で確認し、destination volume の f32 stereo spoolを
`AudioSourceReader` から4,096 framesの窓で読む。純粋 mixer に file / codec を渡さず、
全 source を同時に `AudioBuffer` に展開しない。source / aggregate / movie全Busの10分上限を外し、
既存の `AudioBuffer` / 単体 decode 上限と batchあたりsample operations予算は維持する。
Bus は codec block（PCM24:4,096、ALAC: native block）ごとに絶対sample格子から生成する。
最後の partial blockだけを許し、sample count とzero-origin PTSを従来と一致させる。
`AudioSources` を使う既存 `mix` / `DocumentAudioPlan::mix` は同じ reader経路の互換wrapper。
[ADR-0074](../adr/0074-bounded-streaming-movie-export.md)、[検証](../testing/render-003.md)。
