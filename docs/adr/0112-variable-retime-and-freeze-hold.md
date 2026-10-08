# ADR-0112: 可変リタイム（スピードランプ）と freeze frame / hold

- 状態: 採用
- 日付: 2026-10-08

## 背景

NLE-006 はクリップ内で速度を変えるスピードランプと
freeze frame / hold を要求する。`TimeMap::PiecewiseLinear` は
既に存在するが、local が厳密単調増加のみ許されるため
ゼロ速度（freeze）の区間を表現できない。また既存の検証は
速度ランプ用の編集コマンドと音声ポリシとの整合を持たない。

## 決定

### TimeMap

- `PiecewiseTimeMap` の local 制約を「非減少」に緩和する。
  `local[i] == local[i+1]` の区間は hold（freeze）区間であり、
  写像はその区間で `local[i]` を返す。逆単調は引き続き拒否
  （`UnsupportedMapSlope`）。既存ドキュメントは厳密単調を
  満たすため後方互換。
- `TimeMap::map` は hold 区間を自然に処理する。
- `inverse_canonical` は非単射となるため、hold 区間の local に
  対して**区間の開始 parent** を返す決定的規則とする。
  `canonical_source_clock` の前方評価には影響しない。
- フレーム正本は有理数のまま。hold 区間の表示は同一
  ソース時刻を繰り返し、逆再生・time-remap 複合との
  組合せも型付きで検証する。

### 音声ポリシとの整合

- hold 区間の音声はソース時刻が進まないため、
  `ResampleV1` / `ReverseResampleV1` でも**無音**とする
  （サンプル率 0 のリサンプルは未定義のため）。
  `Reject` は従来通り計画時拒否。
- スピードランプ区間の音声は各区間の線形速度に従い
  `ResampleV1` で逐次リサンプルする。ピッチは区間速度に
  追従する既存意味を変えない。

### 共有 API

- 既存の `clip_time_set` が `time_map` を丸ごと受け取るため、
  スピードランプは piecewise 点列を同コマンドで設定する。
  ドメインがクリップの `timeline_range` を被覆し、local が
  ソース実尺に収まること、リンク・トランジション・
  保護区間との整合を計画時に検証し、違反は型付き拒否。
- 利便コマンド `clip_freeze { sequence, clip, at }` を追加する
  （at における freeze：該当位置で分割し hold 区間を持つ
  piecewise map を生成する等価操作として実装）。

## 影響

- 速度ランプ・freeze が共有 API・Undo・永続化・両描画経路で
  一貫する。GUI は M8 では速度ポイントの確認・基本編集まで。
- `PiecewiseTimeMap` の緩和は schema の記述変更を伴う。

## 関連

- NLE-006、ADR-0043（正有理数時刻）、AUDIO-004、
  `crates/kronello-time/src/mapping.rs`
