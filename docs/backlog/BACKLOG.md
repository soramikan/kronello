# バックログ一覧

<!-- このファイルは scripts/backlog.py render が生成する。直接編集しない。正本は backlog.json。 -->

- schema_version: 0.3
- 更新日: 2026-10-02
- タスク数: 50

## 集計

| マイルストーン | planned | in_progress | done | dropped | 計 |
|---|---:|---:|---:|---:|---:|
| M0 | 5 | 0 | 0 | 0 | 5 |
| M1 | 10 | 0 | 0 | 0 | 10 |
| M2 | 10 | 0 | 0 | 0 | 10 |
| M3 | 10 | 0 | 0 | 0 | 10 |
| M4 | 6 | 0 | 0 | 0 | 6 |
| M5 | 5 | 0 | 0 | 0 | 5 |
| M6 | 4 | 0 | 0 | 0 | 4 |

## M0

### ARC-001 意味規約と互換性境界をADRに固定

- 優先度: P0 / 領域: architecture / 状態: planned
- 依存: なし
- 受け入れ条件:
  - Timeline/Composition/Property/Renderの依存方向を文書化する
  - 単位・alpha・未知機能・スナップショット版の規約をレビューする

### TIME-001 有理数時間・半開区間・時間マッピング

- 優先度: P0 / 領域: time / 状態: planned
- 依存: ARC-001
- 受け入れ条件:
  - 整数overflowを検出しJSONの整数精度を失わない
  - 30000/1001fpsと48kHz音声の境界をproperty-based testで検証する

### PROP-001 型付きPropertyとschema registry

- 優先度: P0 / 領域: model / 状態: planned
- 依存: ARC-001
- 受け入れ条件:
  - 型・単位・補間可否・範囲・安定IDを共通schemaで表す
  - Constant/Curve/Expressionの主値源を排他的に保持する

### GPU-001 最短GPU描画とFrameBridge技術スパイク

- 優先度: P0 / 領域: gpu / 状態: planned
- 依存: ARC-001
- 受け入れ条件:
  - 2D素材からRGBA16F出力までの色とalphaを検証する
  - CPU往復とGPU内コピーを区別して各候補経路の可否を記録する
  - macOS (Metal / VideoToolbox) を最初の検証対象とする

### QA-001 参照素材とgolden sceneを整備

- 優先度: P0 / 領域: test / 状態: planned
- 依存: ARC-001
- 受け入れ条件:
  - 日本語・alpha・異なるfps・VFR・HDRの権利確認済みfixtureを用意する
  - 固定環境の画像比較と意味的比較を別に定義する


## M1

### STORE-001 文書・イベント・スナップショット保存

- 優先度: P0 / 領域: storage / 状態: planned
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - 文書とイベントとrevisionを同一transactionで更新する
  - 完全snapshotからの復元と失敗migrationの非破壊性を確認する
  - 単一SQLiteファイル(.koma)を正本とし、レンダーキャッシュをプロジェクト外へ分離する
  - 別プロセスからの同時書き込みをrevision照合で直列化し、古いbase_revisionを拒否する

### COMP-001 Composition/Instance/Group/Nullモデル

- 優先度: P0 / 領域: scene / 状態: planned
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - InstancePathで共有定義の複数配置を区別する
  - 所有・変換・参照の循環を個別に診断する

### ANIM-001 基本キーフレームと補間

- 優先度: P0 / 領域: animation / 状態: planned
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - Hold/Linear/Cubicと連続角の複数回転を実装する
  - 同時刻key upsertと任意時刻評価の結果を固定する

### EVAL-001 Property依存グラフと任意時刻評価

- 優先度: P0 / 領域: evaluation / 状態: planned
- 依存: COMP-001, ANIM-001
- 受け入れ条件:
  - 順序・逆順・ランダム順の評価で同じ意味的結果を返す
  - 依存循環の経路と原因Propertyを報告する

### VEC-001 Shape/Path/Fill/Stroke IR

- 優先度: P0 / 領域: vector / 状態: planned
- 依存: COMP-001
- 受け入れ条件:
  - 角丸矩形・楕円・ベジェ・基本fill/strokeを保持する
  - 拡大と解像度変更で文書をビットマップ化しない

### TEXT-001 日本語組版とクラスタ/グリフ対応

- 優先度: P0 / 領域: text / 状態: planned
- 依存: COMP-001, QA-001
- 受け入れ条件:
  - 横書き日本語・基本禁則・フォント固定・欠落検査を実装する
  - 書記素とグリフが一対一でないfixtureで対応を検証する

### GPU-002 線形色合成・mask・isolated group

