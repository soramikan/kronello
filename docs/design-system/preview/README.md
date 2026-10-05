# デザインシステムの見本

状態: M3 の GUI 実装前に作成（2026-10-05）。ネイティブ実装の画面ではなく、仕様を HTML / CSS で描いた参照用の見本である。

[デザインシステム](../README.md)の値・部品・画面を、ブラウザで開ける HTML と、そこから撮った PNG で再現する。値はすべて [tokens.json](../tokens.json) から生成した CSS を参照し、書体（Noto Sans JP / Noto Sans Mono）とアイコン（Lucide 1.51.0）も同梱したものを使うため、ネットワークなしで同じ見た目になる。

## 位置付け

- 正本は仕様の文書（[README](../README.md)、[components/](../components/)、[screens/](../screens/README.md)）と [tokens.json](../tokens.json) である。見本と食い違うときは仕様を正とし、見本を直す。
- 見本は各 OS の実装（SwiftUI / AppKit、WinUI 3、GTK4）が目指す見た目の基準として使う。画素単位の一致は求めない（ブラウザとネイティブで文字の描画が異なる）。
- 仕様に書かれていない細部（segmented の選択中の表し方、素材の代わりに描いた背景の画など）は見本のための仮の描き方で、仕様ではない。決める場合は仕様の文書に書いてから見本を合わせる。

## 開き方

HTML をブラウザで直接開く（`file://` で動く。サーバーは要らない）。

| ページ | 内容 |
|---|---|
| [index.html](index.html) | 基礎: 色とコントラスト、文字、余白・寸法・角丸・影、アイコン、画面の一覧 |
| [components.html](components.html) | 22 のコンポーネントを状態ごとに並べたもの |
| [screens/](screens/) | 1440×900 の画面: `welcome` `edit` `motion` `template` `export` |

- `t` キーか右上のボタンでテーマ（Dark / Light）を切り替える。URL の `?theme=light` でも指定できる。
- 画面の状態は `?state=` で切り替える。複数はカンマで区切る。

| ページ | `state` | 内容（[states.md](../screens/states.md)） |
|---|---|---|
| `screens/motion.html` | `curve` | 下段を Curve editor にする |
| `screens/motion.html` | `undo-conflict` | `UNDO_CONFLICT` のシート |
| `screens/motion.html` | `revision-conflict` | `REVISION_CONFLICT` のバナー |
| `screens/motion.html` | `adapter-unavailable` | Viewer の `ADAPTER_UNAVAILABLE` |
| `screens/motion.html` | `evaluation-error` | `EVALUATION_ERROR` と `FONT_MISSING` |
| `screens/motion.html` | `selection-deleted` | 選択中のレイヤーが外部で削除された（GUI-001 で確定する提案） |
| `screens/motion.html` | `safe-mode` | 安全モードの帯 |
| `screens/edit.html` | `asset-missing` | `ASSET_MISSING` の再リンクのシート |

## 画像

[images/](images/) は HTML を headless Chrome で 2 倍の解像度で撮ったもの。仕様の各文書に埋め込んでいる。

- `images/foundations/*.png`: 基礎の各節（Dark。色とコントラストは両テーマを並べてある）
- `images/components/<Component>-{dark,light}.png`: コンポーネントごと
- `images/screens/<page>-{dark,light}.png`: 画面ごと。状態（`state-*.png`）は既定の Dark だけ

## 構成

| パス | 内容 | 生成 |
|---|---|---|
| `assets/tokens.css` | tokens.json の CSS カスタムプロパティ（`--surface-0`、`--font-body` など） | 生成 |
| `assets/tokens.js` | 基礎ページが表を描くための tokens.json の写し | 生成 |
| `assets/icons/*.svg` | [icons.md](../icons.md) に載せた Lucide の SVG（改変しない）と LICENSE | 取得 |
| `assets/icons.js` | 上の SVG をインラインで描くための表 | 生成 |
| `assets/fonts/` | Noto Sans JP / Noto Sans Mono のサブセット（woff2）、OFL、収録文字の一覧 | 生成 |
| `assets/kit.css` | コンポーネントのスタイル。値はすべてトークンを参照する | 手書き |
| `assets/screens.css`、`assets/docs.css` | 画面の配置と、見本ページの枠 | 手書き |
| `assets/preview.js` | テーマ・状態の切り替え、アイコンの埋め込み、撮影用の寸法の報告 | 手書き |

## 変更の手順

[scripts/design_preview.py](../../../scripts/design_preview.py) で生成と検査を行う。

```bash
python3 scripts/design_preview.py render
python3 scripts/design_preview.py check
```

- tokens.json を変えたら `render` で `tokens.css` / `tokens.js` を作り直す。`check` は生成物が古いとき、HTML が icons.md にないアイコンを使っているとき、フォントのサブセットにない文字を使っているときに失敗する（CI でも実行する）。
- icons.md にアイコンを足したら `vendor-icons` で固定版の lucide-static から SVG を取り直す（ハッシュを検証する）。
- 見本に新しい文字を書いて `check` が失敗したら `fonts` でサブセットを作り直す。fontTools と brotli が要る（`pip install 'fonttools[woff]'`）。元のフォントは google/fonts の固定コミットからハッシュを検証して取得する。Noto にない記号（`⌥`、`⌫` など）はサブセットに入らないため、`fonts` が失敗して知らせる。
- 見た目を変えたら `screenshots` で画像を撮り直す。Google Chrome か Chromium が要る（`CHROME` で場所を指定できる）。引数なしは全部を撮り、使われなくなった画像を消す。`screenshots screens`、`screenshots motion Button` のように群や名前で絞れる。

```bash
python3 scripts/design_preview.py screenshots
```

- コンポーネントや画面を仕様に追加したら、`components.html`（`data-section` の節）か `screens/` に見本を足し、仕様の文書に画像を埋め込む。
