# TextField

名前・パス・検索語など文字列を入力する 1 行の欄で、ラベル・補足・エラーを上下に添えられる。数値は NumberField を使う。

## 利用側が渡すもの

- ラベル（上に置く）、値、任意でプレースホルダ・補足・検索用のアイコン。
- 検証結果: エラーコードと日本語の説明。
- 確定の処理。入力中（IME の未確定文字列を含む）は作品に書き込まず、確定（Enter・フォーカス移動）で Command を 1 回発行する。

## 見た目

- 高さ `control-height`、`surface-200` + 1px `line-strong`、角丸 `radius-sm`、左右 `space-2`、文字は `body`、プレースホルダは `ink-muted`。
- フォーカスは `selection` の 2px リング。検索は左に 12px の `search` アイコン。
- ラベルは `label`、補足は `caption` の `ink-muted`、エラーは枠を `danger` にし、下に `triangle-alert` + mono のコード + 説明（`danger`）。
- 無効は不透明度 0.45。

## 使い分け

- する: 日本語 IME の変換中は検証もエラー表示もしない。確定してから検証する。
- しない: 入力した文字列をシェルや FFmpeg の引数として扱わない。値はデータとして API に渡す。
