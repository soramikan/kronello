# 07 テンプレート

## 公開入力

公開入力は Text、Color、Number、Enum、MediaSlot、DataTable 等の型とする。

- 公開していない内部 Property をバッチ操作で勝手に書き換えない。上級編集の操作は別権限・別意図として扱う。
- 入力から内部 Property へ bind し、template definition と instance inputs は別保存にする（[ADR-0007](../adr/0007-template-definition-vs-instance-inputs.md)）。
- テンプレート更新はバージョンを固定し、移行計画とプレビュー比較を生成する。既存作品を自動更新しない。

## 尺の伸縮

例: Authoring duration 5 秒、intro 0.4 秒、outro 0.3 秒。

- 8 秒に変更すると、intro / outro は 0.4 / 0.3 秒を保持し、中間を 7.3 秒にする。
- 中間が静止なら hold、動く背景なら loop または stretch を明示指定する。
- 総尺が保護区間と最低 hold 時間の合計を下回れば `DURATION_TOO_SHORT` にする。

## 定義の例（提案スキーマ）

```json
{
  "template_id": "lower_third_ja",
  "version": "1.0.0",
  "public_inputs": {
    "headline": {"type":"text", "required":true},
    "subtitle": {"type":"text", "default":""},
    "accent": {"type":"color", "default":"#F59E0B"},
    "logo": {"type":"media_slot", "optional":true}
  },
  "duration_policy": {
    "intro": {"num":"2", "den":"5"},
    "outro": {"num":"3", "den":"10"},
    "minimum_hold": {"num":"1", "den":"2"},
    "middle_mode": "hold"
  },
  "layout_policy": {
    "max_lines": 2,
    "overflow": "error",
    "variants": ["landscape", "portrait"]
  }
}
```

上記は提案スキーマの例であり、稼働中製品の設定形式ではない。
色空間の指定がない色入力（`#F59E0B` など）は sRGB として解釈し、保存時に `{space, components}` の明示表現へ正規化する（[ADR-0024](../adr/0024-working-color-space.md)）。

色入力は straight RGB と独立 alpha（省略時 1）で、作業空間の値と取り違えない（[ADR-0044](../adr/0044-color-and-alpha-contracts.md)）。Number 入力の単位・範囲は bind 先の Property と整合させ、秒・設計単位・度・倍率を暗黙に混用しない（[ADR-0043](../adr/0043-semantic-dependencies-and-units.md)）。

## 実装段階

| 段階 | タスク | 内容 |
|---|---|---|
| M2 | TEMPLATE-001 | 公開入力、定義と instance 入力の分離、版固定、保護時間区間、背景帯の単方向追従と overflow 検出 |
| M3 | TEMPLATE-002 | 短尺拒否 / hold / loop / stretch、縦横比 variant、data 入力、版移行の差分計画 |
