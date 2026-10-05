# InspectorRow

1 つの Property を 1 行で表し、主値源（`PropertySource` の `Constant` / `Curve` / `Expression`）をキーフレームナビゲータの形で示す Inspector の行。

## 利用側が渡すもの

- `label`（Property 名。英語の型名のまま: `Position`、`Opacity`）。
- `source`: `constant` | `curve` | `expression`、`curve` のときは `onKeyframe`（現在時刻にキーフレームがあるか）。
- 値フィールド（NumberField を 1〜4 個。複数成分は `X` / `Y` などの `caption` ラベルを前に置く）。
- 任意: `error`（エラーコードと日本語の説明）、`selected`。
- キーフレーム操作の処理: 前へ・次へ・現在時刻に追加 / 削除。いずれも Command を 1 回発行する。

## 主値源の見せ方

| source | ナビゲータ | 意味 |
|---|---|---|
| `constant` | 中空のひし形のみ（`line-strong` の枠） | 押すと最初のキーフレームを打ち、`curve` になる |
| `curve`・キー上 | ◀ 塗りのひし形（`ink`） ▶ | 現在時刻にキーフレームがある。押すと削除 |
| `curve`・キー間 | ◀ 中空のひし形 ▶ | 前後の矢印があることがアニメーション中の印。押すと追加 |
| `expression` | `fx` バッジ | 値は評価結果の表示のみで、スクラブできない |

矢印の有無とひし形の塗りの両方で状態を表し、色だけに頼らない。ひし形の形は Keyframe の補間の形とは独立で、ナビゲータでは常に Linear のひし形を使う。

## 見た目

- 高さ `row-height`、3 列（ナビゲータ 48px / ラベル / 値）、列間 `space-2`、右余白 `space-3`。
- ラベルは `label` スタイルの `ink`。hover で `control-hover`、選択で `selection-bg`。
- エラーは値フィールドを error 状態にし、行の下に `danger` のアイコン + エラーコード（mono）+ 日本語の説明を出す。最終レンダーはこの状態で黙って続行しない。

## 使い分け

- する: セクション見出し（`Transform` など）は `heading` で行の上に置き、行は字下げしない。
- しない: アニメーション中であることを値の色（赤・青など）で示さない。選択色の `selection` を状態表示に流用しない。
- アニメーションしない設定（書体・太さ・文字揃え・表示する bounds など）は設定行（`KRInspectorSettingRow`）に置く。ナビゲータの 48px 列を空けたままにし、ラベルを Property の行と同じ列に揃える。値の列には PopupButton・SegmentedControl・TextField を置く。Popover の行を Inspector に流用しない。