- 優先度: P0 / 領域: gpu / 状態: planned
- 依存: GPU-001, VEC-001, TEXT-001
- 受け入れ条件:
  - Group opacityを子ごとに適用した誤結果をgolden testで区別する
  - mask/alpha edge/色変換を固定環境で検証する

### RENDER-001 Scene IRからRender DAGと画像連番

- 優先度: P0 / 領域: render / 状態: planned
- 依存: EVAL-001, GPU-002
- 受け入れ条件:
  - 指定有理数時刻と領域を入力に一枚の画像を描画する
  - 出力の色/alpha規約をmetadataへ記録する

### CACHE-001 値・layout・geometry・rasterの分離cache

- 優先度: P0 / 領域: cache / 状態: planned
- 依存: RENDER-001
- 受け入れ条件:
  - 位置変更で組版cacheを再利用する
  - 色・本文・フォント変更が必要な範囲だけを無効化する

### CLI-001 ヘッドレス生成と機械向けCLI

- 優先度: P0 / 領域: cli / 状態: planned
- 依存: STORE-001, RENDER-001
- 受け入れ条件:
  - stdin JSON/stdout JSONとstderr logを分離する
  - GUIなしでShapeと日本語Textのアニメーション連番を生成する


## M2

### MEDIA-001 FFmpeg素材I/Oと時刻精度

- 優先度: P0 / 領域: media / 状態: planned
- 依存: TIME-001, QA-001, GPU-001
- 受け入れ条件:
  - VFR/B-frame/seek後の対象PTSを確認する
  - 使用中のdecode/encode/transfer経路を報告する
  - LGPL構成のFFmpegを動的リンクし、検出したcodec/hwaccelをcapabilitiesへ報告する

### AUDIO-000 基本音声: デコード・ミックス・音声付き書き出し

- 優先度: P0 / 領域: audio / 状態: planned
- 依存: TIME-001, MEDIA-001
- 受け入れ条件:
  - 素材音声を48kHzへ変換しクリップ音量を適用してBusへミックスする
  - 音声サンプル位置を絶対時刻から計算し、映像との同期をサンプル精度で検証する
  - 書き出しで映像と音声を同じ固定snapshotからmuxする

### NLE-001 CompositionClipとマルチトラック統合

- 優先度: P0 / 領域: timeline / 状態: planned
- 依存: COMP-001, MEDIA-001, RENDER-001
- 受け入れ条件:
  - 同じCompositionを異なる長さ/時刻で複数配置する
  - trim/stretch/instance retimeを別操作としてテストする

### FX-001 基本エフェクト: drop shadow / gaussian blur

- 優先度: P0 / 領域: effects / 状態: planned
- 依存: GPU-002, RENDER-001
- 受け入れ条件:
  - エフェクトが必要な入力領域(ROI halo)を宣言しvisual_boundsへ反映する
  - premultiplied alphaと線形作業空間でのshadow/blurをgolden testで検証する

### SERVICE-001 計画・適用・競合・冪等性

- 優先度: P0 / 領域: service / 状態: planned
- 依存: STORE-001, CLI-001
- 受け入れ条件:
  - base_revision/plan_hash/idempotency_keyを検証する
  - 同じキー同じpayloadは重複適用せず異なるpayloadは拒否する
  - idempotency_keyと適用結果をプロジェクト内に保存し、別プロセスからの再送にも同一結果を返す

### TEMPLATE-001 公開入力と保護時間区間の最小template

- 優先度: P0 / 領域: template / 状態: planned
- 依存: COMP-001, SERVICE-001
- 受け入れ条件:
  - 定義とinstance入力を分離し版を固定する
  - 5秒から8秒にしてintro/outroの長さが変わらない
  - テキストlayout_boundsへの単方向参照で背景帯が追従し、max_lines超過をoverflowとして検出する

### API-001 共通schemaとquery/command公開

- 優先度: P0 / 領域: api / 状態: planned
- 依存: SERVICE-001, PROP-001, EVAL-001
- 受け入れ条件:
  - scene query/property sample/capabilitiesを構造化結果で返す
  - APIの任意シェル/外部URL実行を禁止する

### MCP-001 MCPアダプター

- 優先度: P0 / 領域: mcp / 状態: planned
- 依存: API-001
- 受け入れ条件:
  - 交渉したprotocol versionでschemaとstructuredContentを返す
  - クライアント接続状態に暗黙の対象Projectを保持しない

### JOB-001 固定snapshot書き出しjob

- 優先度: P0 / 領域: jobs / 状態: planned
- 依存: SERVICE-001, MEDIA-001, RENDER-001
- 受け入れ条件:
  - 切断後も保存されたjob IDで状態取得できる
  - 一時出力を検証してから確定名へ切り替える

### INTEGRATION-001 縦断デモ第1段階: 日本語lower-third (CLI/MCP)

