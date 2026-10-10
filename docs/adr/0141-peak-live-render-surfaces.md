# ADR-0141: レンダー面予算を同時保持寿命に合わせる

- 状態: 採用
- 日付: 2026-10-10
- 対象: RENDER-005
- 部分置換: [ADR-0079](0079-bounded-streaming-movie-export.md) の全 image stage・全 Group children・全 effect temporary の累積加算だけを置換する。512 MiB、union execution ROI、最大512×512 tile、型付き拒否、backend 選択、音声・出力契約は維持する。

## 背景

90字程度の日本語でも、字形ごとの全タイル面を同時保持として加算すると予算を超える。CPU/GPU executor の pass-local memo も単一参照結果を末尾まで保持していた。予算式だけの縮小は実際の保持量を過小評価するため、所有権と計算を同時に変更する。

## 決定

- 単一参照の描画結果は consumer の処理後に解放する。Group は一つの accumulator に子を順次合成する。複数参照（重複 edge・root を含む）の結果だけを pass-local memo に保持する。共有結果は安全側に pass 全期間の保持として数える。
- 入力の評価順を保持し、Group は accumulator + 最大の子の作業量、通常の多入力ノードは先に評価した入力と destination + 次の入力の作業量、effect は入力・出力と同時 temporary の最大を数える。標準 effect は6 temporary、その他の effect は3 temporary、coverage/generator は1 temporaryを保守的に確保する。連続 effect の temporary を全段分加算しない。
- 共有 memo、外部 raster の保持コピー（2面/入力）、blank・最終変換/crop/readback の4面 reserveを加算する。GPU の sticky validation 待ち cache publication は最新8面まで保持し、最大8面（候補ノード数以下）も事前予算に含める。検証成功後だけ公開する契約を維持する。永続 cache / pool は既存の独立 byte budget で管理する。
- `SurfaceLifetime` / `peak_surface_count` を RenderDag・CPU/GPU backend・読み取り検査の共通計算にする。具体 backend は、全結果を保持すると仮定した従来の保守的上界との小さい方を採る。小さな graph で publication が既存結果への参照であることを二重に課金せず、従来通った単純な4K native previewを維持する。synthetic root 省略による差もあり、DAG の値は安全側になる。union execution ROI の全面積 × bytes/pixel × 同時面数上界を計算し、overflow も拒否する。ROI の小さい字形でも allocation が全面である以上は全面として数える。
- 通常 frame と movie streaming は同じ事前検査を通し、float32 の16 bytes/pixelで512 MiB超過を backend allocation 前に `UNSUPPORTED_FEATURE` とする。backend の8/16 bytes/pixel検査、node/depth/edge制限は維持する。画素演算・合成順序・色変換・fallback 方針は変更しない。

## 影響と検証

文字数に比例して単一参照の面を保持しなくなる。共有面の多い DAG や大きな cumulative halo は引き続き拒否される。GPU の大きな graph では検証待ち cache の上限により再利用候補が減りうる。これは画素の意味を変更しない。算出値は payload の保守的上界であり、driver・allocator・幾何・font・RSS の実測値ではない。

[レンダー設計](../architecture/05-render-gpu.md)、[検証記録](../testing/render-005.md) を参照。GPU 実測の有無は検証記録で区別する。
