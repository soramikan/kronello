# 00 概要

状態: 採用済み設計と将来計画を含む。2026-10-06時点でM0・M1・M2のP0・M3・M4を受け入れ済み。実装・検証の範囲は [現在の実装範囲と残件](../roadmap/implementation-status.md) と各タスクの検証記録を参照し、設計例だけで対応済みと判断しない。
対象: Rust / FFmpeg / wgpu を基盤とする、GUI・CLI・MCP 共通の動画編集ソフトウェア Kronello。
元仕様: [v0.2](../archive/motion_editor_architecture_v0_2.md)（2026-10-01）。本書群は採用ADRと実装の進展に合わせて更新する。

## 結論

NLE（カット編集）の Timeline と、モーショングラフィックスの Composition を別の編集モデルとし、同じ時間・プロパティ・組版・合成・レンダー基盤へコンパイルする。
Composition は SourceRef として Timeline へ配置できる。Composition の中から別の Composition も参照できるが、参照循環は禁止する。

配置から Composition、Property、Render へ至る意味の追跡順と、crate のコード依存は区別する。依存境界と単位は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md)、色・alpha は [ADR-0044](../adr/0044-color-and-alpha-contracts.md)、保存と実行の互換性は [ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md) を契約とする。これらは採用した設計規約であり、個々の受け入れ証拠は [バックログ](../backlog/BACKLOG.md) と [M4記録](../testing/m4-acceptance.md) に対応付ける。

GUI・CLI・MCP は同格の入口であり、同じ Command / Query API を通る（[ADR-0001](../adr/0001-shared-command-query-api.md)）。

## スコープ

### 初期リリースから実装する基盤

- 有理数時間、サブフレーム評価、InstancePath、ローカル時間マッピング。
- 型付き Property、基本キーフレーム、依存関係検証。
- 2D の Composition、Shape、Text、Group、Null、Media、CompositionInstance。
- 幾何形状と組版結果を保持する中間表現。
- 空間領域・時間サンプルを要求できるレンダー API。
- 公開入力を持つテンプレート、変更計画・トランザクション・プレビュー検査 API。
- 基本音声（デコード、ミックス、音声付き書き出し）と基本エフェクト（drop shadow、gaussian blur）。

### 後段の実装と進捗

- M5作業ブランチではPath morph・Trim path・限定SVG、文字selector・ルビ・縦書き、事前解析した音声特徴量の式参照を受け入れ済み（[M5進捗](../testing/m5-acceptance.md)）。mainへの統合とGUI全体の完成とは区別する。
- Repeaterと追加の式評価（固定DataAsset・過去sample・連続noise）はM5作業ブランチで受け入れ済み（[M5進捗](../testing/m5-acceptance.md)）。人間向け式構文とGUI式入力（EXPR-002・ADR-0105）、macOSの追加編集操作GUI-007、AAC-LC/Opusの圧縮配信音声（AUDIO-005・ADR-0106）も共有API・worker・直接GUI確認を経て作業ブランチで受け入れ済み。
- チェックポイント付きシミュレーションSIM-001は、固定刻み・checkpoint・canonical source時計・CLI/MCP/Metal比較・native GUI確認を経て作業ブランチで受け入れ済み（[記録](../testing/sim-001.md)）。多重時間サンプルのroot合成はM4で実装済みであり、対応範囲と資源上限は [05](05-render-gpu.md) を参照する。
- 2.5D カメラ、完全な 3D、外部プラグイン互換、分散レンダー。

初期から 3D 描画を実装するのではなく、出力ポート・時間・変換型・拡張バージョンの境界を確保する。
初期未対応の機能を保存形式に記録しても、「対応済み」と表示したり、無視して最終レンダーしたりしない（[ADR-0010](../adr/0010-unsupported-features-fail-final-render.md)）。

### 対象プラットフォーム

macOS (Apple Silicon) を先行し、Metal + VideoToolbox を最初の保証経路とする。Windows / Linux は互換経路（CPU 往復を許容）で CI を通し、順次昇格する（[ADR-0015](../adr/0015-macos-first-platform-priority.md)）。

## 共通実行経路

```text
ネイティブ GUI (kronello-ffi) / CLI / MCP
       |
Command API / Query API
       |
Project Service (writer の直列化, revision, policy, audit)
       |
Immutable Snapshot
       |
Document Compiler
  |- Timeline -> placement model
  |- Composition -> scene model
  |- Property / Layout -> dependency graph
  |- Template -> instance bindings
       |
Scene IR + Render DAG
       |
Time/Region Scheduler + Cache + Resource Budget
       |
Text/Vector raster / Video decode / GPU effects / Audio mixer
       |
Preview / Still image / Image sequence / Encoder
```

GUI・CLI・MCP はそれぞれ別プロセスになりうる。各プロセスが同じライブラリ（`kronello-service`）を内包し、同じ `.kronello` ファイルを開く。プロセスをまたぐ書き込みの直列化は SQLite のトランザクションと revision 照合で行う（[09 保存と同時編集](09-storage-concurrency.md)）。

リアルタイム音声コールバックとプロジェクト更新、ディスク読み出し、重い式評価は別の実行系にする。
独立 worker の状態・FIFO・lease は `kronello-jobs`、Windows の native 起動・生存確認・
no-clobber publication は `kronello-platform` に置く。安全な API の外に OS handle を出さず、
model / time / service / jobs の unsafe forbid を維持する。
WindowsはMEDIA-003でFFmpeg loaderとfull CLI/MCPの実media・worker CIまで確認済み。
Windows/Linux GUIとhardware resident decodeは未実装・未保証であり、CLIの成功から保証を広げない
（[ADR-0074](../adr/0074-windows-job-workers-and-process-evidence.md)、
[ADR-0082](../adr/0082-windows-ffmpeg-runtime.md)、[12](12-platform-dependencies.md)）。

素材分析（ASR、音声特徴量、人物マスク等）は不変の DataAsset を生成する外部ジョブとして取り込む。

## 関係グラフを混同しない

| 構造 | 役割 | 不変条件 |
|---|---|---|
| Timeline placement | クリップ配置、トリム、リップル、リンク | 時間区間と対象トラックを明示 |
| Scene containment | 描画順、Group 隔離、所有関係 | 一つの所有親、循環なし |
| Transform parenting | 親の変換を継承 | 描画順・所有親とは独立、循環なし |
| Property / Layout dependencies | 値、式、テキスト計測、制約 | 依存を静的に列挙、循環診断 |
| Render DAG | 色・マスク・画像の処理順序 | 色空間、アルファ、時刻、領域が型付けされる |

ステートフルな Simulation は通常の Property DAG に自己参照を埋め込まず、専用ノードと状態管理を使う。

## 優先順位

実装の優先順は、コアの契約、Command / Query API、GUI の順とする（[ADR-0041](../adr/0041-core-api-gui-order.md)）。機能はまず API として完成させて CLI / MCP で検証できる状態にし、GUI はその上に載せる。GUI にしかない編集機能は作らない。

最優先は TIME、MODEL、PROP、SCENE、TEXT、RENDER の契約。
GUI の装飾、プラグイン数、完全な 3D、クラウド分散はこの後。
タスクの依存関係と受け入れ条件は [backlog](../backlog/README.md) に記載する。
