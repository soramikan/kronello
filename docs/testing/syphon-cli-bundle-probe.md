# Syphon検出でCLIのbuild directoryをbundle走査しない

2026-10-13のPERF-013必須workspace検証で、FFI外部出力2件が10秒deadlineを超過した。export/metadataの失敗ではなく、native framework検出に残っていた実装上の遅延である。元の全workspace実行はexit101（`target/perf-013-final-workspace-tests.log`）であり、成功扱いにしない。

## 原因と実測

同じworkspace試験binary `output-3840789999f3eee8` を `KRONELLO_FFI_TIMING=1 --nocapture` で再実行した。要求5/6の処理は約14,522.7/14,519.6msであり、queue待ち約0.007msと区別する。別の開始直後のsampleでは、517samplesすべてで一方のworkerが`kr_output_syphon_detected`→`NSBundle.mainBundle`→`CFBundleGetMainBundle`→`_CFBundleGetBundleVersionForURL`に入り、そのうち497samplesが`_CFIterateDirectory`→`readdir`だった。他方のworkerは同じ検出の`dispatch_once`を待っていた。Metalの初期化も同じmain bundleを参照していた。永続deadlockとは扱わず、大きいbuild/artifact directoryでbundleを推測する走査が遅延の原因と判断する。

- native stack: `target/perf-013-ffi-output-sample-2.txt`、161,574bytes、SHA-256 `85874b6d2509d5352050d92812e27c5b236a61253168e41ce2a0fdf760c2da74`。
- 元timing: `target/perf-013-ffi-output-trace.log`、SHA-256 `4bf527b8bb305537dc37d00da9a576d83ddf3afa102f0ab94c06e25771e30bd2`。

## 修正

Syphonのbundled候補は`_NSGetExecutablePath`とsymlink解決済み実行fileの親から、明示した`.app/Contents/MacOS`または`.app/Contents/Helpers`にある場合だけ`Contents/Frameworks/Syphon.framework/Syphon`を生成する。CLI/test executableではNSBundleを探索しない。already-loaded class、system frameworkと既存relative候補、dispatch_onceによる検出のprocess寿命は維持する。任意のplugin pathや新しい公開requestを追加しない。

既存配布のGUIはMacOS、workerはHelpersに置かれる（`AppController.swift`）。両配置・空白入りapp名・通常CLI・似た不正配置・出力buffer容量不足をnative helperの単体試験で確認する。helperはlayoutを解決するだけでfileのprobe/loadをしない。Metal/CAMetalLayer自身の初回初期化は別の処理である。

最初の修正後FFI試験は3 passed、全体25.88秒だった。問題の要求は約1.02〜4.21msへ減り、以降の要求は0.023〜0.091ms。全体時間には別testのCAMetalLayer初期化が残るため、要求時間と区別する。deadline10秒と試験のtyped outcome条件は変更していない（`target/perf-013-ffi-output-fixed.log`、SHA-256 `5da87224bd43dbc66864a247d92347d34a3fcaa05c202aa71e53bed3d283819e`）。Helpers配置を加えた最終版の単体/FFI再検証とworkspace必須gateは別ログで追跡する。
