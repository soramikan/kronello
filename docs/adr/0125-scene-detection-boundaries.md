# ADR-0125: シーン検出（カット境界の自動検出）

- 状態: 採用
- 日付: 2026-10-08

## 背景

AI-002 は映像のシーン境界自動検出と、結果のマーカー /
クリップ分割への適用を要求する。検出手法・結果の表現・
適用 API を決める。

## 決定

- 検出は `scene.detect` の固定入力 job とし、デコード済み
  フレーム列に対して輝度ヒストグラム差とエッジ変化量の
  決定的スコアを計算し、適応閾値でカット境界を出力する。
  乱数・外部プロセス・機械学習モデルを使わない。
- 結果は版付き `SceneBoundaryAsset`（boundary timecode と
  confidence の列）として `Project` に保持し、メディア
  asset・stream と同じ固定参照規則に従う。
- `scene.apply` は `SceneBoundaryAsset` を入力に、
  `mode: markers | split` で sequence marker 追加または
  対象クリップ分割を行う共有編集として実装する。Undo・
  revision・競合規則は既存 NLE 編集と同じ。
- 入力メディアの欠落・デコード失敗は既存の
  `ASSET_MISSING` / `MEDIA_DECODE_FAILED` 系の型付き
  エラーで返す。
