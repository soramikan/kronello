# ADR-0100: 編集コントロールと明示した逆方向 source sampling

- 状態: 採用
- 日付: 2026-10-06
- 対象: GUI-007
- 追加: ADR-0051 / ADR-0069 の既定の正方向 TimeMap・audio policy は変更しない。

## 決定

GUI の Effects、clip 時間、合成、track 出力、Text span は共通 Command / Query を使う。
`Track.state` は省略可能で、欠落時は visible=true / muted=false。visible は映像、muted は音声。
`TrackStateSet` は作品イベントである。`ClipTimeSet` は source_in、TimeMap、audio_retime、
reverse_sampling を原子的に置換し、候補全体の source bounds、link、transition、protected content
を検証する。clip properties / effects は既存 `ClipSetEffects` を使う。

guide、snapping 設定、viewer pan、明示 font file locator は UIState に保存する。
font file は read-only `font.pin` で検証・hash固定した FontRef を共有 TextSet に書く。URL取得はない。
書式範囲は UTF-8 byte の書記素境界に変換する。複数 style の本文変更は既存の書式と source association
を保存し、不整合な ruby / selector を黙って除去しない。独立した span 色・サイズには
`kronello.text.style_color` / `kronello.text.style_size` を使う。DescriptorDefinition.repeatable=true の
登録された descriptor だけは一つの node に複数 Property を持てる。識別は PropertyId で行い、
既存 singleton descriptor の重複拒否を維持する。

## 逆方向の時刻契約

TimeMap の正の slope 不変条件を維持する。負の Linear TimeMap は元々受理しておらず、
負の slope を新たに許す変更はしない。省略可能 `Clip.reverse_sampling=reverse_grid_v1` を
明示した clip は `source_in - TimeMap.map(elapsed)` を連続 source envelope とする。
通常は source_in を対象区間の上端へ置き、TimeMap は正の速度の大きさを保存する。
source_in・duration・速度は正規化された有理数のまま保持する。GUI の source-in 欄は実際に
保存した上端を表示し、同じ数値を保持したと主張しない。

- Composition は自身の edit_rate の直前の source cell を評価する。
  `position=source*edit_rate`、整数 endpoint は `position-1`、非整数は floor(position)。
  これは sampled reverse であり、連続した逆方向アニメーションの新定義ではない。
- Video は decoder が検証した実際の presentation interval `(pts,end]` を選ぶ。
  nominal frame rate、GOP近似、epsilon は使わない。CFR / VFR / B-frame の前後要求で同じ interval を返す。
  最終 frame に明示 duration がない等の不正な interval は型付き失敗。
- audio は明示 `audio_retime=reverse_resample_v1` を必要とする。48kHz source coordinate q の
  neighbors は ceil(q)-1 / ceil(q)-2、reverse fraction は ceil(q)-q。これは正方向 interpolation を
  q-1 に適用した式と等しい。fractional endpoint の underflow、不足する neighbor は失敗し、
  padding、clamp、無音で代替しない。pitch は speed に従う。

RenderSnapshot の optional semantic_versions.reverse_sampling=1 を固定する。旧 snapshot の欠落は
opt-in clip がない場合のみ旧意味として受理する。未知 pin と、新 policy を pin の欠落で実行する要求は拒否。
明示 policy と capability pin が新経路を区別するため、audio evaluator 2 の旧 policy は維持する。
Temporal sample は同じ source policy を各 rational sample time に純粋適用する。continuous reverse の
motion blur と同じ意味とは主張しない。GPU-resident decoder がこの selection を実装していない場合は
明示 software presentation decode を選ぶか型付き未対応とする。

## 検証状態

受け入れ結果は [GUI-007 検証記録](../testing/gui-007.md) に記載する。
この ADR の採用と部分実装は GUI-007 の完了を意味しない。

2026-10-06: GUI-007の全条件は直接GUI・worker・CLI/MCP同等性と統合checkpointで受け入れ済み（[受け入れ記録](../testing/gui-007.md)）。本ADRの設計決定は変更していない。