- 優先度: P0 / 領域: integration / 状態: planned
- 依存: NLE-001, TEMPLATE-001, MCP-001, JOB-001, AUDIO-000, FX-001
- 受け入れ条件:
  - CLIで作成しMCPで確認して固定snapshotから4K出力する
  - 別instanceの文字/色/長さが干渉しない
  - 5秒から8秒への尺変更と別テキストで、背景帯追従・overflow検出・基本shadowを検証する


## M3

### EXPR-001 型付きASTと有界式評価

- 優先度: P1 / 領域: expression / 状態: planned
- 依存: EVAL-001, API-001
- 受け入れ条件:
  - 静的依存列挙と命令/メモリ/サンプル予算を実装する
  - 固定seedのnoiseと禁止機能の拒否をテストする

### LAYOUT-001 responsive layoutとbounds段階

- 優先度: P1 / 領域: layout / 状態: planned
- 依存: TEXT-001, VEC-001, EVAL-001
- 受け入れ条件:
  - layout/ink/visual boundsを区別する
  - 文字幅と背景幅の循環およびoverflowを診断する

### TEMPLATE-002 長さ・縦横比variant・data入力・版移行

- 優先度: P1 / 領域: template / 状態: planned
- 依存: TEMPLATE-001, LAYOUT-001
- 受け入れ条件:
  - 短尺拒否/hold/loop/stretchを明示する
  - 版更新の差分計画とプレビューを生成し勝手に既存作品を更新しない

### GUI-001 macOSネイティブGUI: Canvas・階層・変換操作

- 優先度: P1 / 領域: gui / 状態: planned
- 依存: FFI-001, RENDER-001
- 受け入れ条件:
  - GUI操作が共通command/eventを使う
  - 選択・pan/zoomなどUI状態を作品から分離する
  - CLI/MCPなど外部プロセスによるrevision変化を検知して再読込する

### GUI-002 Dope sheetとCurve editor

- 優先度: P1 / 領域: gui / 状態: planned
- 依存: GUI-001, ANIM-001
- 受け入れ条件:
  - キー移動/接線編集/UndoがCLIで読める同じモデルを変更する
  - 空間パスと時間イージングを区別して表示する

### AUDIO-002 リアルタイム音声再生とA/V同期

- 優先度: P1 / 領域: audio / 状態: planned
- 依存: AUDIO-000, GUI-001
- 受け入れ条件:
  - 音声コールバックをプロジェクト更新・ディスク読み出し・式評価と別の実行系にする
  - プレビュー再生で音声クロックを基準に映像フレームを提示する

### INSPECT-001 非表示原因・依存・レンダー経路のexplain

- 優先度: P1 / 領域: inspection / 状態: planned
- 依存: API-001, LAYOUT-001, CACHE-001
- 受け入れ条件:
  - opacity/active range/parent/mask/asset不足を要因別に返す
  - 過大処理と転送/メモリ/キャッシュを構造化して表示する

### FFI-001 koma-ffi: ネイティブGUI向けCommand/Query境界

- 優先度: P1 / 領域: ffi / 状態: planned
- 依存: API-001
- 受け入れ条件:
  - SwiftからCommand/Query APIを呼び、CLIと同じrevision/eventへ到達する
  - ネイティブ側のCAMetalLayerをwgpu surfaceとして受け取りプレビューを表示する
  - FFI境界にwgpu/SQLite/Tokioの型を露出しない

### QA-002 GUI/CLI/MCP同等性と日本語IME

- 優先度: P1 / 領域: test / 状態: planned
- 依存: GUI-002, MCP-001, INTEGRATION-001
- 受け入れ条件:
  - 同じ編集操作の結果snapshotが一致する
  - 未確定IME文字列を作品履歴へ大量commitしない

### INTEGRATION-002 縦断デモ第2段階: GUI閲覧と縦型variant

- 優先度: P1 / 領域: integration / 状態: planned
- 依存: INTEGRATION-001, TEMPLATE-002, GUI-001
- 受け入れ条件:
  - 第1段階と同じプロジェクトをmacOS GUIで開き、CLI/MCPと同じ値・layout boundsを表示する
  - 同じtemplate定義を縦型variantで再利用し、再レイアウト結果を検証する


## M4

### RENDER-002 時間サンプルと高品質モーションブラー

- 優先度: P1 / 領域: render / 状態: planned
- 依存: RENDER-001, CACHE-001, ANIM-001
- 受け入れ条件:
  - サブ時刻ごとの全体合成を基準に比較する
  - カット境界/ネスト/露光位相/重複サンプルを扱う

### GPU-003 各OSのFrameBridge保証経路

