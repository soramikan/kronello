# AssetRow

Project パネルの素材一覧の 1 行で、素材の種類をアイコンとその色（`kind-*`）で示し、映像と音声を一目で見分けられるようにする。

## 利用側が渡すもの

- `kind`: `video` | `image` | `audio` | `composition` | `subtitle`（タイムラインの Clip と同じ区分）。
- 名前、仕様の要約（`1920×1080 · 23.976 fps`、`48 kHz · stereo`、`42 件`）、尺（タイムコード。尺のない静止画は「—」）。
- 状態: 通常 / 選択 / missing（`ASSET_MISSING`。ハッシュ不一致は `ASSET_HASH_MISMATCH`）。
- 選択・ダブルクリック（Viewer で開く）・ドラッグ（タイムラインへ配置）の処理。配置は離した時点で 1 コマンド。

## 見た目

- 高さ `row-height`、左右 `space-3`、4 列（アイコン 14px / 名前 / 仕様 / 尺）、列間 `space-2`。
- アイコンは種類ごとに `kind-video`（映像・静止画）、`kind-audio`、`kind-composition`、`kind-subtitle` で塗る。名前は `body` の `ink`、仕様は `caption` の `ink-muted`、尺は `timecode` の `ink-muted`。
- 選択は `selection-bg`、hover は `control-hover`。種類の色は選択中も変えない。
- missing は名前とアイコンを `danger` にし、アイコンを `triangle-alert` に替え、仕様の欄にエラーコードを mono で出す。

## 使い分け

- する: 種類はアイコンの形と色の両方で示す。色だけに頼らない。
- しない: 種類の色で文字を塗らない（種類の色は線・アイコン専用で、文字としてのコントラストを保証しない）。
- しない: 行全体や背景を種類の色で塗らない。色分けはアイコンだけの控えめなものにする。
