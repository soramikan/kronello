# ADR-0059: テンプレートの保護尺・variant・data と明示した版移行を共有する

- 状態: 採用
- 日付: 2026-10-04
- 対象: TEMPLATE-002

## 背景

ADR-0007 の版固定、ADR-0043 の純粋評価と有理数、ADR-0053 の既定 wrap_width、
ADR-0057 の三段階 bounds を維持し、M3 の尺・縦横比・data・比較と版移行を追加する。
既存 ADR を置換しない。Composition の Media node 描画は実装されていない。

## 決定

### 尺

`TemplateMiddleMode` は `hold` / `loop` / `stretch`。
authoring と requested の両方で intro + outro + minimum_middle を満たし、
中間が正であることを確認し、満たさなければ `DURATION_TOO_SHORT`。
intro は [0,intro)、中間は [intro,requested-outro)、outro は
[requested-outro,requested)。境界を含む TimeMap の評価域は [0,requested]、
配置の active_range は従来どおり [0,requested)。

- hold: 中間全体を authoring の intro 時刻の静止 pose に写す。outro 開始で authoring-outro へ明示的に切り替える。
- loop: 中間の elapsed を authoring-intro-outro を周期とする有理数の剰余へ写す。周期端は intro に戻り、outro 開始は authoring-outro を優先する。端数周期を stretch しない。
- stretch: 従来の正傾斜 PiecewiseLinear を保持する。既存保存 JSON と意味を変えない。

hold / loop は `TimeMap::Protected`（wire `kind: "protected"`）に
authoring / requested / intro / outro / mode を保存する。
floor と checked 演算だけで評価し、時計、前回値、frame rate、浮動小数点時刻を使わない。
Linear / PiecewiseLinear の正傾斜制約は緩めない。
汎用 Sequence Clip の Protected map の trim / stretch / lowering は引き続き型付き未対応。
template placement の retime は選んだ variant の authoring 尺で map を再構築する。

### 明示した縦横比 variant

`TemplateDefinition.variants` は名前から `TemplateVariant` への map。
variant は composition_ref、全公開入力の targets、constraints、content_hash を持つ。
design_extent と authoring duration は参照 Composition の正本を使う。
`TemplateInstance.variant` を省略すると従来の base Composition、指定するとその名前を選ぶ。
出力画素数や縦横比から variant を自動選択しない。
入力の型・既定値・公開範囲は edition 共通、内部 target と帯・max_lines は variant ごとに固定する。
公開入力すべてを同じ名前で束縛し、欠落・余分な束縛・重複 target を拒否する。

define 時に base と全 variant の到達内容の hash を固定する。
既存 edition の内容変更・opaque に隠す import を拒否する。新規内容には新規 ID を使う。
variant を選ぶ変更も既存 pin の変更であり、後述の明示した migration と Undo を通す。
保護した NLE retime の検出も variant の Composition を含む。
variants と variant の省略は旧 JSON の往復を維持する。

### DataTable / MediaSlot

`ValueType::DataTable` / `Value::DataTable` は inline の `{columns,rows}`。
columns は名前と ValueType、rows は同じ key 集合の型付き Value の map。
String / Scalar / Color / Bool の列を受け付け、nested table、URL fetch、実行コードを受け付けない。
128 列・10,000 行・100,000 cells の上限を持ち、上書きの columns は edition の default と完全一致させる。
`TemplateInputTarget::DataTable` は明示した row / column から Text / Property への bindings を持つ。
行不足、余分な cell、型違い、schema の変更は `INVALID_DATA_TABLE`。
Property の descriptor・単位・範囲と既存の単一 style Text 制約を再利用する。
table はバッチ内でも公開名だけで変更し、内部 target を指定して公開 policy を回避できない。

MediaSlot は `Value::AssetRef` と `TemplateInputTarget::MediaSlot {node}`。
明示した Null node を slot とし、Project.assets の既知 asset の存在を検証する。
欠落は `ASSET_MISSING`。preview / diff に asset ID と binding を返す。
Composition に Media 描画の実装がないため、active slot の最終 compiler は
`UNSUPPORTED_FEATURE` で拒否し、空の成功画像を返さない。
この境界は supervisor が承認した範囲である。**Composition Media-node 描画を後続タスクにする**。
slot の保存・束縛の確認を media 画素の描画成功と扱わない。

すべての入力は default を持つ。呼び出し元が値を省略しても default を使用できる。
optional MediaSlot の空値や外部 DataAsset の取込みは今回追加しない。

### 比較 preview と版移行

`template.preview` は一つの保存 revision の owned snapshot から proposed instance を隔離して比較する read-only query。
edition・選択した bindings、resolved_inputs、寸法、local_time、MediaSlot、node key、
renderer と同じ evaluated properties / text / world_transform / effects / 三段階 bounds を返す。
明示した font lock / local path を共有し、GPU を初期化せずに意味的 preview を取得できる。
任意の region を指定すると選択 backend で FrameResult も返す。CLI の CPU は
`--backend cpu-reference` の明示選択であり、GPU 不在の代替にはしない。
overflow・font・media・backend の失敗は diagnostic に返し、frame は生成しない。
成功 envelope の diagnostic は描画成功ではない。time は [0,instance.duration) に限定する。

`template.migration_plan` は base_revision、instance ID、次の edition ID、variant、
任意の inputs、比較 time / fonts / region を取り、
`{plan,changes,before,after}` を返す。
plan は既存の EditPlan、changes は JSON pointer の field と before / after の値、
before / after は同じ source revision と font lock の preview。
異なる template_id への移行は `TEMPLATE_MIGRATION_INCOMPATIBLE`。
inputs 省略時は旧上書きをすべて保持し、新版が受け付けない入力は失敗する。
削除や table schema の変更に伴う上書きの破棄は inputs を明示して解決する。
自動変換・勝手な入力削除をしない。

新版の define、preview、migration_plan は既存作品を更新しない。
適用は `edit.apply` に返された commands と plan_hash を渡す。
`TemplateCommand::Migrate` が指定した instance の edition / variant / placement map だけを変更し、
通常の revision・idempotency・Undo を利用する。import は pin を変更できない。
plan の診断を確認してから適用する責任は caller にある。意味的に有効な候補の overflow を
保存編集で禁止する規約へは変えず、最終レンダーは同じ型付き失敗を返す。

### Bounds

帯の `bounds` は ADR-0057 をそのまま使い、既定 layout と明示した ink / visual を区別する。
variant の table から供給された文字も同じ組版・依存 DAG・padding を使う。
空白・空本文の ink は空、帯は変換した原点の padding だけになる。
`LAYOUT_OVERFLOW` / `TEMPLATE_OVERFLOW` / `PROPERTY_DEPENDENCY_CYCLE` /
`LAYOUT_SINGULAR_TRANSFORM` を維持し、wrap_width の既定を変えない。

## 検証と関連

[検証記録](../testing/template-002.md) に各条件と実行結果を対応付ける。
公開 Project / API schema は Rust の生成器を使う。CLI / MCP は同じ registry と strict wire を通す。
workspace の GPU / FrameBridge を含む検証は supervisor の host run を待つ。

- [07 テンプレート](../architecture/07-templates.md)
- [08 API](../architecture/08-api-cli-mcp.md)
- [ADR-0007](0007-template-definition-vs-instance-inputs.md)、[ADR-0057](0057-layout-bounds-stages.md)
