# PR #20 レビュー修正の検証

2026-10-12、`has_unknown_content` の union 分岐を JSON の型で絞り、文字列リテラルの `const` を `enum` と同様に検査するよう修正した。object の分岐へ文字列を通さず、string の分岐へ object を通さない。未来の文字列 variant を保持し、数値などの誤った型や既知 variant の不正 payload は `INVALID_DOCUMENT_OBJECT` として拒否する。

macOS のローカル検証:

- `project_diagnostics`: 6 passed。未来の TrackKind、MarkerRole、TrackingMode、ExpressionNode の文字列/外部タグ形式を lossless に保持。TrackKind の数値、ExpressionNode の数値・既知 literal の不正 payload は拒否。
- model unit test の `string_literal_unions_distinguish_extensions_from_wrong_types`: passed。実際の生成 schema を使い TrackKind / TrackingMode / MarkerRole / ReframeEasing / ExpressionNode の未知・既知文字列と誤った数値を区別。
- store の `future_expression_union_variants_survive_store_and_history_reads`: passed。2種類の未来 ExpressionNode を import し、履歴読み込み、close 後の read-only snapshot、再open後の履歴読み込みで一致を確認。

前段 PR #17 / #19 / #18 の修正を merge commit で取り込んだ。workspace 全体の検証と各 OS の受け入れは、この修正を含む最新 PR CI の結果を正本とする。過去の計測・検証 receipt の source hash は書き換えない。