- 優先度: P1 / 領域: gpu / 状態: planned
- 依存: GPU-001, GPU-002, MEDIA-001
- 受け入れ条件:
  - macOS (Metal / VideoToolbox) を最初の保証経路とし、Windows/Linuxは順次昇格する
  - 対応形式ごとにデバイス/所有権/同期/寿命を検証する
  - 非対応経路は明示fallbackまたはrequire_gpu_residentエラー

### COLOR-001 HDR/alpha/高解像度品質

- 優先度: P1 / 領域: color / 状態: planned
- 依存: GPU-002, RENDER-002, QA-001
- 受け入れ条件:
  - HDRの表示変換を最終出力へ勝手に焼き込まない
  - 文字・mask・glowを含む8K offline出力を検証する

### CACHE-002 temporal/region cacheと無効化

- 優先度: P1 / 領域: cache / 状態: planned
- 依存: CACHE-001, RENDER-002
- 受け入れ条件:
  - ROI haloと複数時刻依存をkeyに含める
  - ネストやTimeMap変更が古いフレームを再利用しない

### PERF-001 参照シーンbenchmarkと資源予算

- 優先度: P1 / 領域: performance / 状態: planned
- 依存: GPU-003, COLOR-001, CACHE-002
- 受け入れ条件:
  - warm/cold・proxy/full・preview/finalを分けてp50/p95を出す
  - decode面/atlas/accumulation/encoderを含めたpeak memoryを記録する

### RECOVERY-001 GPU lost/容量不足/worker停止の復旧

- 優先度: P1 / 領域: reliability / 状態: planned
- 依存: JOB-001, GPU-003, STORE-001
- 受け入れ条件:
  - 失敗でProjectや確定済み成果物が壊れない
  - 出力fileへの無条件appendを再開方法に使わない


## M5

### VEC-002 Trim path・morph・SVG対応表

- 優先度: P1 / 領域: vector / 状態: planned
- 依存: VEC-001, ANIM-001, QA-001
- 受け入れ条件:
  - morphは点/segment対応を検証し不整合を拒否する
  - SVGのunsupported機能と外部参照を報告する

### REPEAT-001 Repeaterとinstanceごとの制御

- 優先度: P1 / 領域: motion / 状態: planned
- 依存: COMP-001, EXPR-001, VEC-002
- 受け入れ条件:
  - Source共有とinstance ID/seedを保持する
  - 個別編集のexpandを明示し元templateを変更しない

### TEXT-002 文字単位selector・ルビ・縦書き

- 優先度: P1 / 領域: text / 状態: planned
- 依存: TEXT-001, ANIM-001, LAYOUT-001
- 受け入れ条件:
  - クラスタを壊さない文字演出と親文字/ルビの結合を実装する
  - 再組版時に古いglyph indexへ誤適用しない

### AUDIO-001 音声特徴量DataAssetと連動

- 優先度: P1 / 領域: audio / 状態: planned
- 依存: AUDIO-000, MEDIA-001, EXPR-001, TIME-001
- 受け入れ条件:
  - 分析版/窓/ホップ/時間写像と入力hashを固定する
  - 同じ出力フレームごとに音声全体を再分析しない

### SIM-001 固定刻み・checkpoint・particle基盤

- 優先度: P2 / 領域: simulation / 状態: planned
- 依存: EVAL-001, CACHE-002, REPEAT-001
- 受け入れ条件:
  - 順次再生とcheckpointからのseekを固定環境で比較する
  - 入力変更で影響するcheckpointを無効化する


## M6

### DIM-001 2.5D Scene3D境界とカメラ

- 優先度: P2 / 領域: 3d / 状態: planned
- 依存: COLOR-001, COMP-001, RENDER-002
- 受け入れ条件:
  - 2D描画順と3D depthを別仕様として持つ
  - Color/Maskと必要な補助ポートを型検証する

### INTEROP-001 OTIO/SVG/外部レンダーの交換アダプター

- 優先度: P2 / 領域: interop / 状態: planned
- 依存: NLE-001, VEC-002, JOB-001
- 受け入れ条件:
  - 保持/変換/欠落した機能をレポートする
  - 完全互換でない表現のbakeを明示承認にする

### PLUGIN-001 有界WASM拡張と信頼区分

- 優先度: P2 / 領域: plugins / 状態: planned
- 依存: EXPR-001, RENDER-001, API-001
- 受け入れ条件:
  - CPU fuel/host call/メモリ/外部権限を別々に制限する
  - WASM制限がGPU処理時間を保証しないことを診断設計へ反映する

### DIST-001 レンダーファームとsimulation bake境界

- 優先度: P2 / 領域: distributed / 状態: planned
- 依存: SIM-001, JOB-001, RECOVERY-001
- 受け入れ条件:
  - 素材/フォント/engine versionsをworker間で照合する
  - 未ベイクstateful区間を任意frameへ自由分割しない
