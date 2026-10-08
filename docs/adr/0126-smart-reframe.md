# ADR-0126: スマートリフレーム（被写体追従の自動縦横比変換）

- 状態: 採用
- 日付: 2026-10-08

## 背景

AI-003 は被写体追従による自動リフレームと responsive
layout（LAYOUT-001）との組み合わせを要求する。
注視点の決め方・crop の表現・レイアウト合成を決める。

## 決定

- 注視点は `TrackingDataAsset` の tracked point 群の
  信頼度加重重心とし、時間方向に平滑化する。seed の
  選択・平滑窓はリフレーム設定の一部として版付きで保持する。
- crop は目的 aspect の矩形 window を注視点中心に配置し、
  ソース境界をはみ出す場合は window を内側へクランプする。
  `padding`・`max_zoom`・`easing` は設定パラメータ。
- LAYOUT-001 の layout rule へ `smart_reframe` 種を追加し、
  responsive layout の評価経路で他ルールと同じ優先順位・
  bounds 規則に従う。GUI では crop window のプレビューと
  パラメータ調整を提供する。
- tracked point が全フレームで失われた場合は
  `TRACKING_INSUFFICIENT` の型付きエラーとし、静止中央
  crop への黙った fallback はしない。
