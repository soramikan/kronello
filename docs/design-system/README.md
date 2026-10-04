# デザインシステム

状態: M3 の GUI 実装前に策定（2026-10-04）。ネイティブ実装による検証は未了。

Kronello の UI は素材が主役の作業場として組む。見た目は macOS に寄せつつ、コントロール・フォント・アイコンはすべて自前で持ち、macOS・Windows・Linux で同じ姿にする。各 OS の標準コントロールやシステムフォントの見た目には委ねない。

## 位置付け

- このディレクトリが Kronello の GUI の見た目と操作の正本である。値の正本は [tokens.json](tokens.json)、規則はこの文書、部品ごとの仕様は [components/](components/) に置く。
- 方針の決定と理由は [ADR-0054](../adr/0054-gui-design-system.md)。GUI の構成と FFI 境界は [10 デスクトップ GUI](../architecture/10-desktop-gui.md)。
- macOS（SwiftUI / AppKit）、Windows（WinUI 3）、Linux（GTK4）の各実装は、標準コントロールをこのシステムの見た目にスタイルして使う。IME・アクセシビリティ・キーボード操作は各フレームワークの仕組みをそのまま使う。
- 例外として、OS が描くもの（macOS のメニューバー、ファイルの選択・保存ダイアログ、ウインドウの枠と信号機ボタン、システムの通知）は OS のものを使う。このシステムが対象にするのはウインドウの中身と、アプリ内で開くメニュー・ポップオーバー・シートである。
- トークン名は英語の識別子で、各実装の色・寸法の定数名にそのまま対応させる。値を実装側で直接書き換えない。

## 原則

- **素材より目立たない。** UI の彩度を落とし、強い色の意味を 3 つに限る。琥珀 = 「今」とブランド、青 = 選択、赤 = 型付きエラー。種類の色（`kind-*`）はアイコンと細い下線にだけ使う控えめな補助。
- **全 OS で同一。** 寸法・色・書体・アイコン・並び・挙動はこのシステムが正本。macOS の慣習（ダイアログのボタン順、22px のコントロール、控えめな角丸）を基準にし、Windows / Linux でも同じにする。修飾キーだけは ⌘ ↔ Ctrl、⌥ ↔ Alt と読み替える。
- **状態は色だけで伝えない。** 形・アイコン・文言を必ず添える（キーフレームの形、種類のアイコン、エラーコード、破線）。

## 色

- テーマは **Dark（既定）** と **Light**。初回起動はシステムの外観に追従せず Dark で開き、設定で Light に切り替える。両テーマとも同じトークン名を使う。
- 地は 3 段: `surface-0`（ビューアの周囲・キャンバスの背後）→ `surface-100`（パネル）→ `surface-200`（トラックのレーン・入力欄・secondary ボタン）。hover の塗りは `control-hover`。
- 区切りは影ではなく 1px の線で引く。パネル間・行間の区切りは `line`（装飾）、操作できるものの枠は `line-strong`（どの地でも 3:1 以上）。
- 文字は `ink`、単位・非アクティブ・目盛り・未選択のキーフレームは `ink-muted`。
- **琥珀（ブランド色）**: 塗りは `accent`（その上の文字は `on-accent`）、文字・細線は `accent-ink`。再生ヘッド、現在時刻、主ボタンだけに使う。Light では生の琥珀が明るい地で読めないため、`accent-ink` を濃くしてある。
- **青（選択）**: 選択中のクリップ・レイヤー・キーフレーム・行は `selection`（線・塗り）と `selection-bg`（行・クリップの塗り）。フォーカスリングも `selection` の 2px 実線（オフセット 1px）で、全コントロール共通。メニューの現在項目（`selection` の塗り + `on-selection`）、チェックボックス・ラジオのオン、ドロップ先の強調も「選ばれている」ものとして青にする。
- **赤（エラー）**: `danger` は `UNSUPPORTED_FEATURE`、`ASSET_MISSING`、`UNDO_CONFLICT` などの型付きエラー専用。アイコンとエラーコードを必ず添える。
- **種類の色**: 素材とクリップの種類を `kind-video`（映像・静止画）、`kind-audio`、`kind-composition`、`kind-subtitle`、`kind-generator`、`kind-adjustment` で示す。Project パネルではアイコンの色、タイムラインではアイコンの色 + クリップ下端 2px の下線（調整だけ破線）。琥珀・青・赤と紛れない色相にしてある。文字や面の塗りには使わない。
- クリップの塗りは種類に関係なく `clip`。

