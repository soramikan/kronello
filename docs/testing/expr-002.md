# EXPR-002 人間向け式構文と式入力UIの受け入れ記録

状態: `done`（2026-10-07）。`codex/m5-completion` の作業ツリーで受け入れた。mainへのマージ・各OS CIの保証とは区別する。

構文は[提案書](../roadmap/m5-expression-syntax-proposal.md)を所有者が採択し、[ADR-0105](../adr/0105-human-readable-expression-syntax.md)で決定した。ASTが正本で、JavaScript互換にしない方針を維持する。

## 構文・往復・診断（Rust）

`crates/kronello-model/src/expression_syntax.rs` に canonical parser / formatter、`crates/kronello-eval/src/expression.rs` に `format_expression` を実装した。

- `cargo test -p kronello-eval --test expression_syntax --locked`: 6/6 PASS。数値リテラル・四則・関数呼出の parse→format→parse 往復、空白の正規化、AST深さ64超過の拒否、byte offset / 行 / 列 / expected tokens / `EXPRESSION_SYNTAX` を含む構造化診断を確認した。
- `cargo test -p kronello-service --test expressions --locked`: `property_expression_text_set` で text→parse→AST保存、syntax拒否時に保存済みASTとrevisionを変更しないこと、`expression.format` query での canonical 往復を確認した。
- `cargo test -p kronello-service --test api --locked`: 12/12 PASS。手書き wire decoder に `expression.format` / `expression_text` が欠落して `INVALID_REQUEST` になっていた欠陥を修正し、全登録操作の実行カバレッジに `expression.format` を追加して再確認した。

## GUI の式入力

`apps/macos/Sources/Kronello/ExpressionField.swift`・`PropertyFields.swift`・`apps/macos/Sources/KronelloAppModel/ExpressionAuthoring.swift` に Inspector の式欄を実装した。draft はローカル状態で、Return / フォーカス離脱の commit だけが `property_expression_text_set` を呼ぶ。構文診断は項目下のインライン表示と、汎用エラーのsheet両方に構造化JSONで表示する。

- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift test --package-path apps/macos`: ExpressionAuthoring の 12/12 を含む全 suite PASS。実workerを使う end-to-end 試験で attach→commit→detach の revision 推移を確認した。
- IME安全は `KRCommittedTextView` の `hasMarkedText` ゲートで実装し、`EditChecks`/`QAChecks` が host view へ実際に `setMarkedText` を送って検証する。

## root の直接GUI確認（macOS）

Computer Use の MCP server はこの環境に存在しなかったため、macOS Accessibility（System Events）経由の実操作と `screencapture` の画像で確認した。CUA の主張とは区別する。対象は同梱FFIと`kronello` CLIを再構築して束ねた `target/macos/Kronello.app` で、fixture は `target/m5-acceptance/gui/m5-final/edit.kronello`。画像・ログ・projectは同ディレクトリに保存した（Git管理外）。

| 操作 | 結果 |
|---|---|
| Opacity の fx で式をアタッチ | rev 2、欄に canonical `1` を表示、expression `4fa5e8b3`（literal scalar 1.0）が保存された |
| `0.5` を入力してReturn | rev 3、Opacity が `50.0 %`、保存ASTが literal scalar 0.5、`expression.format` で `0.5` に往復 |
| `0.5 +` を入力してReturn | sheet と欄下に `EXPRESSION_SYNTAX`・`1:5: expected expression, found end of input`・byte range・expected tokens を表示。保存ASTとrevisionは不変で、draftと赤い診断だけが残った |
| Cmd-Z | rev 4、Opacity が `100.0 %` に戻り、式 source は維持（Undoはtext commitのみを戻す） |
| `式を解除` | rev 5、Opacity が constant `1.0` に戻り、式欄が消えて fx が復帰 |
| 再アタッチ後、Kotoeri ひらがな入力モードで `nihongo` | rev 6 のまま、変換中の marked text `日本語` / 未確定 `にほんご` の間は確定まで commit が発行されない |
| marked text 中の Escape | 1回目はIME側で未確定解除、2回目でdraft取消。いずれも commit なし |
| ABC入力へ戻して `0.25` + Return | 正常にcommitされ rev 7、Opacity `25.0 %` |

## 残る境界

- 診断表示は構文段階のもの。type / unit / budget の診断を欄へ別途描画する拡張はこのタスクの範囲に含めない。
- 上表はrootの環境（macOS arm64）での確認であり、Windows / Linux GUI（GUI-005/006）の対象外である。
