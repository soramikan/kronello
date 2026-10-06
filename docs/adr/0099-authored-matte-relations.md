# ADR-0099: 保存する Matte 関係

状態: 採用（設計契約。受け入れ完了は検証記録と backlog で管理）
日付: 2026-10-06
対象: MATTE-001

## 決定

`Project.mattes` に `DocumentObject<MatteRelation>` を保存する。`MatteRelation` は独立 UUID の `id`、`version: 1`、`composition`、`source` / `matte` の NodeId、`kind: alpha | luminance`、`invert`、`visible` を持つ。配列番号・表示名を ID にしない。異なる Composition の直接 Node 参照は許可しない。同じ Composition definition の instance ごとに、同じ `InstancePath` の source と matte へコンパイルする。

これは `RenderSnapshot.mattes` の transient `MatteBinding` とは別の正本である。既存 transient 入力は保存しない。文書関係と transient 入力が同じ source に重なった場合は `MATTE_DUPLICATE_SOURCE`。後勝ち・入口ごとの置換はしない。

## 合成と失敗の意味

- `alpha` は premultiplied matte の alpha を被覆にする。`luminance` は作業用線形 Rec.709 / Rec.2020 の premultiplied RGB の luminance を `[0,1]` に clamp して使う。matte alpha を改めて掛けず、透明部の luminance は0。
- `invert` は上の被覆を `1 - coverage` にする。source の全 premultiplied RGBA に被覆を掛ける。CPU reference と GPU WGSL に同じ意味を実装する。
- matte は既定で通常の描画列から除外する。`visible` が true の場合は通常の描画順でも表示する。source / matte の effect と Group isolation は既存 DAG の node 出力に従う。描画順を関係の参照規則にしない。
- 一つの source に関係は一つ。source / matte / Composition 欠落は `MATTE_MISSING`、多重 source は `MATTE_DUPLICATE_SOURCE`。containment の子への依存と source → matte の依存を同じ graph で検証し、循環は `MATTE_CYCLE`。削除で dangling 関係を黙って解除しない。同じ transaction 内で関係を明示削除できる。
- active な source に対し disabled / active_range 外等で matte が active scene に存在しない場合も `MATTE_MISSING`。inactive source の関係はその時刻に合成を要求しない。
- 関係数は4,096、検証の graph recursion は1,024を上限とし、超過は `MATTE_BUDGET_EXCEEDED`。既存 renderer の scene / nesting budget も維持する。
- 未知フィールド・variant は `DocumentObject::Opaque` で lossless 保存し、未知版・opaque 関係の編集と実行は `UNSUPPORTED_FEATURE`。既存文書の `mattes` 省略は空集合。snapshot は `document_matte: Some(1)` を固定し、legacy pin 省略を許可するのは関係が空の文書だけ。

## 編集と入口

共有 `EditCommand::MatteSet { matte }` / `MatteRemove { id }` を `edit.plan` / `edit.apply` で扱う。通常の revision 照合・idempotency・保存・Undo を使う。関係・Composition と source / matte に structure changed key を付け、競合を関係の外へ隠さない。

macOS Inspector の Matte section は同じ command の JSON を `EditorModel.submit` に渡す。選択対象は現在の Composition に直接所属する Node、候補は同じ Composition の他 Node。Matte layer / Alpha・Luminance / Invert / Show matte を編集できる。instance の内部を GUI 専用モデルで書き換えない。shared errors は通常の失敗表示へ送る。

## 証拠

[MATTE-001 検証記録](../testing/matte-001.md)。公開 schema と GeneratedAPI は共有 Rust 型から統合担当が再生成する。
