# ADR-0095: 論理文字 selector と縦書き・ルビ

- 状態: 採用
- 日付: 2026-10-06
- 対象: TEXT-002

## 決定

組版意味版 1 の横書き・未対応拒否を維持し、`layout_version = 2` で縦書き、ルビ、文字演出を明示選択する。縦書きは rustybuzz の TopToBottom shaping と OpenType の縦字形を使い、列は右から左へ進む。日本語は正立、Latin script の outline は時計回りに回転する。`wrap_width` は縦書きでは列の高さ、`line_height` は列間隔である。縦中横、混植の高度な Unicode orientation、RTL はこの版の対象外である。

`CharacterAnimation` は UTF-8 source range とその範囲の `expected_text`、offset / opacity の node Property を保存する。範囲は拡張書記素の境界で検証し、本文が変わった selector は明示更新が必要となる。再組版では source range を安全な shaping unit に投影してから outline と色へ適用する。glyph index を文書に保存しない。

ルビは同一 style の親範囲に限り、親の半分の font size で shape する。親範囲内部の soft break と animation unit 分割を禁止する。横書きは親の上、縦書きは右に中央配置する。親の selector はその ruby outline にも適用する。重複・空ルビ、境界外の範囲を拒否し、改行を跨ぐルビ・混合 style のルビは型付き未対応にする。

組版 cache は source / style / ruby / direction / dimensions を含み、offset / opacity は組版後に適用する。geometry cache は最終 outline 自体を含むため再利用した組版に古い演出が残らない。gradient と文字 opacity の併用は明示的に未対応とする。

## 理由と影響

既存の横書き意味を保存したまま縦書きを導入し、全文組版を先に固定した文字演出を通常 Property の任意時刻評価へ接続する。本文編集と reflow の意味を分離し、再組版による glyph index 変化を誤った文字への適用原因にしない。高度な編集 GUI、語辞書・タグ selector、縦中横はこの実装の保証範囲に含めない。
