# MATTE-001 検証記録

状態: `done`（2026-10-06）。固有条件、直接GUI、[統合checkpoint](m5-acceptance.md)を確認した。M5全体の完了ではない。

## 契約と実装

[ADR-0099](../adr/0099-authored-matte-relations.md) に文書と transient 入力、ID・版・missing/cycle・alpha/luminance/invert の意味を固定した。`Project.mattes` → shared EditCommand → SQLite revision / Undo → instance-aware SceneIr / Render DAG → CPU/GPU の経路と macOS Inspector の編集を追加した。

## 確認済み

- `cargo test -p kronello-model --test matte -p kronello-service --test matte --locked`: 各1 test 成功。unknown field の opaque lossless 再読込・未知版編集拒否・legacy省略、shared plan/apply と idempotent retry、保存再読込、revision conflict、Undo、alpha/luminance/invert の実 CPU 画素、逆向き関係の cycle・Node欠落・Node削除・未知版の typed rejection、active matte 無効化時の `MATTE_MISSING`、snapshot serde/hash と pin欠落拒否を確認。
- `cargo clippy -p kronello-service -p kronello-gpu --all-targets --locked -- -D warnings`: 成功（上記の追加テストを含む全 workspace の最終 clippy は統合担当が実施）。
- `cargo check -p kronello-service --locked`: 成功。

## 確認済みの native / GPU と残件

- `cargo test -p kronello-gpu --test scene gpu_inverted_alpha --locked -- --nocapture`: sandbox 外で成功、Apple M4 / Metal。alpha/luminance inverse の GPU / CPU 比較を Rec.709 / Rec.2020 の両方で確認。sandbox 内の初回は adapter unavailable で失敗し、fallback / skip はしていない。
- macOS build: Command Line Tools toolchain は SwiftUIMacros plugin 欠落で失敗。`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` に切り替えて `swift build` は成功（36.65秒）。cache は `/private/tmp`、`--disable-sandbox` を使う。
- rootによるGUI直接操作・外部編集取り込み・CLI/MCP実transport・同梱FFIの比較は下記の記録で確認済み。
- 全 workspace fmt / clippy / test と公開 schema / Swift GeneratedAPI の再生成・比較。

## GUI 確認対象

Inspector → Matte: `Matte layer` popup（「なし」または同じ Composition の Node）、`Mask` popup（Alpha / Luminance）、`反転` checkbox、`Matte layer を表示` checkbox。解除は `matte_remove`、その他は同じ UUID を保持する `matte_set`。busy / locked 時は無効。

GUI で関係を作成したあと `project.export` の `mattes`、`scene.query` / render の revision と output を確認する。外部 `MatteSet` のあとの GUI 再読込で同じ関係が表示されることを確認し、古い revision の変更計画は競合として扱う。

## 主担当の直接GUI・実入口検証（2026-10-06）

Apple M4 / macOS 27.0.1 / Metal、debug開発bundleを使用した。共通fixtureは [m5-text-matte.project.json](../../examples/m5-text-matte.project.json)。固定Noto Sans CJK JP Regular（hash `68a3fc98800b2a27b371f2fb79991daf3633bd89309d4ffaa6946fd587f375b5`）を [fixture手順](fixtures.md) で取得してfont入力に指定する。

再現手順: fixtureを`project.create`で新しい`.kronello`へ保存し、GUI Motionで横書きレイヤーのInspector → Matte layerをMatte rectangleへ変更する。Alpha → 反転 → Luminance → 反転解除 → Undoの順に操作する。この記録ではrev 2〜6に対応し、matteの範囲内だけの表示、範囲外への反転、線形luminanceによる減衰、Undoによる反転復元を直接確認した。

- rev 6のGUI保存文書を実CLIとMCP stdioでexportし完全一致。320×240・時刻0のCPU referenceは全76,800画素のlinear / displayとsnapshot hashが一致。hashは`0f255da877e18620f3915b1f2c51170c656ee13568ae2256fd6131c3bd429138`。
- MCP `edit.plan/apply`で同じ関係のinvert=false / visible=trueへ変更しrev 7へ進めた。GUIの「別のセッションの変更を読み込みました（rev 6 → 7）」、チェック状態、可視化したmatte矩形を直接確認した。
- 後述TEXT-002の折り返し変更を含むrev 8・時刻1で、GUI同梱FFIの`render.frame`と実CLIの明示GPU backendを比較。160×120の全19,200画素のlinear / displayが完全一致し、backend=`wgpu_rgba16f`、snapshot hash=`2fde9357c162cc1653a18cb8ba886fcc59e316ceee946ce5a712ef4366a122f1`。同梱FFI SHA-256は`f41a629f66ba43a1949417240a074f027886a613dc30da730b02ab8a211cb801`。これは共有FFIの描画比較であり、OS compositor後の画面画素のbit一致を主張しない。

ローカル証拠は`target/m5-acceptance/gui/`の`matte-{alpha,luminance,external-revision7}.png`、`matte-transports-report.json`、`matte-native-frame-report.json`と実行script / log。最初のGUI確認では旧ruby配置不具合を含むbundleを使用し、FFI比較時には修正済みbundleを再構築した。最終M5変更全体のworkspace・schema・CI検証は別途必要。

## 統合受け入れ

2026-10-06にタスク固有条件と [M5統合checkpoint](m5-acceptance.md) を確認し、`done` とした。M5最終変更の各OS CIはマイルストーン全体で別途確認する。
