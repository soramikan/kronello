# ADR-0118: モーショントラッキングと結果の DataAsset 化

- 状態: 採用
- 日付: 2026-10-08

## 背景

TRACK-001 は点/平面モーショントラッキングの解析と、
結果を Property/DataAsset として式・エフェクトへ接続する
ことを要求する。解析は時間のかかる処理であり、結果の
保持形式・実行形態・参照方法を決める必要がある。

## 決定

### 実行形態

- `track.analyze` コマンドを追加する。入力は asset +
  範囲 + 追跡モード（`points` / `plane`）+ 初期 seed
  （`points`: 各追跡点の初期矩形 or 点、`plane`: 4 隅）。
- フレーム数上限を設けた同期コマンドとする（例
  `frames ≤ 4000`）。より長い範囲は将来の job 化を
  open-question に残し、M8 は同期実行とする。

### 解析アルゴリズム

- 点追跡は**正規化相互相関（NCC）**によるテンプレート
  マッチで、前フレーム位置を中心に有界探索窓を走査する。
- 平面追跡は 4 隅の点追跡から homography を推定する。
- すべて決定的実装（乱数なし・同一入力で同一結果）とし、
  結果に confidence（NCC スコア）を含める。

### 結果の保持

- `TrackingDataAsset` を `DocumentObject` に追加する
  （`AudioAnalysisDataAsset` と同じパターン）。フィールド:
  `id`, `source_asset`, `range`, `mode`, `frames`（各フレームの
  {frame, track_id or corner index, x, y, confidence} の列）、
  `sample_rate`。
- 生成は `edit.apply` の mutation として行い、Undo・共有
  編集・リビジョン管理に乗せる。source asset の content_hash
  で版固定し、hash 不一致は型付きエラー。

### 参照

- 既存の `ExpressionNode::DataAssetCell` と
  `property.sample` で参照可能にし、`x`, `y`, `confidence`
  等の列名を公開する。GUI での追跡点指定は M8 では
  最小限（コマンド起点）とする。

## 影響

- 解析結果は決定的な DataAsset としてドキュメントに残り、
  式やエフェクトから再利用できる。
- asset のロケータ解決・decode は既存 media 経路を使う。

## 関連

- TRACK-001、COMP-002、ADR-0102（評価済み asset）、
  `crates/kronello-model/src/audio_analysis.rs`、
  `crates/kronello-service/src/audio_analysis.rs`
