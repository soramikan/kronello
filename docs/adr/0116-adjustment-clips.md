# ADR-0116: アジャストメントクリップ（下位レイヤーへの一括適用）

- 状態: 採用
- 日付: 2026-10-08

## 背景

FX-007 は「下位レイヤーへのエフェクト一括適用」を要求する。
複数クリップにまたがるグレーディングやブラーを、個別の
クリップではなく 1 本のクリップとして管理する仕組みが
必要になる。新しい SourceRef バリアントか clip の種別か、
適用の評価意味を決める必要がある。

## 決定

### モデル

- `SourceRef` に `Adjustment` バリアントを追加する
  （ペイロードなし）。このバリアントを持つクリップを
  adjustment clip と呼ぶ。
- adjustment clip は `clip.effects` をその `timeline_range` に
  おける**直下の合成済み映像**へ適用する。評価は
  「そのクリップより下の全 video track の合成結果 →
  clip.effects を順に適用 → 上位トラックの合成を続行」。
- 制約（すべて型付き拒否）:
  - video track 上のみ有効。audio/caption track に置くと
    計画時拒否
  - adjustment clip 同士の「上の adjustment が下の
    adjustment を巻き込む」は許可し、適用順はトラック順
    に従う
  - retime（非線形タイムマップ）・音声リタイムは
    `audio_retime = Reject` 固定で、可変リタイムを掛けると
    拒否する
  - `clip.masks` や `clip.enabled = false` は従来通り機能し、
    disabled の adjustment clip は何も適用しない

### 共有 API

- `clip_place` は `SourceRef::Adjustment` を持つクリップを
  そのまま受け付ける。追加の専用コマンドは設けない。

## 影響

- 評価・描画では adjustment clip の範囲で下位スタックを
  グループ化し、その上に clip.effects を適用するため、
  bounds/halo は下位合成の union に効果分を加える。
- GUI はエフェクトパネルから「アジャストメントクリップを
  追加」し、Inspector はその clip.effects を通常通り編集する。

## 関連

- FX-007、ADR-0114（mask との併用）、ADR-0108、
  `crates/kronello-eval/src/sequence.rs`
