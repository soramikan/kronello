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
M3 の TEMPLATE-002 は hold / loop、variant、data、移行の差分・比較を下記の規約で追加した。
保護区間と minimum_middle を満たせない尺は TimeMap を構築せず `DURATION_TOO_SHORT`。

M2 の背景帯の `size` / `position` は text の `layout_bounds` に padding を加えた値を読む。
M3 の LAYOUT-001 は、この既定を維持して `TemplateBandBinding.bounds` に `ink` / `visual` の明示選択を追加した。
上位 compiler が組版後に外部 `LayoutValue` を供給し、評価 DAG は text の Property → LayoutValue → 背景帯 Property を宣言する。
`max_lines` 超過は `TEMPLATE_OVERFLOW` として最終レンダーを拒否する。
実装範囲・制約・受け入れ条件とテストの対応は [TEMPLATE-001 検証](../testing/template-001.md) を参照。


### LAYOUT-001 の帯 stage 選択

`constraints.bands[]` に `bounds: "layout" | "ink" | "visual"` を追加した。省略時は従来の wrap_width 基準。
`ink` は短文・空白を含めた字形の領域、`visual` は変換と blur / shadow を含む包含矩形に padding を加える。
空の ink / visual は変換した text 原点に padding だけの帯を作り、layout box へ代替しない。
帯と text の同一親空間・Rectangle・position のみという既存の制約を維持する。
親変換が特異な visual 追従は `LAYOUT_SINGULAR_TRANSFORM`、text wrap と band size の静的循環は `PROPERTY_DEPENDENCY_CYCLE`。
幅 overflow は `LAYOUT_OVERFLOW`、max_lines は従来の `TEMPLATE_OVERFLOW`。組版結果を clip / 縮小して最終出力を続けない。
stage 選択も不変の template edition に保存され、既存 instance を暗黙に変更しない。

比較 UI は `scene.query.evaluation: {time, fonts}` の active node の `evaluated.bounds` を使う。
三段階を同時に root Composition の `design_px` で返し、既存の text-local `layout_bounds` も維持する。
responsive variant と比較・移行操作は下記の TEMPLATE-002 規約を使う。
詳細は [ADR-0057](../adr/0057-layout-bounds-stages.md) と [検証記録](../testing/layout-001.md) を参照。

帯の対象 text は leaf node に限る。子の合成結果を組版時の字形 bounds へ混ぜず、子を持つ対象は `UNSUPPORTED_FEATURE` で拒否する。帯の対象でない text の子は通常の scene 合成と bounds 集約で扱う。


### TEMPLATE-002 の実装規約（M3）

[ADR-0059](../adr/0059-template-duration-variants-and-migration.md) と
[検証記録](../testing/template-002.md) を正本とする。

尺の middle_mode は hold / loop / stretch。intro / outro は unit speed のまま保持する。
hold は中間全体を authoring の intro 時刻に固定し、outro 開始で明示的に切り替える。
loop は authoring の中間長を周期とする有理数の剰余、端数周期を stretch しない。
stretch は従来の PiecewiseLinear と保存 JSON を維持する。
総尺が intro + outro + minimum_middle 未満、または中間が空なら DURATION_TOO_SHORT。
hold / loop の保存は TimeMap::Protected、評価は checked 演算と floor の純粋関数。

TemplateDefinition.variants は名前ごとの {composition_ref,targets,constraints,content_hash}。
TemplateInstance.variant 省略は従来の base、指定はその variant の Composition を選ぶ。
寸法・authoring 尺は Composition に従い、出力画素数から自動切替しない。
公開入力の型・default は edition 共通、内部 targets と bounds / max_lines は variant ごとに固定する。
define 時にすべての到達内容を hash 固定し、opaque に隠す import も拒否する。

DataTable は Value::DataTable({columns,rows})。列型は String / Scalar / Color / Bool、
各行は同じ列名の型付き Value。128 列・10,000 行・100,000 cells 以内。
TemplateInputTarget::DataTable の明示した row / column を Text / Property に projection する。
上書きは default と同じ列 schema に限定し、欠落・余分な cell・型違いを INVALID_DATA_TABLE。
内部 descriptor・単位・範囲と Text 制約、公開名 policy を再利用する。

MediaSlot は Value::AssetRef と TemplateInputTarget::MediaSlot {node}、対象は明示した Null slot。
既知 Project.assets の参照を検証し、欠落は ASSET_MISSING。
preview / diff は binding と asset ID を返すが、Composition Media-node 描画は後続タスク。
active slot の final render は UNSUPPORTED_FEATURE で拒否する。空画像の成功扱いはしない。
全入力は default を持ち、空の optional MediaSlot は今回追加しない。

template.preview は proposed instance の寸法・public_inputs と bindings・local_time・MediaSlot、
renderer と同じ evaluated values と三段階 bounds を返す read-only query。
任意 region を指定すると選択 backend の FrameResult も返す。
region 省略は backend を初期化せず、診断は diagnostic、frame 生成失敗は成功描画と扱わない。

template.migration_plan は版 / variant / 入力の changes、EditPlan、before / after preview を返す。
inputs 省略時は旧上書きをすべて保持し、受け付けない入力の破棄は inputs で明示解決する。
異なる template_id は TEMPLATE_MIGRATION_INCOMPATIBLE。
公開・比較・計画では既存 instance を更新せず、edit.apply に commands / plan_hash を渡して適用する。
TemplateCommand::Migrate は指定 instance の pin と placement を変更し、revision・再送・Undo を共有する。
import は pin を変えられない。overflow は preview diagnostic と final render の型付きエラーを共有する。
