# ADR-0111: トラックロック・クリップ有効/無効・トラックターゲティング

- 状態: 採用
- 日付: 2026-10-08

## 背景

NLE-005 が要求する 3 つの編集状態がモデルに存在しない。
現在の GUI ロックは `ui.locked` のローカル状態であり、保存・
共有編集・CLI/MCP で一貫しない。「入口専用の作品状態を
作らない」の不変条件に従い、これらを作品ドキュメントに
永続化する。

## 決定

### モデル

- `TrackState` に `locked: bool` を追加する（`#[serde(default)]`
  で後方互換）。ロックは編集保護であり描画・音声評価に
  影響しない。`visible` / `muted` の意味は変えない。
- `Clip` に `enabled: bool` を追加する（`#[serde(default = 真,
  skip_serializing_if = 真)]`）。`enabled = false` のクリップは
  映像合成・音声ミックス・字幕出力から除外するが、
  タイムライン上の区間は占有し、自動リップルは起きない。
- `Sequence` に `targets: Option<TargetTracks>` を追加する。
  `TargetTracks { video: Option<TrackId>, audio: Option<TrackId> }`
  は種別ごとに 1 本の対象トラックを指し、ペースト・挿入・
  ギャップ編集の暗黙の対象を定める。参照先は存在し、
  種別が一致する必要がある。

### 共有 API（`TimelineCommand`）

- `track_state_set` は `TrackState` 全体を受け取るため、
  `locked` は同コマンドで更新できる。
- `clip_enable_set { sequence, clip, enabled }` を追加する。
- `sequence_targets_set { sequence, targets }` を追加する。
- ロックしたトラック上のクリップ・トラック自身への変更は
  計画時に `TRACK_LOCKED` で型付き拒否する。`edit.apply` は
  1 コマンドでも拒否があれば全体を適用しない。

## 影響

- GUI のローカル `ui.locked` は `TrackState.locked` の編集に
  置き換わり、保存・Undo・他クライアントで共有される。
- `sequence.query` / `sequence.json` / スキーマが新フィールドを
  返す。無効化クリップは描画・音声・字幕の全経路で除外される。

## 関連

- NLE-005、ADR-0001（共有 Command/Query API）、
  ADR-0110（シーケンスの永続編集状態）
