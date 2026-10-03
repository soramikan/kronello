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

### TEMPLATE-001 の実装規約（M2）

`Project.templates` の `TemplateDefinition` は版ごとに不変の ID と version、composition_ref、型付き public_inputs（default / 数値範囲 / Enum choices / target）、duration_policy、constraints、到達内容の content_hash を保持する。
`Project.template_instances` は placement ID、定義 ID、固定 version、duration、入力上書きを別保存する。
`template.define` と `template.set_input` は別の共通 service command。`template.instantiate` で配置し、`template.set_duration` で尺を変更する。
同じ版の再公開、暗黙の版移行、固定した authoring 内容の編集は拒否する。新版の公開入力変更は既存 instance へ伝播しない。
到達内容の hash には入れ子の template の版・既定値・instance 入力も含め、内側の変更で外側の固定版が暗黙に変わることを防ぐ。
未知の定義・instance は `DocumentObject::Opaque` として create / import / export で保存する。保存できることと編集・実行できることを区別し、最終レンダーでは選択 Composition から到達する template だけを検証する。
既存の公開版の内容を未知フィールドで opaque に変える import は、版固定を回避する変更として拒否する。

M2 は有理数の `PiecewiseLinear` TimeMap により中間を stretch し、intro / outro の長さを保持する。
hold / loop、variant、data、移行の差分・比較は M3 の TEMPLATE-002 に残す。
保護区間と minimum_middle を満たせない尺は TimeMap を構築せず `DURATION_TOO_SHORT`。

背景帯の `size` / `position` は text の `layout_bounds` に padding を加えた値だけを読む。
上位 compiler が組版後に外部 `LayoutValue` を供給し、評価 DAG は text の Property → LayoutValue → 背景帯 Property を宣言する。
`max_lines` 超過は `TEMPLATE_OVERFLOW` として最終レンダーを拒否する。
実装範囲・制約・受け入れ条件とテストの対応は [TEMPLATE-001 検証](../testing/template-001.md) を参照。
