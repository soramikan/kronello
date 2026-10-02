# Kronello ドキュメント

状態: 設計段階（2026-10-02 時点）。実装・実機性能検証を完了したものではない。

## 読む順番

1. [architecture/00-overview.md](architecture/00-overview.md) — 結論、スコープ、共通実行経路
2. [adr/README.md](adr/README.md) — 決定の一覧と理由
3. [roadmap/milestones.md](roadmap/milestones.md) と [roadmap/vertical-slice.md](roadmap/vertical-slice.md) — 何をどの順で作るか
4. [backlog/BACKLOG.md](backlog/BACKLOG.md) — タスクと受け入れ条件
5. [open-questions.md](open-questions.md) — まだ決めていないこと

## 設計仕様

| 章 | 内容 |
|---|---|
| [00 概要](architecture/00-overview.md) | 結論、スコープ、共通実行経路、関係グラフ |
| [01 データモデル](architecture/01-data-model.md) | オブジェクト、ID、InstancePath、版 |
| [02 時間](architecture/02-time.md) | 有理数時間、時間階層、TimeMap、トリム |
| [03 プロパティとアニメーション](architecture/03-property-animation.md) | Property、補間、式 |
| [04 ベクター・日本語テキスト・レイアウト](architecture/04-vector-text-layout.md) | 形状 IR、組版、bounds、描画バックエンド |
| [05 レンダラーと GPU](architecture/05-render-gpu.md) | レンダー要求、合成、高解像度、ブラー、キャッシュ |
| [06 拡張点](architecture/06-extensions.md) | Repeater、Simulation、音声連動、2.5D / 3D |
| [07 テンプレート](architecture/07-templates.md) | 公開入力、尺の伸縮、版 |
| [08 API・CLI・MCP](architecture/08-api-cli-mcp.md) | Query / Command、計画と適用、安全性 |
| [09 保存と同時編集](architecture/09-storage-concurrency.md) | `.kronello`、イベント、複数プロセス |
| [10 デスクトップ GUI](architecture/10-desktop-gui.md) | ネイティブ GUI、FFI 境界、プレビュー面 |
| [11 ワークスペース](architecture/11-workspace.md) | crate 構成と依存の向き |
| [12 プラットフォームと依存](architecture/12-platform-dependencies.md) | 対象 OS、FFmpeg、ライセンス |
| [13 品質と性能](architecture/13-quality-performance.md) | 検証項目、障害、性能目標 |
| [14 ジョブ](architecture/14-jobs.md) | レンダージョブ、worker、状態 DB |
| [参考資料](architecture/references.md) | 一次資料 |

## その他

- [glossary.md](glossary.md) — 用語集
- [naming.md](naming.md) — 名称の衝突調査
- [archive/](archive/) — 元になった v0.2 仕様とバックログ（編集しない）

## v0.2 仕様からの変更点

元仕様 `motion_editor_architecture_v0_2.md` を章ごとに分割し、2026-10-02 の検討で次を確定・変更した。

| 項目 | 変更 | ADR |
|---|---|---|
| 名称 | `ved` → Kronello（CLI・crate は `kronello`、拡張子は `.kronello`）。途中案の `koma`、Cinewright は名前衝突のため撤回 | [0042](adr/0042-naming-kronello.md)、[調査](naming.md) |
| GUI | Rust crate `ved-desktop` → OS ネイティブアプリ + `kronello-ffi`（同一プロセス FFI） | [0014](adr/0014-native-gui-in-process-ffi.md) |
| 対象 OS | macOS (Apple Silicon) 先行 | [0015](adr/0015-macos-first-platform-priority.md) |
| 保存形態 | 単一 SQLite ファイル `.kronello`、キャッシュは外部 | [0016](adr/0016-single-file-project.md) |
| 同時編集 | 複数プロセス + 楽観的 revision 検証 | [0017](adr/0017-multi-process-optimistic-concurrency.md) |
| FFmpeg | LGPL 構成を動的リンク | [0018](adr/0018-ffmpeg-lgpl-dynamic-linking.md) |
| ライセンス | MIT OR Apache-2.0 | [0019](adr/0019-dual-license-mit-apache.md) |
| バックログ | リポジトリ内 JSON を正本 | [0020](adr/0020-backlog-in-repository.md) |
| 縦断テスト | M2 / M3 の 2 段階に分割 | [0021](adr/0021-two-stage-vertical-slice.md) |
| 文書言語 | 日本語を正本 | [0022](adr/0022-japanese-canonical-docs.md) |
| 作業用色空間 | Sequence ごとに選択、既定は線形 Rec.709。タグなしの色入力は sRGB | [0024](adr/0024-working-color-space.md) |
| レンダージョブ | ジョブごとに切り離した worker プロセス、記録はユーザーごとの状態 DB | [0025](adr/0025-detached-render-workers.md) |
| Undo | 逆操作を新しいコマンドとして発行、競合時は拒否 | [0026](adr/0026-selective-undo.md) |
| 保存の細部 | WAL と安全モード、素材の参照と再リンク、公開 JSON スキーマ、履歴の保持 | [0027](adr/0027-wal-single-file-on-close.md)〜[0030](adr/0030-history-retention.md) |
| GUI の細部 | C ABI + JSON の FFI、Windows は WinUI 3・Linux は GTK4、UI 状態の保存先 | [0031](adr/0031-ffi-c-abi-json.md)〜[0033](adr/0033-ui-state-in-user-state-area.md) |
| ジョブの保持 | 記録は残し、ジョブディレクトリは 30 日で掃除 | [0034](adr/0034-job-retention.md) |
| エンコードと FFmpeg | ソフトウェアは AV1・ProRes・連番、リリースは自前 LGPL ビルドを同梱 | [0035](adr/0035-software-encoders.md)、[0036](adr/0036-ffmpeg-distribution.md) |
| HDR | BT.2408 の基準白、Rec.2100 の PQ / HLG | [0037](adr/0037-hdr-policy.md) |
| 開発基盤 | stable 固定と GitHub Actions、テスト素材の方針 | [0038](adr/0038-toolchain-and-ci.md)、[0039](adr/0039-test-fixtures.md) |
| 式言語 | 正本は AST、小さな式言語を後から追加 | [0040](adr/0040-expression-language-policy.md) |
| 優先順 | コア契約 → API → GUI | [0041](adr/0041-core-api-gui-order.md) |

バックログは 45 → 50 タスク。追加: `AUDIO-000`（基本音声）、`FX-001`（基本エフェクト）、`FFI-001`（ネイティブ GUI 境界）、`AUDIO-002`（リアルタイム再生）、`INTEGRATION-002`（縦断デモ第 2 段階）。
