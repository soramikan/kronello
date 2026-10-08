# ADR-0119: プロキシワークフロー（生成ジョブ・編集時切替・relink）

- 状態: 採用
- 日付: 2026-10-08

## 背景

PROXY-001 はプロキシ生成 job・編集時のプロキシ参照切替・
relink・書き出しでは元素材を使う検証を要求する。
「どこにプロキシを紐付けるか」「編集時切替を作品状態に
するか」を決める必要がある。

## 決定

### モデル

- `Project.proxies: Vec<ProxyLink>` を追加する
  （`#[serde(default, skip_serializing_if = "Vec::is_empty")]`）。
  `ProxyLink { original: AssetId, proxy: AssetId, scale: Rational,
  created_by: JobId or info }` で original ↔ proxy を紐付ける。
  Asset 自体は変更しない。
- proxy asset は `AssetKind::Video` の通常 Asset で、
  content_hash 固定・独立して relink/collect 対象になる。

### 生成ジョブ

- `proxy.generate` ジョブを追加する。入力は asset 集合 +
  `scale`（既定 0.5、選択肢は制限付き）で、出力は
  ProRes mezzanine（既存の ProRes 書き出し経路）として
  `.kronello` 管理下または隣接フォルダに生成する。
  完了時に `ProxyLink` を `edit.apply` で登録する。

### 編集時切替

- 切替は**セッション/評価入力側の表示モード**とし、作品
  ドキュメントの永続状態にはしない（「入口専用の作品状態を
  作らない」に反しないため、プレビュー品質スイッチとして
  扱う）。render/sequence/preview の入力に
  `media_proxies: bool` 相当のフラグを設け、on のときに
  `ProxyLink` があれば proxy asset を decode 対象に選ぶ。
- 書き出し（`render.submit`/`render.export`）は常に
  original asset を使うことをテストで検証する。

### 整合

- proxy が欠落・hash 不一致・元素材と尺が合わない場合は
  型付きエラーまたはフル品質へのフォールバックを明示する。
- `asset.relink` / `project.collect` は proxy asset も対象に
  含める。

## 影響

- 編集時の decode 負荷を下げつつ、書き出し品質は
  元素材で保証される。GUI は「プロキシを使う」トグルを
  Preview パネルに追加し、link 状態を Inspector に表示する。

## 関連

- PROXY-001、MEDIA-002、JOB-001、ADR-0048、
  `crates/kronello-model/src/asset.rs`
