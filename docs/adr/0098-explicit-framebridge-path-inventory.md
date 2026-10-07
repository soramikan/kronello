# ADR-0098: FrameBridge の具体経路一覧と generic VideoToolbox 拒否

- 状態: 採用
- 日付: 2026-10-06
- 対象: FRAMEBRIDGE-001

## 背景

`PathKind::VideoToolbox` は M0 の未実装 selector であり、本番 decode は
`VideoToolboxDecodeBgra8` / `VideoToolboxDecodeNv12Biplanar` と resident backend に
具体化された。generic 側の「decode/encode unimplemented」「M0で未測定」という古い診断は、
具体的な実装が未実装であるようにも、汎用 codec path が別に存在するようにも読める。

## 決定

generic selector は互換性のため enum に残すが、常に型付き `UNSUPPORTED_FEATURE` で
拒否する tombstone とする。BGRA8 / NV12 への暗黙 alias、fallback、encode の入口にはしない。
`CONCRETE_PATHS` は具体的な transfer / decode probe selector だけを列挙し、generic を含めない。

`require_gpu_resident` と `SpikePath::measure` の generic 拒否診断は同じ文字列を使い、
具体的な二つの decode selector と、generic が encode を提供しないことを明示する。
BGRA8 selectorは新しいstrict probeを使い、BGRA8失敗をNV12成功へ置換しない。
既存の候補探索用 `probe_videotoolbox_decode(...,false)` は互換helperとして残すが、
explicit selectorから呼ばず、返すMeasurementの実経路を常に保持する。
IOSurface probe の診断は transfer の測定範囲に限定し、codec 全体の未測定とは呼ばない。

経路一覧と residency policy の成功は native runtime・codec・形式の利用可能性を証明しない。
実行時の hardware property・Metal device・format・所有権・同期の検証は
[ADR-0081](0081-guaranteed-metal-hardware-video-decode.md) の具体経路が担う。
同ADRの保証範囲、明示software選択、非対応host・形式の拒否は維持する。
FFmpeg の H.264/HEVC hardware encoder は別契約であり、このprobe一覧に含めない。

## 検証

[FRAMEBRIDGE-001](../testing/framebridge-001.md) に inventory・policy・診断・実native probe の
証拠を記録する。汎用decode/encode、HDR、10-bitや新しい形式の保証を追加しない。
