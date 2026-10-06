# macOS package

macOS アプリの SwiftPM package。GUI-001 の実アプリ、FFI-001 の native 境界（`CKronelloFFI` / `KronelloCore`、検証用 harness）、デザインシステムの SwiftUI 部品（`KronelloDesign`、後半の節）を含む。
package 名は `Kronello`、Swift tools 5.10、macOS 14 以上。

| target | 内容 |
|---|---|
| Kronello | Welcome / main window、Edit / Motion / Template / Export、明示 Dark / Light theme |
| KronelloAppModel | 共有 Command / Query の view model、session Undo、ユーザー UI state actor |
| KronelloAppModelTests | selection / conflict / Undo / state 分離 / commit-once / GUI-CLI 同等性 |
| CKronelloFFI | C header / module map。Rust の9関数だけを公開 |
| KronelloCore | MainActor の async wrapper と公開 schema 由来の Codable 型 |
| KronelloPreviewHarness | 一つの AppKit window と CAMetalLayer。編集・プレビューの検証専用 |
| KronelloJSONBenchmark | JSON encode / decode の単体計測 |
| KronelloCoreTests | Swift/CLI Event 同等性、生成型・raw transport の検証 |

## build / test

repository root で実行する。Rust 1.95.0、Swift compiler / macOS SDK、既存 native dependencies が必要。
共有環境では設定済みの `CARGO_HOME` / `CARGO_TARGET_DIR` / `TMPDIR` を維持する。

```sh
python3 scripts/generate_swift_api.py --check
python3 scripts/build_ffi.py
swift build --package-path apps/macos -j 3
swift test --package-path apps/macos -j 3
swift run --package-path apps/macos --skip-build KronelloJSONBenchmark examples/ffi-json-benchmark.request.json
```

`build_ffi.py` は `cargo build -p kronello-ffi -p kronello-cli --locked` を jobs=3 で実行し、
Cargo の artifact JSON から cdylib / CLI の実際の出力先を取得する。
`Libraries/libkronello_ffi.dylib` と `Libraries/kronello` はローカル生成物で Git 管理しない。
C header は手書きで Rust の全署名との一致を `header_matches_every_exported_function_signature` が検査する。
Swift ファイルを更新する場合は `python3 scripts/generate_swift_api.py` を使う。

Rust library の install name は `@rpath/libkronello_ffi.dylib`。
SwiftPM が絶対パスのローカル `Libraries` を link / rpath に加える。
開発 app bundle と ad-hoc signing は下記スクリプトで行う。配布 signing / notarization と LGPL FFmpeg runtime の組み立ては後続範囲。
FFI build は FFmpeg executable や GPL binary を同梱しない。

## M3開発アプリ

```sh
python3 scripts/build_macos_app.py --release
target/macos/Kronello.app/Contents/MacOS/Kronello
```

スクリプトは FFI / CLI、UI fonts、`swift build -j 3` を実行する。`--release`はRust FFI / CLIを最適化し、Swiftは開発buildを使う。既存buildのみ使う場合は `--skip-build`。
M3の最終受け入れ・実機証拠と保証範囲は [統合検証記録](../../docs/testing/m3-acceptance.md)。
Xcodeのtoolchainを使用する環境では `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` を設定する。
LGPL FFmpegを別途用意し、必要に応じて `PKG_CONFIG_PATH` と `KRONELLO_FFMPEG_LIB_DIR` をその開発 / runtimeディレクトリに設定する。
`target/macos/Kronello.app`（bundle ID `dev.kronello.Kronello`）の MacOS executable、Frameworks の FFI dylib、
Helpers の `kronello` worker、Resources の `Kronello_KronelloDesign.bundle` と font licenses を配置する。
executable の rpath は `@executable_path/../Frameworks`。library / helper / app を ad-hoc sign し verify する。
FFmpeg executable / runtime、GPL binary はコピーしない。開発 bundle の組立・起動は host run を要する。

起動時に `KRFonts.registerBundled()` を呼び、theme は Dark 既定。表示メニュー / Settings の明示設定だけで
Light へ切り替える。`NSApp.appearance` と各 window root の `krTheme` を一致させる。
新規 `.kronello` は共有 `project.create`、open は NSOpenPanel、recent はユーザー状態領域へ保存する。
Motion の操作と既知の制限、host screenshot procedure は [GUI-001 の検証](../../docs/testing/gui-001.md)。

