# 04 ベクター・日本語テキスト・レイアウト

## ベクター

Path、Fill、Stroke、Gradient、ClipPath を意味的 IR に保持する。最初からビットマップ化して保存しない。

| 用途 | 候補 |
|---|---|
| 幾何演算 | kurbo |
| テッセレーション | lyon |
| SVG 読み込み | usvg |

これらの完全な SVG 互換やあらゆる Path 演算を前提にしない。
SVG 読み込みは対応表を持つ。外部 URL、script、外部フォント等は自動取得・実行せず、明示インポートする。

初期から矩形、角丸矩形、楕円、ベジェパス、単色塗り・線・基本グラデーションを扱う。
Trim path、線端・破線のアニメーション、Path boolean、morph は段階実装する。

### 設計寸法と出力解像度

Composition の design_extent と出力画素数を分ける。同じ 16:9 で解像度だけを変える場合は原則再レイアウトしない。
16:9 → 9:16 等のアスペクト比変更は、明示した responsive variant / constraint で再レイアウトする。

## 日本語テキスト

```text
UTF-8 + style spans + ruby associations
 -> grapheme / shaping clusters
 -> glyph selection, metrics, Japanese line breaking
 -> LayoutResult (lines, glyphs, bounds, anchors)
 -> animation units / selectors
 -> glyph coverage / paths
 -> linear-HDR compositing
```

Parley / Fontique を基礎候補とするが、禁則、ルビ、縦書きの要件充足は個別に検証・補完する。

- Unicode の書記素クラスタとグリフは一対一ではない。元テキスト範囲から組版クラスタ・グリフへの対応を保持する。
- 「一文字ずつ」は既定で組版クラスタを壊さない AnimationUnit に変換する。
- ルビ付き文字は親文字 + ルビを一体の単位にする既定動作を用意し、独立演出は明示指定とする。
- セレクターは文字、行、語、範囲、タグに対応するが、語分割の辞書・アルゴリズムを版管理する。
- テキスト更新で範囲指定が無効になった場合は再計算を報告し、曖昧な古いグリフ番号をそのまま使用しない。

## レイアウトと描画の分離

Position / Opacity の変更では原則組版を再実行しない。本文、フォント、サイズ、折り返し幅の変更は組版キャッシュを無効化する。
語数・文字数が変わる型送りでは、原則として全文を組版してから表示単位を隠す。行が毎フレーム組み直される方式を既定にしない。

### bounds の段階

| bounds | 意味 |
|---|---|
| layout_bounds | レイアウト用の幅・高さ・行ボックス |
| ink_bounds | 実際の字形・線などが占める領域 |
| visual_bounds | shadow / glow / blur / transform を含む描画領域 |

背景帯の自動追従は layout_bounds 等の明示段階を参照する。
`背景幅 <- テキスト幅 + 余白` と `テキスト折り返し幅 <- 背景幅` のような循環は検出して拒否する。
解像度別の最大行数、最小文字サイズ、安全領域、overflow の方針をテンプレートの制約として持つ。

### 実装段階

- M2: テキストの layout_bounds への単方向参照による背景帯追従と、`max_lines` 超過の overflow 検出（TEMPLATE-001）。
- M3: 3 種の bounds の区別、循環診断、responsive variant による再レイアウト（LAYOUT-001、TEMPLATE-002）。

## 描画バックエンドの制約

確認した Vello の `render_to_texture` API は `Rgba8Unorm` を要求する。この経路を HDR 全画面合成へ直結しない（[ADR-0012](../adr/0012-hdr-compositing-vs-vector-rasterizer.md)）。

- 文字や単色形状の coverage マスクを生成して、RGBA16F 側で色を適用する構成を優先する。
- 任意のグラデーションや色付き SVG まで「coverage だけで完全再現できる」とは扱わず、必要な色処理は独自 GPU 描画パスで実装する。
- 同じベクター IR から異なるラスタライザーへ渡せるようにし、Vello を作品の保存形式にしない。
