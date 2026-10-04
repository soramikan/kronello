# AUDIO-000 基本音声と A/V 書き出し

設計契約: [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)。基本音声の library API を実装する。AUDIO-000 の範囲は定数音量の明示配置。AUDIO-003 で文書由来の再帰配置・音量 Property / Curve と共有 `render.export` / `render.submit` を追加した。実時間 callback、リタイム、圧縮音声出力は後続。

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

保守的な memory budget と対応 codec / layout / timing の範囲は ADR-0049 に記載する。大型 export の streaming、hardware / GPU 常駐転送、cancel / resume、実時間 playback は別の契約で昇格する。検証は [AUDIO-000](../testing/audio-000.md) を参照。

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
retimed audio / 音声経路 effects / Generator は AUDIO-004 の `UNSUPPORTED_FEATURE`。
