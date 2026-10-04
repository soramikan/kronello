# AUDIO-000 基本音声と A/V 書き出し

設計契約: [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)。基本音声の library API を実装する。実時間 callback、音量アニメーション、リタイム、圧縮音声出力、service / CLI / MCP の新しい command は今回の範囲に含めない。

## 依存境界

```text
固定 RenderSnapshot + 音声配置
  → AvExportSnapshot（owned / hash / schema 1）
  → kronello-media: 素材解決・hash 検証・FFmpeg decode / swresample
  → kronello-audio: absolute sample grid / Gain / Bus / PCM24 quantization
  → kronello-render: render_frame + 呼出側の RenderBackend
  → kronello-media: ProRes / PCM24 encode / MOV mux / probe / publication
```

`kronello-audio` は model / time の意味的な型と serde だけを利用する純粋層で、workspace の unsafe 禁止を継承する。具象 FFmpeg resource は media の C shim と `ffi.rs` だけに閉じる。render / model / time / service の API と RenderSnapshot schema は変更しない。

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

現行 Project に timeline audio placement の正本型がないため、`AvExportSnapshot` は純粋な明示入力 envelope として固定する。独立した編集状態や暫定の unknown Project field は作らない。audio asset は owned RenderSnapshot の Project.assets からだけ引く。

RenderSnapshot の既存 hash は映像文書・素材 lock を識別する。export snapshot hash はさらに audio placement / stream / source_in / Gain と envelope schema を含む。clip gain だけ変えると export hash は変わり、RenderSnapshot の hash は変わらない。MOV の `kronello_render_snapshot_hash` / `kronello_export_snapshot_hash` と report の両 hash でこの境界を明示する。export request の range / fps / region / background / clipping policy は report に別途保存する。

最終映像は explicit background を使う SDR linear Rec.709 → BT.709 encoded RGB → RGBA8 → ProRes。音声は stereo f32 → 明示 clipping policy → PCM24。stage の audio / video の sample 数・duration を要求と照合し、mux 後も codec / start PTS / duration / metadata を確認する。最終ファイルは既存成果物を上書きせず同じ volume の stage から確定する。

保守的な memory budget と対応 codec / layout / timing の範囲は ADR-0049 に記載する。大型 export の streaming、hardware / GPU 常駐転送、cancel / resume、実時間 playback は別の契約で昇格する。検証は [AUDIO-000](../testing/audio-000.md) を参照。
