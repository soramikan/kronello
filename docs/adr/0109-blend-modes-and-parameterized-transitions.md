# ADR-0109: ブレンドモード一式とパラメータ付きトランジション（wipe / slide / dip）

- 状態: 採用
- 日付: 2026-10-07
- 関連: FX-003、ADR-0101、ADR-0038

## 背景

`BlendMode` は `normal` / `multiply` / `screen` の 3 種、
`TransitionKind` は `crossfade` の 1 種のみで、標準的な
合成・継ぎ目表現が不足している（FX-003）。
既存の合成規約（ADR-0101）では線形 premultiplied 値域の
source-over が定義済みであり、ここへ標準ブレンド関数と
トランジション種別を追加する。

## 決定

### ブレンドモード

`BlendMode` に W3C Compositing and Blending 準拠の
separable / non-separable モードを加え、計 18 種とする。
wire 名はすべて snake_case。

- separable（各チャンネル独立の B(cb,cs)）:
  `darken`, `lighten`, `color_dodge`, `color_burn`,
  `hard_light`, `soft_light`, `difference`, `exclusion`,
  `overlay`, `linear_dodge`, `linear_burn`, `vivid_light`,
  `linear_light`
- non-separable（色相・彩度・輝度の合成）:
  `hue`, `saturation`, `color`, `luminosity`
- 既存: `normal`, `multiply`, `screen`

定義規約:

- 演算は ADR-0101 の線形 premultiplied 値に対して行う。
  separable モードは straight 化（alpha 除算）した
  cb/cs に B を適用し、結果を `αo = αs + αb(1-αs)` の
  標準合成式 `co = (1-αs)cb + (1-αb)cs + αsαb B(cb,cs)`
  で合成する（W3C spec の composite 式）。
- HDR（1.0 超）・負値は clamp しない。B の出力が
  定義域を外れた場合も値をそのまま保持する。
- CPU（`kronello-gpu/src/color.rs` の共通実装）と
  WGSL（GPU shader operation）は同一の式を実装し、
  operation id を固定表で割り当てる。
- `BLEND_KEY` の単一 property・Constant のみ・重複拒否の
  規約は変更しない。

### トランジション

`TransitionKind` に `Wipe`・`Slide`・`Dip` を追加し、
`Transition` に `params: Option<TransitionParams>`
（serde default、kind ごとの版付き構造体）を追加する。
`version` は kind ごとに独立採番とし、v1 から開始する。

- `Wipe { direction: TransitionDirection }`:
  incoming clip が direction 側から矩形マスクで
  徐々に表示される。v1 は hard edge（feather なし）。
- `Slide { direction: TransitionDirection }`:
  incoming clip 全体が direction 側から平行移動して
  入る。outgoing clip は静止。
- `Dip { color: Color }`:
  区間前半で outgoing → `color` へ、後半で
  `color` → incoming へ線形 crossfade。
- `TransitionDirection` は `left` / `right` / `up` /
  `down` の閉集合。

描画規約:

- crossfade 同様、overlap 区間内の両クリップを
  それぞれ合成し、kind ごとの重み・マスク・変換を
  適用する。`lower_sequence` の出力に
  transition 適用ノードを明示する。
- 音声は本 ADR では crossfade（等パワーではなく線形
  ゲイン）のみとし、kind によらず区間内で
  outgoing→incoming の線形クロスフェードを行う。
  音声トランジション種別の拡張は後続タスク。
- `Sequence::validate` の「overlap は transition 宣言済み
  区間のみ許可」規則は kind によらず適用する。

## 影響

- `Transition` の `params` 追加は serde default により
  後方互換。旧文書の crossfade（params なし）は
  `Crossfade` kind として従来通り解釈する。
- 未対応の `version`・未知の `kind` / direction は
  型付き拒否。`Effect::Opaque` と同様、Transition の
  未知フィールドは `deny_unknown_fields` で保護する。
- CPU/GPU 両経路に描画実装が必要。`snapshot.rs` の
  crossfade 処理を kind 別に拡張する。
- GUI は `transition_set` コマンドの kind/params を
  そのまま編集できる。

## 関連

- ADR-0101（線形 premultiplied 合成）、ADR-0038（序列）、
  [01-data-model](../architecture/01-data-model.md)
