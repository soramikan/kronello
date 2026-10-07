# ADR-0113: LUT（.cube）の取り込み・検証・適用位置とスコープ観測契約

- 状態: 採用
- 日付: 2026-10-08

## 背景

COLOR-003 は .cube 3D LUT の取り込みと適用、COLOR-004 は
waveform / vectorscope / histogram / RGB parade のスコープを
要求する。LUT の保持場所（文書内 DataAsset か外部 Asset か）と
適用位置、スコープが観測する評価点と wire 形式を決める必要がある。

## 決定

### LUT

- .cube（IRIDAS）形式をサポートする。`LUT_3D_SIZE N`、
  `DOMAIN_MIN`/`DOMAIN_MAX`、データ行を検証する。受理する
  サイズは 2..=65、M8 の文書内保持上限は **N ≤ 33**
  （33³ × RGB）とする。1D LUT と TITLE/COMMENT は許容するが
  1D LUT のみのファイルは `UNSUPPORTED_FEATURE`。
- LUT は `AssetKind::Data` の外部 Asset として保持し、
  content_hash で版固定する。描画系は fonts 入力と同様に
  `luts` 入力（hash → bytes）で供給され、パーサが固定形式に
  正規化する。ドキュメント本体には展開しない。
- 新エフェクト `kronello.color.lut` v1 を追加する。
  パラメータは `lut`（`ValueType::AssetRef`、Data 種別の
  AssetId）と `intensity`（0..=1、恒等 LUT との線形補間）。
  他の COLOR-002 エフェクトと同じく authored 順に適用する
  pointwise 扱いで、alpha を保持する。
- サンプル位置は working space（linear premultiplied を
  unpremultiply した RGB）。入力は LUT の domain に正規化し、
  domain 外は端に clamp。HDR の負値・>1 は clamp 前にそのまま
  扱い、規約として「LUT は display-referred 0..1 を想定するが
  clamp で拡張範囲を捨てない（=端色を返す）」と定める。
  補間は tetrahedral。

### スコープ（COLOR-004）

- 新クエリ `inspect.scopes`（仮名、実装は api.rs に登録）:
  対象（composition or sequence + time）の評価済みフレームから
  4 種のデータを返す。
  - waveform: 列ごとの輝度分布（2D bin、列数 = 表示幅に
    独立した固定分解能、例 512×256）
  - vectorscope: Cb/Cr 2D ヒストグラム（256×256 等）
  - histogram: R/G/B + luma の 256 bin
  - RGB parade: 3 系統の列別分布
- 返却は bin 配列（整数カウント、必要なら正規化係数）で、
  画像ではない。GUI はデータから描画する。評価は固定
  snapshot に対して行い、リビジョンを結果に含める。
- 観測点は「合成後・working space のフレーム」。display 変換は
  GUI 側の表示範囲でのみ行い、スコープ値は working space の
  直接値を返す。

## 影響

- LUT 欠落・壊れた .cube・非対応形式は型付きエラー。
  最終描画・書き出しでも同じ規約で評価される。
- スコープは query であるため CLI/MCP からも同一値を取得できる。

## 関連

- COLOR-003、COLOR-004、ADR-0108（版付き色補正）、
  ADR-0048（media 由来の型付き検証）
