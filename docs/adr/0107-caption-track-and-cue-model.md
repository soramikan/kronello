# ADR-0107: 専用 caption track と版付き字幕キューモデル

- 状態: 採用
- 日付: 2026-10-07
- 関連: SUB-001、SUB-002、TEXT-002、ADR-0010、ADR-0038、ADR-0100

## 背景

字幕・キャプションは NLE-002 の範囲外として保留されており、sequence には
Video / Audio の 2 種類の track しか存在しない。SRT / WebVTT / ITT の
入出力（SUB-002）とタイムライン上の字幕編集（GUI-009）を実現するには、
字幕を第一級の編集対象としてモデル化する必要がある。

検討した代替は次の 2 つである。

1. 字幕を通常の text clip として video track に置く
   （既存 `SceneContent::Text` の流用で実装量は最小）。
2. 字幕専用の track kind とキューモデルを導入する。

1 は「字幕は映像のレイヤー」という仮定を文書構造に埋め込み、
サイドカー書き出し・字幕規格のキュー制約（開始/終了の半開区間、
位置・領域の意味）を sequence の検証規則で表せない。
また text clip との区別が付かず、字幕だけのエクスポートが
構造的に記述できない。よって 2 を採用する。

## 決定

- `TrackKind` に `Caption` を追加する。caption track は
  音声ミックスの対象外で、描画は video track の合成順の
  最上位（全 video track の上）として `lower_sequence` が
  caption キューをテキストノードへ変換する。
- `SourceRef` に `Caption { caption: CaptionId }` を追加し、
  字幕キュー本体は `Project.captions` の
  `DocumentObject<CaptionDocument>` として保持する。
  clip はキューのタイムライン配置（`timeline_range` / `source_in`
  は使わず、キューの表示区間を `timeline_range` で直接表す。
  `source_in` は `Time::ZERO` 固定、`time_map` は恒等）を保持する。
- `CaptionDocument` は版付き（`version: u32`、v1 のみ採用）で、
  次を持つ。
  - `text`: キュー本文。改行は `\n`。素材中の文字列は
    データであり命令として解釈しない（不変条件の維持）。
  - `styles`: cue 内の装飾 span のリスト。v1 では cue 全体に
    適用する単一スタイルと、span 指定の `bold` / `italic` /
    `color` / `font` を許す。未知の span 属性は型付き拒否。
  - `placement`: `CaptionAnchor`（bottom/top/center 等の
    9 方向 anchor）+ safe area 内のオフセット（有理数比率）。
  - `format`: `srt` / `vtt` / `itt` の出自を示す任意フィールド。
    書き出し時の既定フォーマット選択に使う。
- 文字装飾（フォント・サイズ・色・縁取り・背景）は
  `CaptionDocument.style: CaptionStyle` として表し、
  caption track 上の clip の `properties` でオーバーライド
  可能とする。装飾値は評価可能な `Property` 規約に従い、
  時刻による変化は v1 では許さない（定数のみ）。
- caption clip 同士は同一 track 内で重ならない
  （`Sequence::validate` で `[start, end)` の交差を拒否）。
  caption clip は他 kind の track に置けず、
  逆に caption track には `Caption` 以外の `SourceRef` を置けない。
- `ClipQuery` の `ClipKind` に `Caption` を追加し、
  `sequence.query`・`edit.plan`/`edit.apply`・CLI/MCP で
  既存の編集コマンドがそのまま使える。
- 未対応の `version`・未知フィールドは `deny_unknown_fields` と
  `UnsupportedMeaning` の既存規約で型付き拒否する。

## 影響

- `TrackKind`・`SourceRef`・`ClipKind`・`Project` に
  後方互換な追加が入る。serde default により旧文書はそのまま開ける。
- `lower_sequence` が caption track を処理する。音声ミックスの
  対象判定は `TrackKind` で行い、caption を除外する。
- SRT/VTT/ITT のパーサは `CaptionDocument` の v1 機能集合へ写像し、
  写像できない要素（VTT の voice/region 設定、ITT の高度な
  スタイル継承等）は `UNSUPPORTED_FEATURE` の型付き拒否とする。
  黙った要素落ちはしない（SUB-002）。
- 字幕描画は `kronello-text` の既存レイアウトを再利用するが、
  キューの anchor/region 解決は sequence の extent に対する
  安全域規則として `lower_sequence` で行う。

## 関連

- [01-data-model](../architecture/01-data-model.md)
- ADR-0010（Command/Query API）、ADR-0038（序列モデル）、
  ADR-0100（共有編集）
