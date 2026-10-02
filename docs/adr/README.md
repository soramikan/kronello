# アーキテクチャ決定記録（ADR）

設計上の決定と、その理由・影響を 1 件 1 ファイルで記録する。

## 状態

| 状態 | 意味 |
|---|---|
| 継承 | v0.2 仕様の決定表（ADR-001〜012）から引き継いだもの。採用済みだが、実装による検証は未了 |
| 採用 | 2026-10-02 以降の検討で決定したもの |
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
| [0016](0016-single-file-project.md) | プロジェクトは単一の SQLite ファイル .cinewright とする | 採用 | 2026-10-02 |
| [0017](0017-multi-process-optimistic-concurrency.md) | 複数プロセスからの編集を楽観的 revision 検証で直列化する | 採用 | 2026-10-02 |
| [0018](0018-ffmpeg-lgpl-dynamic-linking.md) | FFmpeg は LGPL 構成を動的リンクする | 採用 | 2026-10-02 |
| [0019](0019-dual-license-mit-apache.md) | ライセンスは MIT OR Apache-2.0 とする | 採用 | 2026-10-02 |
| [0020](0020-backlog-in-repository.md) | バックログの正本をリポジトリ内のファイルとする | 採用 | 2026-10-02 |
| [0021](0021-two-stage-vertical-slice.md) | 最初の縦断テストを M2 と M3 の 2 段階に分ける | 採用 | 2026-10-02 |
| [0022](0022-japanese-canonical-docs.md) | 設計文書は日本語を正本とする | 採用 | 2026-10-02 |
| [0023](0023-naming-cinewright.md) | 名称を Cinewright に変更する | 採用 | 2026-10-02 |
| [0024](0024-working-color-space.md) | 作業用色空間は Sequence ごとに選択し、既定を線形 Rec.709 とする | 採用 | 2026-10-02 |
| [0025](0025-detached-render-workers.md) | レンダージョブは切り離した worker プロセスで実行する | 採用 | 2026-10-02 |
| [0026](0026-selective-undo.md) | Undo は逆操作の発行とし、競合時は拒否する | 採用 | 2026-10-02 |

## 追加と変更の規則

- 新しい決定は次の番号で追加する。[0000-template.md](0000-template.md) を複製して使う。
- 採用済みの ADR の決定内容は書き換えない。変える場合は新しい ADR を追加し、旧 ADR の状態を「置換（ADR-NNNN）」にする。
- 決定を変えたら、対応する `docs/architecture/` の章を同じ変更で更新する。
- まだ決められない論点は [未決事項](../open-questions.md) に置く。