## 文字

- 書体は Noto で統一し、アプリに同梱する（SIL OFL）。UI は `sans`（**Noto Sans JP** — ラテン文字も含む）、数値は `mono`（**Noto Sans Mono**、和文は Noto Sans JP にフォールバック）。版はアプリの同梱時に固定する。
- 既定は `body`（12px）。パネル見出しは `heading`、Property 名・タブ・クリップ名は `label`、単位や補足は `caption`、空状態とダイアログ題だけ `display`。
- タイムコード・フレーム番号・数値フィールドの値は `timecode`、ルーラーの目盛りは `ruler`。スクラブ中に桁が動かないよう等幅・桁揃えにする。
- 時刻は浮動小数点で見せない。タイムコード（`00:01:23:12`）か秒 + フレーム（`1s12f`）で表す。

## 余白・サイズ・角丸・影

- 余白は 4px 刻みの `space-1`〜`space-4`。パネル内側は `space-3`、セクション間は `space-4`。
- コントロールの高さは `control-height`（22px）、行は `row-height`（24px）、パネル見出しとタブ帯は `panel-header-height`（28px）、トラックは `track-height`（36px）、ルーラーは `ruler-height`（20px）、キーフレームは `keyframe-size`（9px）。
- 角丸はクリップ・入力欄・再生ヘッドのつまみに `radius-sm`、ボタン・メニュー・ポップオーバーに `radius-md`、シート・ダイアログ・HUD に `radius-lg`。パネルは角丸にしない。
- 影はポップオーバー・メニュー・ダイアログ・HUD の `shadow-popover` だけ。パネルは影を落とさない。

## 操作と状態

- hover は `control-hover`、押下は一段暗く、disabled は不透明度 0.45。
- ドラッグ（数値のスクラブ、クリップの移動、キーフレームの移動）中は候補表示で追従し、離した時点で Command を 1 回だけ発行する。Undo も 1 回で戻る。
- 外部（CLI / MCP）からの変更は自動で再読込し、StatusBar に 1 行で残す。表示が変わっても色の意味は変えない。
- 進捗バーは地 `line-strong`、進んだ分 `ink` の中立色。琥珀・青を使わない。
- 動きは最小限にする。回転する `loader-circle` だけを使い、`prefers-reduced-motion`（OS の視差効果を減らす設定）では止める。パネルやメニューの開閉にアニメーションを付けない。
- オーバーレイは 3 種に分ける: 選んで実行する Menu、その場で数項目を調整する Popover（確定ボタンなし）、作業を止めて判断を求める Dialog。

## アイコン

- **Lucide**（ISC）だけを使う。使用中のアイコンと用途の対応は [icons.md](icons.md) にある。ここにないものが要るときも Lucide から選んで追加し、他のセットや絵文字を混ぜない。
- 24px グリッド・stroke-width 2 のまま、ツールバーは 14px、行内・クリップ内は 12px に縮小して描く。インクは `currentColor` で、親の文字色（`ink-muted`、hover で `ink`、エラーは `danger`、種類は `kind-*`）を継ぐ。
- キーフレームのナビゲータの小さな三角（◀ ▶）と、キーフレーム自体の形（◆ ● ■）はアイコンではなく部品の一部として描く。

## 文言

- UI の文言は日本語を正本とし、Property 名・パネル名・型名・エラーコードは英語のまま表示する（`Position`、`Inspector`、`ASSET_MISSING`）。
- ボタンは動詞で終える（「書き出す」「再リンク」）。続けて選択や設定が開くものは「…」を付ける。
- エラーはコード + 何が起きたか + 次にできること、の順で 1〜2 文。素材のファイル名や字幕は文章に混ぜず、データとして詳細欄に出す。

## トークン

値は [tokens.json](tokens.json) から転記したもの。食い違う場合は tokens.json を正とする。テーマは Dark（`dark`） / Light（`light`） で、最初の Dark が既定。

### 色

