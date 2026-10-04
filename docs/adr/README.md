# アーキテクチャ決定記録（ADR）

設計上の決定と、その理由・影響を 1 件 1 ファイルで記録する。

## 状態

| 状態 | 意味 |
|---|---|
| 継承 | v0.2 仕様の決定表（ADR-001〜012）から引き継いだもの。採用済みだが、実装による検証は未了 |
| 採用 | 2026-10-02 以降の検討で決定したもの |
| 部分置換 | 決定の一部だけを後の ADR に置き換えたもの。置換範囲と維持する条項を明記する |
| 置換 | 後の ADR に置き換えられたもの。ファイルは残し、置き換えた ADR へリンクする |

## 一覧

| 番号 | 決定 | 状態 | 日付 |
|---|---|---|---|
| [0001](0001-shared-command-query-api.md) | GUI / CLI / MCP は同じ Command / Query API を使う | 継承 | 2026-10-01 |
| [0002](0002-timeline-composition-separate-models.md) | Timeline と Composition は別モデルとし、共通 IR へ変換する | 継承 | 2026-10-01 |
| [0003](0003-pure-evaluation-at-arbitrary-time.md) | 通常アニメーションは任意時刻の純粋評価とする | 継承 | 2026-10-01 |
| [0004](0004-exclusive-property-source.md) | Property の主値源は Constant / Curve / Expression のいずれか一つ | 継承 | 2026-10-01 |
| [0005](0005-semantic-snapshot-vs-gpu-resources.md) | 意味的スナップショットと GPU 資源を分離する | 継承 | 2026-10-01 |
| [0006](0006-document-vs-render-cache.md) | 文書保存とレンダーキャッシュを分離する | 継承 | 2026-10-01 |
| [0007](0007-template-definition-vs-instance-inputs.md) | テンプレート定義とインスタンス入力を分離する | 継承 | 2026-10-01 |
| [0008](0008-explicit-cpu-gpu-transfer-paths.md) | CPU / GPU 転送経路を明示する | 継承 | 2026-10-01 |
| [0009](0009-sandboxed-expression-evaluation.md) | 式評価にネットワーク・ファイル・時計・非固定乱数を与えない | 継承 | 2026-10-01 |
| [0010](0010-unsupported-features-fail-final-render.md) | 未対応機能は保持できても最終出力は失敗させる | 継承 | 2026-10-01 |
| [0011](0011-local-sqlite-source-of-truth.md) | 初期の正本はローカル SQLite とする | 継承 | 2026-10-01 |
| [0012](0012-hdr-compositing-vs-vector-rasterizer.md) | HDR 映像合成とベクターラスタライザーを分離する | 継承 | 2026-10-01 |
| [0013](0013-naming-koma.md) | 名称を koma に統一する | 置換（0023） | 2026-10-02 |
| [0014](0014-native-gui-in-process-ffi.md) | GUI は OS ネイティブフレームワークで実装し、Rust コアを同一プロセス FFI で呼ぶ | 採用 | 2026-10-02 |
| [0015](0015-macos-first-platform-priority.md) | macOS (Apple Silicon) を先行プラットフォームとする | 採用 | 2026-10-02 |
| [0016](0016-single-file-project.md) | プロジェクトは単一の SQLite ファイル .kronello とする | 採用 | 2026-10-02 |
| [0017](0017-multi-process-optimistic-concurrency.md) | 複数プロセスからの編集を楽観的 revision 検証で直列化する | 採用 | 2026-10-02 |
| [0018](0018-ffmpeg-lgpl-dynamic-linking.md) | FFmpeg は LGPL 構成を動的リンクする | 採用 | 2026-10-02 |
| [0019](0019-dual-license-mit-apache.md) | ライセンスは MIT OR Apache-2.0 とする | 採用 | 2026-10-02 |
| [0020](0020-backlog-in-repository.md) | バックログの正本をリポジトリ内のファイルとする | 採用 | 2026-10-02 |
| [0021](0021-two-stage-vertical-slice.md) | 最初の縦断テストを M2 と M3 の 2 段階に分ける | 採用 | 2026-10-02 |
| [0022](0022-japanese-canonical-docs.md) | 設計文書は日本語を正本とする | 採用 | 2026-10-02 |
| [0023](0023-naming-cinewright.md) | 名称を Cinewright に変更する | 置換（0042） | 2026-10-02 |
| [0024](0024-working-color-space.md) | 作業用色空間は Sequence ごとに選択し、既定を線形 Rec.709 とする | 採用 | 2026-10-02 |
| [0025](0025-detached-render-workers.md) | レンダージョブは切り離した worker プロセスで実行する | 採用 | 2026-10-02 |
| [0026](0026-selective-undo.md) | Undo は逆操作の発行とし、競合時は拒否する | 採用 | 2026-10-02 |
| [0027](0027-wal-single-file-on-close.md) | 開いている間は WAL、閉じるときに単一ファイルへ戻す | 採用 | 2026-10-02 |
| [0028](0028-asset-references-and-relink.md) | 素材は相対パスと絶対パスの両方で参照し、hash で検証する | 採用 | 2026-10-02 |
| [0029](0029-public-json-schema.md) | 版付きの公開 JSON スキーマを一つ定義する | 採用 | 2026-10-02 |
| [0030](0030-history-retention.md) | 履歴は既定で全保持し、明示的な compact で切り詰める | 採用 | 2026-10-02 |
| [0031](0031-ffi-c-abi-json.md) | FFI は細い C ABI と JSON payload で構成する | 採用 | 2026-10-02 |
| [0032](0032-windows-winui-linux-gtk.md) | Windows の GUI は WinUI 3、Linux の GUI は GTK4 とする | 採用 | 2026-10-02 |
| [0033](0033-ui-state-in-user-state-area.md) | GUI の UI 状態はユーザーごとの状態領域に保存する | 採用 | 2026-10-02 |
| [0034](0034-job-retention.md) | ジョブの記録は残し、ジョブディレクトリは 30 日で掃除する | 採用 | 2026-10-02 |
| [0035](0035-software-encoders.md) | ソフトウェアエンコードは AV1・ProRes・画像連番とする | 採用 | 2026-10-02 |
| [0036](0036-ffmpeg-distribution.md) | リリースには自前の LGPL ビルドの FFmpeg を同梱する | 採用 | 2026-10-02 |
| [0037](0037-hdr-policy.md) | HDR は BT.2408 の基準白と Rec.2100 の PQ / HLG に従う | 採用 | 2026-10-02 |
| [0038](0038-toolchain-and-ci.md) | Rust は stable の特定版に固定し、CI は GitHub Actions とする | 部分置換（0047: golden 環境のみ） | 2026-10-02 |
| [0039](0039-test-fixtures.md) | テスト素材は生成と CC0 / OFL に限り、小さいものだけ同梱する | 採用 | 2026-10-02 |
| [0040](0040-expression-language-policy.md) | 式の正本は AST とし、人間向けには小さな式言語を後から追加する | 採用 | 2026-10-02 |
| [0041](0041-core-api-gui-order.md) | 実装の優先順はコア契約、API、GUI の順とする | 採用 | 2026-10-02 |
| [0042](0042-naming-kronello.md) | 名称を Kronello に変更する | 採用 | 2026-10-02 |
| [0043](0043-semantic-dependencies-and-units.md) | 編集モデル・評価・レンダーの依存境界と単位を固定する | 採用 | 2026-10-03 |
| [0044](0044-color-and-alpha-contracts.md) | 作業用線形色と alpha の入出力契約を固定する | 採用 | 2026-10-03 |
| [0045](0045-snapshot-compatibility-boundaries.md) | 保存・意味・実行能力の互換性を分けて判定する | 採用 | 2026-10-03 |
| [0046](0046-store-format-and-location-policy.md) | 保存の外枠・安全モード判定・履歴警告を固定する | 採用 | 2026-10-03 |
| [0047](0047-apple-silicon-metal-golden.md) | GPU golden は Apple Silicon + Metal の共通基準で比較する | 採用 | 2026-10-03 |
| [0048](0048-media-native-build-and-asset-verification.md) | FFmpeg ABI 境界・同梱ビルド・素材検証を固定する | 採用 | 2026-10-03 |
| [0049](0049-audio-bus-timing-and-codec.md) | 音声 Bus・サンプル格子・PCM24 の書き出しを固定する | 採用 | 2026-10-04 |
| [0050](0050-fixed-job-execution-and-publication.md) | 固定ジョブ入力・実行 lease・成果物確定を共有 service で扱う | 採用 | 2026-10-04 |
| [0051](0051-nle-placement-and-retime.md) | Sequence の配置・合成順序と三つの時間編集を固定する | 採用 | 2026-10-04 |
| [0052](0052-snapshot-policy-evaluation.md) | サイズ閾値による追加 snapshot の既定採用を見送る | 採用 | 2026-10-04 |
| [0053](0053-integration-evaluated-queries-and-render-tiles.md) | 縦断デモの評価 query と大解像度の tile 実行 | 採用 | 2026-10-04 |
| [0054](0054-gui-design-system.md) | GUI の見た目を全 OS 共通のデザインシステムで定める | 採用 | 2026-10-04 |
| [0055](0055-main-window-pages-and-workspaces.md) | メインウインドウをページとワークスペースで構成する | 採用 | 2026-10-04 |
| [0056](0056-native-ffi-worker-and-swiftpm.md) | native FFI の非同期 worker と SwiftPM 境界 | 採用 | 2026-10-04 |
| [0057](0057-layout-bounds-stages.md) | bounds の三段階を純粋値と明示した帯追従 policy で共有する | 採用 | 2026-10-04 |
| [0058](0058-bounded-canonical-expression-ast.md) | 正規 postorder AST と有界 Expression 評価 | 採用 | 2026-10-04 |
| [0059](0059-template-duration-variants-and-migration.md) | テンプレートの保護尺・variant・data と明示した版移行を共有する | 採用 | 2026-10-05 |
| [0060](0060-structured-read-only-inspection.md) | 非表示原因と実行前レンダー計画を読み取り Query で共有する | 採用 | 2026-10-05 |
| [0062](0062-video-generator-and-timeline-edits.md) | 動画・Generator Clip と明示した Timeline 編集範囲 | 採用 | 2026-10-05 |

## 追加と変更の規則

- 新しい決定は次の番号で追加する。[0000-template.md](0000-template.md) を複製して使う。
- 採用済みの ADR の決定内容は書き換えない。変える場合は新しい ADR を追加し、旧 ADR の状態を「置換（ADR-NNNN）」にする。
- 一部だけを変える場合は旧 ADR を「部分置換（ADR-NNNN）」とし、旧 ADR と新 ADR に置換範囲・維持する条項を明記する。
- 決定を変えたら、対応する `docs/architecture/` の章を同じ変更で更新する。
- まだ決められない論点は [未決事項](../open-questions.md) に置く。
