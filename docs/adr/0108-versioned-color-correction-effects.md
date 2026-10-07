# ADR-0108: 版付き色補正エフェクト（exposure / levels / curves / HSL）

- 状態: 採用
- 日付: 2026-10-07
- 関連: COLOR-002、COLOR-001、FX-001、ADR-0101、ADR-0066

## 背景

`EffectParameters` は `gaussian_blur` / `drop_shadow` /
`audio_gain` の 3 種のみで、色補正の手段が存在しない。
COLOR-002 では exposure・levels・curves・HSL 補正を
versioned effect として追加する。

検討した代替は次の 2 つである。

1. 補正群をまとめた単一の `color_correction` エフェクトにする。
2. 補正ごとに個別の版付きエフェクトにする。

1 は Inspector での並び替え・個別の有効化・Undo の粒度が
悪化し、1 補正の追加が全体の版を上げる。既存の
`gaussian_blur` / `drop_shadow` と同じく「1 機能 = 1 版付き
effect_id」の規約に揃えるため 2 を採用する。

## 決定

`EffectParameters` に次の 4 バリアントを追加する。
effect_id は `kronello.color.*` 名前空間、`version` は
各 v1 から開始する。パラメータは `clip.properties` 上の
`Property` を `PropertyId` で参照する既存規約に従う。

- `kronello.color.exposure` v1:
  `exposure`（EV 単位の scalar、scene-linear 係数 `2^exposure`
  を RGB に掛ける）、`offset`（加算、既定 0）。
  alpha は変更しない。
- `kronello.color.levels` v1:
  `in_black`・`in_white`・`gamma`・`out_black`・`out_white`
  （いずれも scalar）。scene-linear 値に対し
  `out = out_black + (in - in_black)/(in_white - in_black) ^ (1/gamma)
  * (out_white - out_black)` を適用する。
  `in_white <= in_black` は検証で型付き拒否。
- `kronello.color.curves` v1:
  `curve`（`ValueType::DataTable`、columns は
  `x: scalar`・`y: scalar`、0.0–1.0 の区間で単調増加、
  最大 64 点）。補間は単調 cubic（Monotone cubic / Fritsch–Carlson）。
  入力を RGB 各成分へ共通に適用する v1 とし、
  チャンネル別 curve は v2 以降とする。
- `kronello.color.hsl` v1:
  `hue_shift`（degrees）、`saturation`（係数、1 が無変換）、
  `lightness`（加算）。変換は表示域の HSL ではなく
  scene-linear 値を一度 0–1 に正規化した working HSL で行い、
  結果を線形へ戻す（負値・HDR 値は正規化時に保持し、
  clamp しない。詳細は後述）。

## 線形空間と HDR/SDR の規約

- 全補正は sequence の working space（`LinearRec709` /
  `LinearRec2020`）の scene-linear premultiplied 値に対して
  行う。`premultiply` の alpha は各補正で保持し、RGB のみを変換する。
- 1.0 を超える HDR 値・負値を clamp しない。`levels` の
  `in_black..in_white` 外は外挿、`curves` の 0–1 外は端点の
  線形延長で処理する。
- CPU（`kronello-render`）と GPU（`kronello-gpu` / WGSL）は
  同一の式を実装し、意味的一致をテストで確認する。
- LUT（COLOR-003）・スコープ（COLOR-004）は本 ADR の範囲外。

## 影響

- `EffectParameters`・`ResolvedEffect`・`PixelEffect` に
  バリアントが追加される。`clip.effects` の 16 件 budget は変更しない。
- CPU: `kronello-render` の pixel effect 経路、
  GPU: `kronello-gpu` の WGSL effect 関数と `scene_gpu` の
  割当に追加が必要。golden 比較シーンを追加する。
- 未知の effect_id / 版は既存の `Effect::Opaque` 保持と
  `UnsupportedMeaning` で保護される規約を維持する。

## 関連

- ADR-0101（線形 premultiplied 合成）、ADR-0066（エフェクト版管理）、
  [01-data-model](../architecture/01-data-model.md)