| トークン | Dark | Light | 用途 |
|---|---|---|---|
| `surface-0` | `#0e0f11` | `#dedfe2` | ビューアの周囲とキャンバスの背後。両テーマで最も奥の地で、素材を中立の沈んだ地の上で判断できるようにする。 |
| `surface-100` | `#16171a` | `#ececee` | パネルの地: Project、Inspector、Timeline、Dope sheet。キーフレームのレーンも。 |
| `surface-200` | `#202125` | `#ffffff` | surface-100 の上に一段上がる行とコントロール: トラックのレーン、数値フィールド、secondary ボタン、ポップオーバー。 |
| `control-hover` | `#2a2c31` | `#f4f4f6` | secondary / plain ボタンと行の hover の塗り。 |
| `line` | `#3a3c42` | `#d6d7db` | パネル・トラック・行の間の 1px の区切り。装飾用（3:1 未満）で、操作できる部品の唯一の境界にはしない。 |
| `line-strong` | `#74777f` | `#7c7e85` | コントロールの 1px の枠（数値フィールド、secondary ボタン）と中空のキーフレーム。両テーマで surface-0/100/200 と control-hover に対し 3:1 以上。 |
| `ink` | `#e9e7e4` | `#1d1d1f` | 主な文字とアイコン。すべての地、clip、selection-bg の上で 9:1 以上。 |
| `ink-muted` | `#9b9ea6` | `#5f6168` | 補助の文字と未選択のキーフレーム: 単位、非アクティブのタブ、ルーラーの目盛り、プレースホルダ。両テーマで surface-0/100/200 と selection-bg に対し 4.6:1 以上。 |
| `accent` | `#f0a33a` | `#f0a33a` | ブランドの琥珀の塗り: 主ボタン、再生ヘッドのつまみ、現在時刻のバッジ。上には必ず on-accent の文字を置く。「今」と Kronello を表し、選択には使わない。 |
| `accent-ink` | `#f0a33a` | `#8a4f00` | 文字・細線としての琥珀: 再生ヘッドの線、現在時刻のタイムコード。両テーマで surface-0/100/200 に対し 4.9:1 以上。生の琥珀は明るい地で 1.8:1 しかないため、Light では濃くする。 |
| `on-accent` | `#1b1206` | `#1b1206` | accent の塗りの上の文字とアイコン（8.8:1）。 |
| `selection` | `#4c9bff` | `#0a5bc0` | 選択されているもの: 選択中のクリップ・レイヤーの 2px の枠、選択中のキーフレームの塗り、フォーカスリング、メニューの現在項目、チェックのオン。両テーマで clip とすべての地に対し 3.8:1 以上。 |
| `selection-bg` | `#1c3557` | `#d2e3fb` | 選択中の行とクリップの塗り。ink（10:1）と ink-muted（4.6:1）が読める。 |
| `on-selection` | `#06121f` | `#ffffff` | selection の塗りの上の文字とアイコン（5.6:1 以上）。 |
| `clip` | `#383b42` | `#c5c8cf` | トラック上のクリップの通常の塗り。上の ink の名前は 9:1 以上。 |
| `kind-video` | `#4fbfae` | `#1f7a6d` | 映像の素材とクリップ（静止画も同じ）: 種類のアイコンとクリップの 2px の下線。青緑。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `kind-audio` | `#92c75e` | `#4c7a1c` | 音声の素材とクリップ: 種類のアイコンとクリップの下線。緑。kind-video とは色相とアイコンの両方で区別する。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `kind-composition` | `#b596f2` | `#7048c8` | Composition の素材、Composition を参照するクリップ、CompositionInstance のレイヤー。紫。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `kind-subtitle` | `#ec8fb5` | `#b03d6c` | 字幕のクリップと字幕の素材。ローズ。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `kind-generator` | `#cfc55a` | `#6e660a` | Generator のクリップ（単色、ノイズ、グラデーションなど）。黄オリーブで、accent の琥珀から離してある。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `kind-adjustment` | `#b4ada2` | `#6e685e` | 下のトラックに効果をかける調整クリップ。媒体を持たないため、温かいグレーの破線の下線にする。 線とアイコン専用で文字には使わない。両テーマで clip、selection-bg、すべての地に対し 3:1 以上。 |
| `danger` | `#ff7a66` | `#b3261a` | UNSUPPORTED_FEATURE、ASSET_MISSING、UNDO_CONFLICT などの型付きエラーの文字・アイコン・破線の枠。必ずアイコンとエラーコードを添え、色だけで伝えない。clip に対し 3.9:1 以上、すべての地に対し 4.9:1 以上。 |

