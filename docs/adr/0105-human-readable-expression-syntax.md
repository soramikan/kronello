# ADR-0105: 人間向け式構文とAST往復

- 状態: 採用
- 日付: 2026-10-06
- 関連: EXPR-002、ADR-0040、ADR-0058、ADR-0102

## 背景

GUI / CLI / MCP が式を人間向けテキストとして編集・表示する構文が未決だった
（OQ-17）。正本の postorder AST・有界評価・禁止能力は変えず、式テキストは
ASTの表示・編集表現に限定する必要がある。

## 決定

[m5-expression-syntax-proposal.md](../roadmap/m5-expression-syntax-proposal.md)
をそのまま採用する。決定事項は次の通り。

- 入力は式一つだけとする。文・代入・変数定義・ループ・JavaScript互換・
  任意関数呼び出しを持たない。空白は意味を持たず、文字列はJSON形式の
  引用とescapeを使う。
- 数値literalは有限の符号付き十進数（指数表記可）とする。符号を持てるのは
  literalだけで、非literalの負値は `0 - x` と記す。演算子は `+ - * /` のみで、
  通常の乗除優先・左結合・括弧を持つ。演算順序を変える最適化を
  parser / formatter で行わない。
- 関数表は `time` `sin` `clamp` `lerp` `vec2` `vec3` `angle` `noise`
  `continuous_noise` `property` `curve` `literal` `property_sample`
  `data_cell` `audio_feature` `audio_band` `time_offset` に固定する。
  arity は表の通りとし、未知関数・未知型・動的な参照文字列を拒否する。
  `time_offset` は単独の式nodeではなく、対応する関数の固定有理数引数だけに
  許す。分子・分母は i64 範囲の十進文字列として読み、共有 `Time` で正規化し、
  ゼロ分母や正規化時の overflow は拒否する。
- `seed` / `element` / `band` の u32 固定引数は十進数字のみで
  0〜4294967295 とし、符号・小数点・指数表記を許さない。u32 を f64 経由で
  保存しない。`row` / `lookback` は Scalar 子式で、row の整数性・範囲と
  lookback の非負秒数は既存評価器が検証する。lookback は共有評価器と同じく
  root timeline 上で 1ns へ量子化してから配置時間へ写像する。
- 参照は静的な UUID 文字列と公開 `ValueType` の snake_case 型名に限定する。
  `property` / `property_sample` の node 引数は UUID 文字列または Composition
  input を示す `null` とする。表示名や動的な任意文字列を参照先として評価しない。
- Bool は `true` / `false`、String は JSON 文字列を簡便 literal とする。
  任意の Value は `literal("...")` に共有 Value の serde 表現を escape した
  JSON 文字列で埋め込み、未知 field / type や非有限値を拒否する。
  `vec2(...)` 等の構築関数は常に構築 node、`Value::Vec2` 等の定数 literal は
  常に `literal(...)` とし、値が同じでも異なる AST を混同しない。
- 式の ID・出力型・budget・意味版は編集対象の metadata として編集境界から渡し、
  テキストの再解析で勝手に生成し直さない。
- 構文処理の上限は UTF-8 入力 64KiB、token 数 8192、構文の入れ子深さ 64 とする。
  上限を超えた入力は AST 構築前または構築中に診断を返す。構文の上限が既存の
  Expression nodes / dependencies / memory / instructions / samples 予算を
  拡張しない。
- formatter は AST の左右の部分木・postorder 順・literal の型を保持する。
  演算子の順序変更・結合変更・定数畳み込み・構築 node と型付き literal の
  置換をしない。元 AST が対応する意味版の範囲内であることを先に確認し、
  parse / format が意味版や budget を暗黙に上げない。
- 構文上限内の正規 AST に対し `parse(format(ast), metadata) == ast` を維持する。
  formatter 出力が構文上限を超える AST は型付き診断を返し、元 AST を保持する。
- 構文診断は byte 範囲・行・列・期待 token を持ち、GUI と API で共有する。
  構文不正時は既存値を保持する。日本語入力中の未確定文字列は作品へ適用しない。
- 適用は共有の edit.plan / edit.apply の `property_expression_text_set`
  コマンドとして行い、revision 検証・idempotent 再試行・選択 Undo・固定
  snapshot を既存と同じく維持する。読み取り側は `expression.format` で
  保存済みまたは与えた AST の正規テキストを返す。

## 影響

- `kronello-model` に `expression_syntax` module を追加し、parse / format と
  型付き診断を提供する。新しい外部 crate は追加しない。
- `EditCommand::PropertyExpressionTextSet` と `expression.format` が公開 API に
  追加され、CLI / MCP / GUI が同じ Command / Query 経路を使う。
- 評価意味・評価器・snapshot 形式は変更しない。構文は既存 AST の表示であり、
  任意 I/O や再帰評価を解禁しない。

## 関連

- [ADR-0040](0040-expression-language-policy.md)
- [ADR-0058](0058-bounded-canonical-expression-ast.md)
- [ADR-0102](0102-bounded-temporal-expression-assets.md)
- [docs/architecture/03-property-animation.md](../architecture/03-property-animation.md)
- [docs/architecture/08-api-cli-mcp.md](../architecture/08-api-cli-mcp.md)
