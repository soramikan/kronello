# ADR-0129: メディア管理（ビン・ブラウザ・オフラインメディア）

- 状態: 採用
- 日付: 2026-10-08

## 背景

FLOW-002 はビン・メディアブラウザ・サムネイル一覧と
オフラインメディア表示・relink 操作を要求する。
ビンが作品状態か環境設定か、オフラインの表現を決める。

## 決定

- ビンは作品ドキュメントの状態とし、
  `Project.bins: Vec<Bin>`（`id`・`name`・`assets: Vec<AssetId>`）
  をモデルに追加する。不変条件「入口専用の作品状態を作ら
  ない」に従い、GUI 固有の場所へは置かない。
  asset は複数 bin に属せる。
- `bin.create/rename/delete/assign` の共有編集を追加し、
  CLI/MCP/GUI が同じ API を使う。
- オフラインメディアは asset の locator 解決失敗状態を
  既存の asset status query で表示し、GUI から既存の
  `relink` 操作を呼ぶ。表示は一覧・サムネイルとも
  offline バッジを出し、検査はプロジェクト export と
  同じ内容を使う。
- サムネイルは固定 snapshot の preview 経路で生成し、
  wall clock・非固定乱数を使わない。GUI 表示用のキャッシュ
  であり作品状態には保存しない。