### 文字

書体: `sans` = "Noto Sans JP", "Noto Sans", sans-serif、`mono` = "Noto Sans Mono", "Noto Sans JP", monospace

| スタイル | 書体 | サイズ / 行送り | 太さ | 用途 |
|---|---|---|---|---|
| `display` | `sans` | 22px / 28px | 600 | 空状態、ウェルカムウインドウ、ダイアログの題だけ。 |
| `heading` | `sans` | 13px / 16px | 600 | パネルの見出しと Inspector のセクション見出し。 |
| `body` | `sans` | 12px / 16px | 400 | 既定の UI 文字: 一覧、レイヤー名、メニュー、ボタンのラベル（ボタンは太さ 500）。 |
| `label` | `sans` | 11px / 14px | 500 | Property 名、タブ、クリップ名、トラック名。 |
| `caption` | `sans` | 10px / 13px | 400 | 単位、ツールチップの 2 行目、ステータスバーの補足。 |
| `timecode` | `mono` | 12px / 16px | 500 | タイムコード、フレーム番号、数値フィールドの値。スクラブ中に桁が動かないよう等幅にする。 |
| `ruler` | `mono` | 10px / 12px | 400 | 時間ルーラーの目盛りとカーブエディタの軸の値。 |

### 余白

| トークン | 値 | 用途 |
|---|---|---|
| `space-1` | `4px` | アイコンとラベルの間、キーフレームナビゲータ内の間隔。 |
| `space-2` | `8px` | 行内のコントロール同士の間隔、数値フィールドの左右の内側。 |
| `space-3` | `12px` | パネルの内側、ボタンの左右の内側、階層 1 段の字下げ。 |
| `space-4` | `16px` | Inspector のセクション間とダイアログの内側。 |

### 寸法

| トークン | 値 | 用途 |
|---|---|---|
| `control-height` | `22px` | ボタン、数値フィールド、icon ボタンの高さ（macOS の regular の高さ）。 |
| `row-height` | `24px` | Inspector、レイヤー一覧、Dope sheet、ステータスバーの行の高さ。 |
| `panel-header-height` | `28px` | パネルの見出しとタブ帯の高さ。 |
| `track-height` | `36px` | タイムラインのトラックの既定の高さ。クリップは上下 2px ずつ内側に置く。 |
| `keyframe-size` | `9px` | キーフレームの記号の外接寸法。当たり判定は四方に space-1 を足す。 |
| `ruler-height` | `20px` | トラックとレーンの上の時間ルーラーの高さ。 |
| `toggle-size` | `14px` | チェックボックスとラジオの箱、スライダーのつまみ。 |

### 角丸

| トークン | 値 | 用途 |
|---|---|---|
| `radius-sm` | `3px` | トラック上のクリップ、数値フィールド、チェックボックス、再生ヘッドのつまみ。 |
| `radius-md` | `6px` | ボタン、segmented、ポップオーバー、メニュー。 |
| `radius-lg` | `10px` | シート、ダイアログ、ウェルカムウインドウ、HUD。 |

### 影

| トークン | 値 | 用途 |
|---|---|---|
| `shadow-popover` | dark: `0 8px 24px #00000080, 0 0 0 1px #3a3c42`<br>light: `0 8px 24px #0000002e, 0 0 0 1px #d6d7db` | ポップオーバー、メニュー、ダイアログ、HUD の唯一の影。パネルは影を落とさない。 |

## コンポーネント

