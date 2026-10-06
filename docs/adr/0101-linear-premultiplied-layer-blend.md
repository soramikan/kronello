# ADR-0101: 線形 premultiplied layer blend

状態: 採用（設計契約。GUI-007 の受け入れは検証記録と backlog で管理）
日付: 2026-10-06
対象: GUI-007

## 決定

Clip の既存 `properties` に固定 descriptor `kronello.blend_mode`（version 1、Enum `normal | multiply | screen`）を保存する。省略は従来の source-over。独立の Clip 欄・入口専用の状態は追加しない。共有 `TimelineCommand::ClipSetEffects` が既存 properties / effects とともに変更し、revision、idempotent replay、Undo を既存 transaction で扱う。GUI は既存 PropertyId を保持し、初回だけ新しい UUID を生成する。

descriptor は非アニメーションで、Curve / Expression / Modifier は許可しない。未知 enum・未知 descriptor 版・同じ layer の重複指定は拒否する。既存の Property 評価と template input の最終値を使用し、評価後の enum も検証する。Composition の Node に明示された同じ descriptor にも同じ layer 契約を適用する。

## 合成

source の fill / stroke / glyph / children を隔離し、node opacity、effects、transition opacity、matte を適用してから、同じ containment group の先行兄弟を backdrop として blend する。Group 自身の mode は Group 内部へ継承せず、Group の完成出力を外側の兄弟列へ合成するときに適用する。Clip は Sequence の既存 bottom-to-top 順を用い、CompositionClip の内部へ placement の blend を配らない。

既存の source-over と同じく、明示された線形 working space（Rec.709 / Rec.2020）の premultiplied RGB を使う。display sRGB に変換してから blend しない。straight color の blend function を B、source/backdrop alpha を as/ab とすると、RGB は `Cs*(1-ab) + Cb*(1-as) + as*ab*B(cs,cb)`、alpha は `as + ab*(1-as)`。

- Normal: 既存 source-over。
- Multiply: `Cs*(1-ab) + Cb*(1-as) + Cs*Cb`。
- Screen: `Cs + Cb - Cs*Cb`。

CPU と WGSL は上の premultiplied な閉形式を使う。zero / tiny alpha の除算を不要にし、透明 source/backdrop を正しく保持する。RGB の `[0,1]` clamp はしない。HDR の 1 超・負値も保持し、有限値・RGBA16F 表現範囲・alpha の既存検証に従う。線形 Screen は display sRGB の Screen と同じ見た目を約束しない。

## DAG / cache / snapshot

非 Normal の合成は明示的 `Blend {source, backdrop, mode}` node にする。両入力の union bounds と ROI、面予算、read-only plan の `BLEND` stage を計算する。raster cache identity は両入力・mode・working space・実行 ROI・backend namespace を含む。全 layer が省略 / Normal の通常経路は既存の isolated source-over DAG を維持する。

snapshot は `semantic_versions.blend: Some(1)` を固定する。legacy の省略 pin を認識するのは authored blend property が一つもない文書だけ。明示 Normal property も template input で別 mode になる可能性があるため pin を必要とする。未知 pin と必要な pin の欠落は `UNSUPPORTED_FEATURE`。snapshot 作成後に最新の mode 契約へ読み替えない。

## 検証

[GUI-007 blend 検証記録](../testing/gui-007-blend.md)。公開 schema / Swift GeneratedAPI と最終 workspace / GUI の受け入れは統合担当が確認する。

2026-10-06: GUI-007の全条件は直接GUI・worker・CLI/MCP同等性と統合checkpointで受け入れ済み（[受け入れ記録](../testing/gui-007.md)）。本ADRの設計決定は変更していない。
