# ADR-0145: 既知不正文書を診断して拒否し、CSS 時間イージングを許す

- 状態: 採用
- 日付: 2026-10-11
- 部分置換: [ADR-0046](0046-store-format-and-location-policy.md) の decode 失敗時に無条件で opaque 保存する条項、[ADR-0070](0070-motion-keyframe-authoring.md) の x ハンドル順序制約のみ。

## 背景

対応する AnimationCurve の不正データが decode 失敗により opaque として保存され、後続の全編集が object を特定できない `UNSUPPORTED_FEATURE` になった。また `x1 <= x2` は単調時間の十分条件だが必要条件ではなく、一般的な CSS ease-in-out を拒否していた。

## 決定

- 厳密 decode の失敗を未知内容と既知不正に分ける。型から生成した schema の未知フィールド・enum / tag、および集合ごとの未対応意味版は従来どおり lossless に保持する。対応版で構造が既知なら欠落・型・値・検証違反を `InvalidObject` / `InvalidDocumentObject`（API: `INVALID_DOCUMENT_OBJECT`）で拒否する。decode の一時的な候補が opaque でも、保存前検証を通過させない。
- 診断は集合名、安定 UUID、検証理由を持ち、API の message と `details.collection` / `id` / `reason` に返す。create / import / 各 `_plan`、低水準 import と保存を同じ境界で検証し、失敗時に作品を作成・変更しない。
- `ensure_editable` は opaque・未対応意味版・effect を持つ全 object を列挙する。`UNSUPPORTED_FEATURE` の message と `details.objects` に集合名・ID・理由を残す。文書意味版と project の未知フィールドも project ID で示す。
- TimeBezier の有限 x は各々 `[0,1]` とし順序を問わない。有限 y の overshoot は維持する。GUI の接線編集は各 x を独立に clamp し、相手の x を上限・下限にしない。

## 単調性と評価

`x'(t)/3 = x1(1-t)^2 + 2(x2-x1)t(1-t) + (1-x2)t^2` は x1 / x2 に関して affine。単位正方形の四隅 `(0,0)`, `(0,1)`, `(1,0)`, `(1,1)` ではそれぞれ `t^2`, `2t(1-t)`, `(1-2t)^2`, `(1-t)^2` で非負。その凸結合である任意の `[0,1]^2` でも非負となる。恒等的にゼロの導関数にはならず、x は厳密増加で逆関数が一意に存在する。

従来の64回の二分法は導関数で割らず交差ハンドルでも収束する。ただし `(1,0)` の x ハンドルは中央に stationary point があり、f64 の x が丸めで plateau になる。新たに許すこの端の組だけ `x(t)=1/2+4(t-1/2)^3` の解析的逆関数を使う。既存の有効入力には同じ計算を維持するため `INTERPOLATION_VERSION=1`、公開 schema version と SQLite user_version は変更しない。

## 維持する契約と影響

ADR-0045 の保存・実行互換性境界、未知内容の所属・数値・参照の lossless 保持、ADR-0046 の他の保存・履歴・場所判定、ADR-0070 の他の編集規約は維持する。過去に保存された既知不正 opaque も読み取り時の保存検証で型付き拒否となり、原本は変更しない。未知フィールドや未知版を持つ object の意味を推測して修復しない。

公開 JSON Schema の opaque 選択肢は保存外枠を示す。JSON Schema 適合だけで既知内容の意味検証を代替しない。model / animation / store / service の通常テストと macOS 接線編集テストで確認する。検証結果は [MODEL-004](../testing/model-004.md) に記録する。
