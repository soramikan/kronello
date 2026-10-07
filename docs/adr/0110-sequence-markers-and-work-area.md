# ADR-0110: シーケンス/クリップマーカーと In/Out・ワークエリア

- 状態: 採用
- 日付: 2026-10-07
- 関連: NLE-004、ADR-0010、ADR-0038

## 背景

タイムライン編集に必須のマーカー・In/Out 点・ワークエリア
書き出しが、モデルにも API にも存在しない。GUI のみの
選択状態にすると保存・共有・CLI/MCP との一貫性が失われる
ため、`Sequence` に永続化する。

検討した代替は次の 2 つである。

1. GUI 層の表示状態としてのみ持つ（文書外）。
2. `Sequence` のモデル要素として持ち、共有 Command/Query API
   と Undo の対象にする。

不変条件「入口専用の作品状態を作らない」および
NLE-004 の受け入れ条件（Undo・共有編集・保存で保持）に
反するため 2 を採用する。

## 決定

### マーカー

- `Sequence.markers: Vec<SequenceMarker>` と
  `Clip.markers: Vec<ClipMarker>` を追加する。
  いずれも `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
  で後方互換とする。
- `SequenceMarker` / `ClipMarker` の shape は共通で
  `Marker { id: MarkerId, time: Time, color: MarkerColor,
  comment: Option<String> }`。
  - `MarkerColor` は `red` / `green` / `blue` / `yellow` /
    `purple` / `cyan` / `orange` / `white` の閉集合。
  - `time` は sequence 時間（clip marker は clip の
    timeline ローカルではなく sequence 時間とし、
    clip の `timeline_range` 内に限定する）。
- 操作は `TimelineCommand` に
  `marker_set`（追加・置換）/ `marker_remove` /
  `marker_move` を追加し、GUI/CLI/MCP から共通に実行する。
  Undo は既存の ChangedKeys 機構で戻る。

### In/Out とワークエリア

- `Sequence.work_area: Option<TimeRange>` を追加する
  （serde default）。「In 点」と「Out 点」は
  work_area の `start` / `end` として表現し、
  独立したフィールドは持たない。
  - In のみ・Out のみの設定は `work_area` の
    半区間として `Option<TimeRange>` で表し、
    未設定は `None`。
- 操作は `TimelineCommand` に `work_area_set`
  （`Option<TimeRange>` を設定/解除）を追加する。
- 書き出し経路: `SequenceRenderRequest.range` は
  既存のままとし、work_area は **呼出し側が range に写像**
  する責務とする。`render.export` / `render.submit` の
  `range` 省略時は sequence 全体、work_area 指定時は
  GUI/CLI が `range = work_area` を明示して送る。
  service 層が sequence の work_area を暗黙に読むことは
  しない（範囲解釈の重複を避けるため）。
- `work_area` は空区間を許さない（validate で
  `INVALID_CLIP` 相当の型付き拒否）。

## 影響

- `Sequence`・`Clip` の serde default 追加で旧文書は
  そのまま開ける。
- `sequence.query` の結果に markers / work_area が
  反映されるため、Query 応答の schema が拡張される。
- マーカー ID は `MarkerId` の新しい ID 型を使い、
  配列 index や表示名から導出しない。
- スナップ対象（クリップ端・再生ヘッド・マーカー）は
  GUI 層の責務で、モデルには含めない。

## 関連

- ADR-0010（Command/Query API）、ADR-0038（序列）、
  [01-data-model](../architecture/01-data-model.md)