| 分類 | コンポーネント | 概要 |
|---|---|---|
| Controls | [Button](components/Button.md) | コマンドを 1 回発行する押しボタンで、primary・secondary・plain・destructive の 4 種と、アイコンだけの icon 形を持つ。 |
| Controls | [NumberField](components/NumberField.md) | 横ドラッグで値をスクラブし、クリックで直接入力に切り替わる数値フィールドで、Inspector の値入力の基本部品。 |
| Controls | [Checkbox](components/Checkbox.md) | オン / オフ（とその混在）を切り替えるチェックボックスと、排他の選択肢から 1 つを選ぶラジオボタン。 |
| Controls | [TextField](components/TextField.md) | 名前・パス・検索語など文字列を入力する 1 行の欄で、ラベル・補足・エラーを上下に添えられる。 |
| Controls | [PopupButton](components/PopupButton.md) | 決まった選択肢から 1 つを選ぶボタンで、押すと Menu が開き、現在の値をボタン上に表示する。 |
| Controls | [Slider](components/Slider.md) | 範囲のある数値（音量、不透明度など）を大まかに合わせるスライダーで、正確な値の入力用に NumberField と組で置く。 |
| Inspector と階層 | [InspectorRow](components/InspectorRow.md) | 1 つの Property を 1 行で表し、主値源（`PropertySource` の `Constant` / `Curve` / `Expression`）をキーフレームナビゲータの形で示す Inspector の行。 |
| Inspector と階層 | [LayerRow](components/LayerRow.md) | Composition の階層（SceneNode の木）の 1 行で、containment parent による字下げと、別に持つ transform parent を併記する。 |
| Timeline | [Track](components/Track.md) | Timeline の 1 トラック（ヘッダ + レーン）と、その上に置く Clip・時間ルーラー・再生ヘッドをまとめたタイムラインの基本部品。 |
| Timeline | [Keyframe](components/Keyframe.md) | AnimationCurve 上の 1 キーフレームを表すグリフで、形が補間（`InterpolationMode`）を、塗りの色が選択状態を表す。 |
| Timeline | [CurveEditor](components/CurveEditor.md) | Property の AnimationCurve を時間軸のグラフとして表示・編集するエディタで、値グラフと速度グラフを切り替えられる。 |
| Project と Viewer | [AssetRow](components/AssetRow.md) | Project パネルの素材一覧の 1 行で、素材の種類をアイコンとその色（`kind-*`）で示し、映像と音声を一目で見分けられるようにする。 |
| Project と Viewer | [Viewer](components/Viewer.md) | Sequence / Composition の現在時刻の画を表示し、再生操作とタイムコードでの移動を受け持つプレビュー領域。 |
| オーバーレイ | [Menu](components/Menu.md) | 右クリックやメニューバー、PopupButton から開くコマンドの一覧で、項目ごとにアイコン・ショートカット・チェック・サブメニューを持てる。 |
| オーバーレイ | [Popover](components/Popover.md) | 対象（キーフレーム、クリップ、ボタン）に矢印で結び付けて開く小さな編集面で、その場で数項目を調整する。 |
| オーバーレイ | [Dialog](components/Dialog.md) | 作業を止めて判断を求めるシートで、型付きエラー（`ASSET_MISSING`、`UNSUPPORTED_FEATURE`、`UNDO_CONFLICT` など）の説明と次の操作を示すのに使う。 |
| レイアウトと状態 | [Panel](components/Panel.md) | ワークスペースを分割する入れ物で、見出しの付いた単独パネルと、開いている Sequence / Composition を切り替えるタブ付きパネルの 2 形を持つ。 |
| レイアウトと状態 | [StatusBar](components/StatusBar.md) | ウインドウ下端の 1 行で、保存状態と revision、外部（CLI / MCP）からの変更、未解決の型付きエラー、実行中のジョブを常に見えるようにする。 |
| レイアウトと状態 | [JobRow](components/JobRow.md) | 書き出し・解析などのジョブ 1 件を表す行で、StatusBar から開く一覧に並べ、状態ごとに次の操作を示す。 |
| レイアウトと状態 | [EmptyState](components/EmptyState.md) | 中身のないパネルに置く案内で、何がないかと次にできることを示し、ファイルのドロップ先も兼ねる。 |
| レイアウトと状態 | [Welcome](components/Welcome.md) | 起動時やプロジェクトを閉じたときに出す最初のウインドウで、新規作成・開く・最近のプロジェクトの再開を受け持つ。 |

## 変更の手順

- 値を変えるときは tokens.json と、この文書のトークン表を同じ変更で更新する。コントラストの記述（「4.5:1 以上」など）は両テーマで計算し直す。
- 色の役割（琥珀・青・赤・種類の色の意味）、テーマの既定、書体、アイコンセットを変える場合は ADR を追加する。
- コンポーネントを追加するときは components/ に仕様を 1 ファイル置き、上の表に加える。仕様には「利用側が渡すもの」「見た目」「使い分け」を書き、値はトークン名で参照する。
