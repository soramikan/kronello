# ADR-0127: マルチカム編集モデル

- 状態: 採用
- 日付: 2026-10-08

## 背景

NLE-007 はマルチカムクリップの同期・角度切替編集をモデルと
共有 API・GUI に要求する。SourceRef の拡張・同期情報・
角度切替の編集意味を決める。

## 決定

- `Project.multicams: Vec<MulticamAsset>` を追加する。
  `MulticamAsset` は `id`・`name`・`angles: Vec<MulticamAngle>`
  を持ち、`MulticamAngle` は `asset: AssetId`・`stream_index`・
  `sync_offset`（有理数）・`name` を持つ。ID は既存の
  UUID 規則に従い、配列番号・表示名から導出しない。
- `SourceRef::Multicam { multicam: MulticamId, angle: AngleId }`
  を追加し、クリップは常に 1 つの有効 angle を参照する。
  角度切替は `clip.angle_switch` 共有編集で `angle` を更新する
  （後続クリップへの伝播はしない）。
- 同期は `multicam.create` 時に `sync: timecode | audio |
  manual` を指定する。`audio` は角度間音声相関で offset を
  決定的に推定し、推定不能は `MULTICAM_SYNC_FAILED` の
  型付きエラー。`manual` は明示 offset を要求する。
- GUI ではマルチカムクリップの角度選択 UI と、再生中の
  角度切替を接続する。モニタ表示は通常 preview 経路と同じ
  固定 snapshot を使う。
