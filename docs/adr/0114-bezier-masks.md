# ADR-0114: ベジェマスク（クリップ適用・フェザー・拡張・時間変化）

- 状態: 採用
- 日付: 2026-10-08

## 背景

FX-004 はクリップへのベジェマスク適用・フェザー・拡張・
時間変化を要求する。`ValueType::Path` は既存であり、
マスクを「新しい effect kind」にするか「クリップの
専用フィールド」にするかを決める必要がある。

## 決定

### モデル

- `Clip` に `masks: Vec<Mask>` を追加する
  （`#[serde(default, skip_serializing_if = "Vec::is_empty")]`）。
  effect ではなくマスク固有のスタックとし、effect 適用前に
  クリップのアルファへ作用する。
- `Mask`:
  - `id: MaskId`（新規 ID 型）
  - `path: PropertyId` — `ValueType::Path` のプロパティを
    参照し、時間変化は既存の keyframe/curve で表現する
  - `mode: MaskMode`（`add | subtract | intersect | difference`）
  - `feather: PropertyId`（design_px、非負）
  - `expansion: PropertyId`（design_px、正負可）
  - `opacity: PropertyId`（0..=1）
  - `invert: bool`（`#[serde(default)]`）
  - `closed: bool`（パス閉合。`#[serde(default = 真)]`）
- マスク数の予算（例 64/clip、点数合計の上限）を型付きで
  検証し、超過は `MASK_BUDGET_EXCEEDED` 相当で拒否。

### 評価

- クリップの描画後・clip effects 適用前に、マスクを
  ラスタライズしてアルファを乗算する。ラスタライズは
  kronello-vector のフラット化を再利用し、feather は
  マスク境界のぼかし、expansion はパスのオフセットで
  実装する。
- CPU/GPU 経路で同じカバレッジ規約（頂点単位の再現性）を
  持ち、時間変化する path は固定時刻で評価される。

### 共有 API

- `clip_masks_set { sequence, clip, masks, properties }` で
  マスクスタックを原子置換する（`clip_set_effects` と同じ
  計画/適用/Undo の枠組み）。

## 影響

- GUI のベジェ編集は既存の SpatialPath 系基盤を流用し、
  Inspector にマスク一覧・追加・パラメータ編集を接続する。
- マスクは描画境界（bounds/halo）を拡張しないが、
  expansion>0 は外側に効果を及ぼすため bounds 計算に含める。

## 関連

- FX-004、ADR-0108、VEC-001（形状 IR）、
  `crates/kronello-model/src/matte.rs`（matte とは別物: matte は
  ノード間関係、mask はクリップ内アルファ）