UI state は `KRONELLO_STATE_ROOT` または `~/Library/Application Support/Kronello/` の
`ui-state/<project-id>.json`、theme / recent は `preferences.json`。`.kronello` に UI state を追加しない。
Text render / creation 用の font は UI font と別契約。`KRONELLO_FONT_INPUTS` に JSON manifest の絶対 path を
指定する（配列要素は共有 API の `{identity, path}`）。既存 document font lock と一致しない限り Text tool は無効。
未指定・欠落時は型付き `FONT_MISSING`。暗黙の代替フォントを使わない。

共有契約の追加は `SceneNode.name` / `enabled`、`node_rename` / `node_enabled_set` /
`node_property_insert`、`project.info.open_mode`。lock は GUI 個人の UI state。
safe-mode store は現行 FFI で要求ごとに開閉され、ウインドウ全体の排他は保証しない（[ADR-0061](../../docs/adr/0061-macos-editor-session-and-ui-state.md)）。

## API / ownership

`ProjectSession(path:workerExecutable:)` はすぐに handle を返す。ファイルの検査結果は
`await ready()` で共有 `project.info` Response として取得する。
`call(API.Request)` は共有 Request / Response を使い、毎回明示的な project path を持つ。
`rawCall(Data)` は共有 strict decoder に生 JSON を渡す。重複 key の拒否も CLI と同じ。
`render.submit` には同じ revision の CLI worker executable を open 時に指定する。
指定がなければ job を作る前に `WORKER_EXECUTABLE_REQUIRED` を返す。

C 側の入力は関数内でコピーし、Rust worker に借用を残さない。入力上限16 MiB。
poll は `response_json` に共有 Response の JSON テキストを入れる。
Swift はこの文字列を Data として保持し、raw 応答中の未知の巨大整数を丸めない。
生成型の `JSONValue.number` は Foundation Decimal を使うため、
Decimal の精度を超える未知 JSON 数値を保持・再送する操作には rawCall を使う。
通常の Event UInt64 revision と Rational の10進文字列は生成型でも正確に保持する。
Project の未知 field と decode 時の明示的 null は生成型の再 encode で保持する。
schema の数値範囲・文字列 pattern・配列長・RenderInput の排他的 target 等の検証は共有 Rust API が正本。

要求は一つの専用 Rust thread で FIFO 実行する。最大64件の未回収 completion。
キューが満杯なら enqueue は0を返し、Swift は `NativeError.rejected` とする。
revision / job notification は250 msの idle interval と要求完了後に検査し、最新 snapshot に集約する。
`subscribe()` と `onNotification` を使い、idle 時は AppKit timer 等から `poll()` を呼ぶ。
Swift wrapper は待機中にも poll し、30秒で timeout する。
Task の cancel / timeout は待機を終了するが、既に受け付けた編集は取り消さない。
`close()` は新規要求を止める。受け付け済み work は worker で終了して資源を解放する。

## Metal preview の実機手順

この手順は Metal を使える Apple Silicon host で行う。
新しい project path を選ぶ。harness は fixture を create し、Shape size を `24 x 16` に edit.plan / edit.apply して Event を stdout に出す。

```sh
mkdir -p target/ffi-host
swift run --package-path apps/macos --skip-build KronelloPreviewHarness \
  target/ffi-host/preview.kronello examples/ffi-preview.project.json apps/macos/Libraries/kronello
```

一つの window に黒背景と赤い Shape が見えること、window resize 後も表示が更新されることを確認する。
stdout の `Presented` に `backend: metal`、編集後 revision、`image_readbacks: 0` が出る。
CAMetalLayer は attach 時に Rust が retain し、surface より後に release する。
NSView への取り付けと drawableSize 更新は AppKit main thread、
adapter / DAG / GPU / surface 操作は Rust worker に分離する。
画素は CPU / JSON を通らず、GPU RGBA16F texture から surface に描く。
4-byte shader validation status の readback は既存 GPU pipeline と同じ。
表示は linear premultiplied Rec.709 を黒に合成して sRGB SDR encode / clip する。
HDR tone mapping は実装しない。最終書き出しの色契約は変えない。

記録する証跡は window の screenshot、resize 前後の Presented log、
`project.info` / `history.list` の revision / Event と host の OS / GPU。
現時点の実行結果と未検証範囲は [FFI-001 検証](../../docs/testing/ffi-001.md) を参照。

