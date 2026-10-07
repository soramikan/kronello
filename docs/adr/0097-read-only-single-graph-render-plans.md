# ADR-0097: 読み取り専用の単一 graph 実行計画

- 状態: 採用
- 日付: 2026-10-06
- 対象: INSPECT-002

## 決定

`render.explain` は固定 snapshot、正規化された shutter sample、各 sample の scene / tile DAG をコンパイルして計画を返す。backend の作成、GPU probe / dispatch、video decode、raster cache の参照、transfer を実行しない。計画の compilation cache は要求専用であり live cache と区別する。

ADR-0092 の単一 graph は final linear / display を共通の RGBA16F texture から生成する。final CPU 出力境界は execution region に対する row-padded RGBA16F 画像二面と sticky status 4 byte 一回で、各実行 3 readback operations。`DUPLICATE_LINEAR_DISPLAY_RENDER` を廃止し `SINGLE_GRAPH_LINEAR_DISPLAY_OUTPUT` を返す。

native preview は final CPU frame の tile traversal と独立した全領域の一 graph である。画像 readback はゼロ、status は 4 byte / 一操作。`native_preview_transfers` は代替境界であって final transfers に加算しない。CPU reference と temporal の CPU accumulation にはこの native preview 境界を提示しない。

見積もりは cold execution、raster / temporal cache hit なし、cache persistence なしの境界を明示する。actual request counters とは区別し、cache hit、external/native decode conversion、control upload と driver allocation を推測しない。CPU reference の GPU transfer はゼロ、未知 backend は不明（null）とする。通常 GPU の video image upload は元 source 寸法と conversion / reuse が分からないため不明であり、出力 draw bounds から捏造しない。明示 RasterInput は RGBA16F の 8 byte / pixel upload。

Temporal は実 renderer と同じ `temporal_samples` を使い全 sample / tile を列挙し、final transfers と graph execution count を合算する。CPU root accumulator と whole-frame / tile streaming の host memory 境界を notice で明示する。strict resident + temporal は実行と同じ型付き未対応とする。surface budget は静的な保守式であり実 GPU memory の測定値ではない。GPU synthetic output root の限定 elision をこの式にも反映する。

## 検証

CPU 実行回数と warm temporal hit、共有 API の反復・revision 不変、固定 GPU final / native texture / resident graph / temporal tile の counters 比較を [INSPECT-002](../testing/inspect-002.md) に記録する。
