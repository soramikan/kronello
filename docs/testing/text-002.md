# TEXT-002 検証

状態: `done`。統合受け入れは [M5 checkpoint](m5-acceptance.md) を参照。

対象: 組版意味版 2 の文字 selector、ルビ、縦書き。正本は `TextDocument` と共有 `TextSet`、通常 Property / Curve 評価、render compiler / semantic cache / coverage renderer である。

## 受け入れ対応

- クラスタを壊さない文字演出と親文字 / ルビ結合: `advanced_vertical_ruby_and_logical_selectors_survive_reflow` は固定 Noto font の日本語、結合濁点、IVS、句読点、Latin を縦組みし、親範囲の一部を選択して親全体とルビが同じ offset / opacity を受けることを確認する。
- 古い glyph index への誤適用防止: source range と `expected_text` を文書に保存し、毎回書記素境界と本文一致を検証する。折り返し高さ変更後も同じ source range が同じ親とルビを選択する。結合濁点内部の範囲は `InvalidCharacterAnimation` で拒否する。
- 実経路: `text002_document_properties_ruby_vertical_render_and_reflow` は `TextDocument` の selector を node-owned Curve に結び、固定 snapshot の任意時刻 CPU render と reflow を検証する。

## コマンド

```sh
cargo test -p kronello-text --locked
cargo test -p kronello-model --locked --test text
cargo test -p kronello-render --locked --test render text002
```

TEXT-001 の既存横書きテストを維持する。GPU の固定環境画素比較をこの CPU 検証から保証しない。縦中横、高度な混植 orientation、独立ルビ演出、gradient と文字 opacity の併用、混合 style / 改行を跨ぐ ruby は保証範囲外である。

## この作業時点の実行記録

2026-10-06: `kronello-text` の既存 21 件と追加 vertical/ruby selector 1 件の合計 22 件が成功した。その後追加した source fingerprint / ruby glyph membership の変更後は `advanced_vertical_ruby_and_logical_selectors_survive_reflow` を再実行して成功し、縦句読点 substitution の追加テストも成功した。`text002_selector_captures_source_and_rejects_stale_same_length_edits` と CPU renderer の `text002_document_properties_ruby_vertical_render_and_reflow` が成功した。

共有 service の `text002_shared_textset_selector_roundtrip_undo_and_stale_edit_rejection`（`TextSet` / Property insert / undo / stale selector の原子的拒否）は成功した。全 workspace 796 tests / 0 failures、fmt / clippy / API / schema の成功と advanced text capability の統合確認を [M5 checkpoint](m5-acceptance.md) に記録した。

## 直接GUI確認で見つかった配置修正（2026-10-06）

縦書きの「日本語の編集。ABC」、親範囲「日本」へのルビ「にほん」をnative GUIで表示し、ルビと親文字の重なりを検出した。shaping originではなく親とルビの最終ink boundsを用いる配置へ修正し、32 design pxの親に対する3.2 design px以上の間隔を回帰試験で確認した。修正後のtext全24テストは成功。その後、主担当が修正したFFIとSwiftを組み立てたdebug appを直接起動し、ルビが親「日本」の右側へ間隔を保って配置されることを確認した。


同じ [M5 fixture](../../examples/m5-text-matte.project.json) で時刻0→1秒へGUIでシークし、親文字とルビが一緒に+50 design px移動することを確認した。縦書きのWrap widthを250→150へGUIで変更してrev 8へ進め、layoutが104×250→156×150に変わっても「日本」と「にほん」の対応・offsetが保たれた。Matteの横書き対象と独立した縦書きの変化を観察した。証拠は`target/m5-acceptance/gui/text-ruby-{fixed,animated,reflow}.png`。これは直接GUIでの視覚確認の証拠である。共有コアのfmt / clippy / workspace / API / schema成功は別途 [M5 checkpoint](m5-acceptance.md) に記録し、統合受け入れを完了した。