## restricted worker の補助確認

SwiftPM の build service が sandbox 外への書き込みを要求する環境向けに、
`scripts/check_ffi_swift.py` は compiler を直接呼び、同じ core / harness / benchmark を compile する。
XCTest target が包む共通の throwing checks を実行するが、`swift build` / `swift test` の成功判定には使わない。

```sh
python3 scripts/check_ffi_swift.py --swiftc /path/to/toolchain/usr/bin/swiftc --sdk /path/to/MacOSX.sdk
```

## macOS デザインコンポーネント

`KronelloDesign` は [デザインシステム](../../docs/design-system/README.md) の SwiftUI 部品ライブラリ。作品モデル、FFI、Command の発行、UI 状態の保存は利用側の責務とし、このライブラリには含めない。実機での操作・見た目の受け入れは、下記の gallery とテストを使って別途確認する。

ウインドウのルートで `KRFonts.registerBundled()` を呼び、`.krTheme(.dark)` または `.krTheme(.light)` を付ける。指定がなければ Dark。色は環境の `krPalette` から取得する。`Generated/` は生成元スクリプトで更新し、手で編集しない。

gallery と ImageRenderer smoke は `.environment(\.krStaticRendering, true)` を指定する。この公開環境値の既定値は false。true のときだけ入力部品は編集可能な TextField を同じ枠・余白・書式の Text に置き換える。実アプリでは指定せず、通常の編集・IME・確定操作を使う。Menu / PopupButton の表示と hover・アンカー計測は SwiftUI のみで、NSPanel は presenter の表示経路にだけある。

### 公開 API

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
| GUI 状態面 | `KRWelcome` / `KRRecentProject`、32px `KRStateBand`、`KRConflictBanner`、`KRViewerError`、7px handle の `KRManipulationOverlay`。`KRWindowMetrics` は画面仕様寸法 |

`KRSegment.unavailableReason` / `KRTool.unavailableReason` は選択肢を個別無効化し tooltip を出す。
`KRInspectorRow.keyframeEditingEnabled` は glyph の編集だけを無効化し、Curve の前後 navigation を残す。
`KRLayerRow.diagnostic` は名前の後ろに danger icon と code / message tooltip を出す。
この追加は gallery の `UnavailableControls` と GUI 状態面の5 sheets で両 theme をレビューする。

`KRControlAppearance` と `KRNumberFieldState` は静止画でも hover / focus / scrubbing などを表示するための指定。実際の hover / focus はコントロール自身も扱う。`EnvironmentValues.krFreezeActivity` は gallery / snapshot の回転停止用で、実アプリでは既定の false を使う。OS の reduced-motion は常に尊重する。

### ビルド・レビュー

リポジトリのルートでフォントを取得する。

```sh
python3 scripts/fetch_ui_fonts.py
```

`apps/macos` で実行する。

```sh
swift build -j 3
swift test -j 3
swift run -j 3 KronelloDesignGallery /tmp/kronello-gallery
```

gallery は 2x の PNG とその絶対パスを出力し、描画や保存に失敗したら終了コード 1 で止まる。PNG をコミットしない。出力は以下の各名前に `-dark.png` / `-light.png` を付けたもの（35 部品・基礎シート + 1 画面 + GUI 状態6 sheets、合計 84 ファイルを予定。追加 sheets の host 描画は未検証）。

```text
Button SegmentedControl PopupButton Menu NumberField TextField SearchField
Checkbox Radio Slider FocusRing KeyframeNavigator InspectorRow Panel TabBar
LayerRow AssetRow KeyframeGlyph Track Clip Ruler Playhead ToolStrip TransportBar
TimecodeField ViewerFrame StatusBar ProgressBar JobRow Dialog Popover EmptyState
Icons Typography Palette Screen-motion
Welcome StateBand ConflictBanner ViewerError ManipulationOverlay UnavailableControls
```

テストは NumberField の閾値・倍率・clamp・確定一回・キャンセル、timecode の省略・範囲・overflow・往復、placement の Codable 往復、Viewer の両方向 fit、32 部品の両テーマ ImageRenderer smoke を含む。smoke は画像生成と寸法の検査で、画素の CSS 一致や実操作・VoiceOver・日本語 IME の受け入れを保証するものではない。
