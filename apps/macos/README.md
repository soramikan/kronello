# macOS デザインコンポーネント

`KronelloDesign` は [デザインシステム](../../docs/design-system/README.md) の SwiftUI 部品ライブラリ。作品モデル、FFI、Command の発行、UI 状態の保存は利用側の責務とし、このライブラリには含めない。実機での操作・見た目の受け入れは、下記の gallery とテストを使って別途確認する。

ウインドウのルートで `KRFonts.registerBundled()` を呼び、`.krTheme(.dark)` または `.krTheme(.light)` を付ける。指定がなければ Dark。色は環境の `krPalette` から取得する。`Generated/` は生成元スクリプトで更新し、手で編集しない。

gallery と ImageRenderer smoke は `.environment(\.krStaticRendering, true)` を指定する。この公開環境値の既定値は false。true のときだけ入力部品は編集可能な TextField を同じ枠・余白・書式の Text に置き換える。実アプリでは指定せず、通常の編集・IME・確定操作を使う。Menu / PopupButton の表示と hover・アンカー計測は SwiftUI のみで、NSPanel は presenter の表示経路にだけある。

## 公開 API

| 用途 | API と利用側の入力 |
|---|---|
| コマンド | `KRButton` / `KRButtonStyle`。`KRButtonVariant` の 4 種、アイコン、`pressed`、action。無効化は `.disabled(true)` |
| 選択 | `KRSegmentedControl` + `KRSegment`、`KRPopupButton` + `KRPopupOption`。安定した ID と selection の Binding、変更 callback |
| メニュー | `KRMenu` + `KRMenuItem`。見出し、区切り、チェック、無効、破壊的操作、子項目。`KRMenuPresenter.present(_:anchoredTo:theme:current:onDismiss:)` は borderless child `NSPanel` を使う。SwiftUI の global frame（content view の左上原点）を渡す `present(_:anchoredTo:in:theme:current:onDismiss:)` もある。`KRMenu.onSubmenu` は行の global CGRect を通知する |
| 数値 | `KRNumberField`。value の Binding、unit、step、range、precision、`onPreview`、`onCommit(from,to)`。`KRNumberEdit` はスクラブ・入力の独立したトランザクションモデル |
| 文字 | `KRTextFieldStyle`、`KRTextField`、`KRSearchField`。文字入力は IME を扱う標準 TextField を使い、`KRTextField` はローカル draft を Return / フォーカス移動で確定。検索は UI 状態の live Binding |
| チェック | `KRCheckbox` / `KRCheckboxStyle` の off / on / mixed、`KRRadio` / `KRRadioStyle` の排他選択。radio とツールはネイティブ radio の accessibility representation を持つ |
| 範囲 | `KRSlider`。range、step、value、preview / commit。正確な入力用の `KRNumberField` を併置する |
| キーフレーム | `KRKeyframeGlyph` + `KRInterpolation`。linear / cubic / hold、selected / hollow / on、選択 action。size の既定値は keyframeSize、レイヤー集約行は 7px。位置・移動の Binding や gesture は利用側が追加する |
| Property | `KRKeyframeNavigator`、`KRInspectorRow` + `KRPropertySource`。constant / curve / expression、前後・追加削除 callback、値フィールド、`KRDiagnostic`。複数成分は `KRInspectorAxis("X") { ... }` などで caption を前置する |
| パネル | `KRPanel` / `KRPanelTitle`、`KRTabBar` + `KRTab`。見出しまたはタブ、中身、右端 actions、閉じる callback |
| 一覧 | `KRLayerRow` + `KRLayerKind`、`KRAssetRow` + `KRMediaKind`。containment level と transform parent は別入力。選択・開く・トグル callback。資産の drag は利用側が `.draggable` / `.onDrag` を追加 |
| タイムライン | `KRTrackHeader`、`KRTrack`、`KRClip` + `KRClipState`、`KRRuler` + `KRRulerTick`、`KRPlayhead`。レーンの位置・幅は利用側で pixel 座標に解決する |
| 道具 | `KRToolStrip` + `KRTool`。selection と `KRToolStripPlacement` の Binding。placement は viewerLeft / viewerRight / floating(x,y) と collapsed を Codable で保持。取っ手は移動量の preview / commit を通知し、ドッキング判定・配置・保存は利用側が行う |
| 再生 | `KRTransportBar`、`KRTimecodeField`、`KRTimecode`。非負の Int64 frame count と整数の nominal fps。非 drop-frame の `HH:MM:SS:FF` と先頭を省略した入力を扱い、解析失敗・overflow は型付きエラー |
| Viewer | `KRViewerFrame` + `KRViewerSelection`。aspectRatio、描画内容、正規化された選択 bounds と整形済み label。`fittingSize` は幅と高さの両方に収める |
| 状態 | `KRStatusBar` + `KRJobSummary`、`KRProgressBar`、`KRActivityIndicator`、`KRJobRow` + `KRJobState`。状態と文字列、error / job を開く callback、行の actions |
| 判断・編集面 | `KRDialog` + `KRDialogAction`（最大 3 ボタン、primary は最大 1 個・右端）、`KRPopover` / `KRPopoverRow`、`KREmptyState`。シート・popover の presentation と外側クリック / Esc の dismissal、drop の受け付けは利用側が行う |
| 共通 | `KRFocusRing` / `.krFocusRing()`、`KRDiagnostic` / `KRErrorLine`。フォーカスは selection の 2px 実線。タブだけは仕様に従い内側に置く |

`KRControlAppearance` と `KRNumberFieldState` は静止画でも hover / focus / scrubbing などを表示するための指定。実際の hover / focus はコントロール自身も扱う。`EnvironmentValues.krFreezeActivity` は gallery / snapshot の回転停止用で、実アプリでは既定の false を使う。OS の reduced-motion は常に尊重する。

## ビルド・レビュー

リポジトリのルートでフォントを取得する。

```sh
python3 scripts/fetch_ui_fonts.py
```

`apps/macos` で実行する。

```sh
swift build
swift test
swift run KronelloDesignGallery /tmp/kronello-gallery
```

gallery は 2x の PNG とその絶対パスを出力し、描画や保存に失敗したら終了コード 1 で止まる。PNG をコミットしない。出力は以下の各名前に `-dark.png` / `-light.png` を付けたもの（35 部品・基礎シート + 1 画面、合計 72 ファイル）。

```text
Button SegmentedControl PopupButton Menu NumberField TextField SearchField
Checkbox Radio Slider FocusRing KeyframeNavigator InspectorRow Panel TabBar
LayerRow AssetRow KeyframeGlyph Track Clip Ruler Playhead ToolStrip TransportBar
TimecodeField ViewerFrame StatusBar ProgressBar JobRow Dialog Popover EmptyState
Icons Typography Palette Screen-motion
```

テストは NumberField の閾値・倍率・clamp・確定一回・キャンセル、timecode の省略・範囲・overflow・往復、placement の Codable 往復、Viewer の両方向 fit、32 部品の両テーマ ImageRenderer smoke を含む。smoke は画像生成と寸法の検査で、画素の CSS 一致や実操作・VoiceOver・日本語 IME の受け入れを保証するものではない。
