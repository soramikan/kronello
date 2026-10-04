# ADR-0058: 正規 postorder AST と有界 Expression 評価

- 状態: 採用
- 日付: 2026-10-04

## 背景

ADR-0003 / 0004 / 0009 / 0040 の純粋評価、排他的な主値源、禁止能力、AST 正本を EXPR-001 で具体化する。OQ-17 の人間向け構文は未決のまま残す。本 ADR は既存 ADR を置換しない。

## 決定

- `kronello-model::Expression` は `id`、意味の `version=1`、`value_type`、`budget`、`nodes` を持つ。Project の省略可能な `expressions` 集合に `DocumentObject<Expression>` として保存する。主値源は既存 `PropertySource::Expression(ExpressionId)` を使う。
- `nodes` は子を左から順に並べた postorder の木で、最後が root。operand は先行 node の `u32` index。各部分木は連続した範囲であり、共有・未使用 node・forward reference を拒否する。index は AST 内の構造位置だけを示し、作品の ID には使わない。演算と operand 順序を保存するため、将来の表示言語はこの木へ一意に往復できる。parser / pretty-printer / surface syntax は今回定義しない。
- root と各演算の型を静的検証する。Scalar の加減乗除、clamp、lerp、sin（radian）、Vec2 / Vec3 / Angle（degrees）の明示構築、既存 Value の literal、ローカル秒 Time、固定 seed noise、同一 authored scope の Property、明示した Curve の有理数 offset sample を提供する。汎用関数呼び出し、動的 Property sample、Property の過去値、DataAsset 読み取りは提供しない。
- Property 参照は NodeId（null は Composition input）/ PropertyId で静的に列挙する。参照の宣言 ValueType を descriptor と照合し、参照先と消費 Property の unit / coordinate space を一致させる。異なる単位・空間の暗黙変換を認めない。Curve 参照も ID / ValueType を静的に照合する。CurveSample の色変換は消費 descriptor の指定色空間、指定がなければ graph の作業空間を使う。
- 定義内の Time / CurveSample / Property は配置のローカル scope、placement input binding の式は親の authored scope を読む。noise の配置識別は消費先の InstancePath を使う。noise は seed / element / 明示 Scalar input の IEEE bits（signed zero を正規化）/ 配置 UUID の順序付き byte 列を FNV-1a と固定整数 mixer で写し、`[-1,1]` を返す。外部乱数と時計は使わず、連続補間 noise は今回提供しない。
- `Expression::dependencies()` は Property / Curve の完全な静的集合を返す。Property 辺を既存 `DependencyGraph` に追加し、欠落・型不一致・閉じた循環経路を評価前に診断する。evaluation は model / time / animation にだけ依存する。

## 予算

| 資源 | 既定かつ上限 |
|---|---:|
| AST nodes | 1024 |
| 静的な参照先 | 64 |
| 命令 | 4096 |
| 評価の一時メモリ | 1048576 bytes |
| sample 要求 | 64 |

文書の budget はこの上限を下げることだけを許す。AST の node / payload / 型検証作業を静的に見積もり、実行では命令と owned payload の clone 前に課金する。CurveSample は key 数 + 65 命令を課金し、既存 cubic の固定 64 回反転と key 検査を含める。noise は固定 byte と配置 UUID byte 数、Property 参照は scope 深さも課金する。Property 読み取りと Curve sample は sample 要求として課金し、同じ参照の繰り返しも数える。

一つの要求 Property の依存 closure 全体にも既定上限を適用する。上流値の payload と memo entry を含む保守的な累積量を数え、transitive な式による予算回避を防ぐ。各式自身の実行はその式のより小さい budget も守る。複数 Property の batch は各 root を独立に評価するため、要求順、batch 分割、render cache の状態で予算判定は変わらない。snapshot / JSON 入力 / graph 自体の保持量や renderer の画素予算は、この式実行予算と別の境界である。

## 共有 API と失敗

- `edit.plan/apply` の既存 commands に `expression_set: {expression: Expression}` を追加する。同じ batch の `property_source_set` で参照を設定できる。候補の AST・source 型・全配置の依存 DAG を検証し、無効な候補を保存しない。
- Expression 更新はその ID と直接消費者の Property key を記録する。主値源設定・node/instance の直接参照は Expression ID も記録する。依存先 Property の値変更による間接影響は Undo 競合キーに広げない。Undo の候補も通常の graph 検証を通す。
- `property.sample` と最終 render は同じ `DependencyGraph` の evaluator を使う。失敗時に定数へ戻さない。render 前の scene 構築が失敗すれば backend と出力公開へ進まない。
- `EVALUATION_ERROR` は型・参照・算術失敗、`EXPRESSION_BUDGET_EXCEEDED` は予算超過、`PROPERTY_DEPENDENCY_CYCLE` は閉じた実行時キー経路、`UNSUPPORTED_FEATURE` は未知意味版 / opaque 内容。編集対象や source catalog の照合には既存の `INVALID_EDIT` も使う。禁止能力を要求する未知 variant や追加 field は Command decode の `INVALID_REQUEST`。Project import/export では object 全体を opaque として保持し、実行しない。
- RenderSnapshot の `semantic_versions.expression` を固定する。既存 snapshot でこの field がない場合は、式を実行できなかった時代の互換入力として version 1 を補う。式本体は Project の固定コピーと identity hash に含み、worker は現在の project を読み直さない。

## 検証と残件

[EXPR-001 の検証](../testing/expr-001.md) に通常テスト、実 CLI / MCP、sandbox と host の境界を記録する。DataAsset、動的な過去 Property sample、汎用 vector arithmetic、連続 noise、人間向け構文は後続設計の対象。OQ-17 は未解決。
