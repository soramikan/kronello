# Kronello

カット編集（NLE）とモーショングラフィックスを、同じ時間・プロパティ・組版・合成・レンダー基盤の上で扱う動画編集ソフトウェア。
人間が使う GUI と、スクリプトや AI エージェントが使う CLI / MCP を同格の入口とし、どこから操作しても同じ Command / Query API・同じ revision・同じレンダー結果に到達することを設計の中心に置く。

> **状態: M0・M1・M2のP0・M3・M4の正式受け入れを完了。** Rustコア、共有CLI / MCP、macOSのEdit / Motion / Template / Exportページ、実時間音声とstreaming書き出しを実装した。M4のHDR、復旧、キャッシュ、性能・OS別検証の状態は[統合受け入れ記録](docs/testing/m4-acceptance.md)と[バックログ](docs/backlog/BACKLOG.md)を参照。M2のSTORE-003の実環境残件は別範囲。M5は作業ブランチで12件を受け入れ済み。NAME-001は名称確保の所有者判断が保留で進行中のため、全13件の完了ではない（[進捗記録](docs/testing/m5-acceptance.md)）。mainへの統合済みとは扱わない。M6は未着手。詳細は[現在の実装範囲と残件](docs/roadmap/implementation-status.md)を参照。
> 実装と保証範囲の正本は [backlog](docs/backlog/BACKLOG.md) と 各タスクの検証記録（[M3](docs/testing/m3-acceptance.md) / [M4](docs/testing/m4-acceptance.md) / [M5進捗](docs/testing/m5-acceptance.md)）。設計文書中の提案APIを、そのまま実装済み仕様と扱わない。

## 何を作るか

- **Timeline と Composition は別の編集モデル。** カット編集のリップルと、モーションの空間階層を混ぜない。両者は共通の Scene IR / Render DAG へコンパイルされる。
- **任意時刻の純粋評価。** 有理数時間とサブフレーム評価により、シーク・逆順レンダー・再試行が同じ結果になる。
- **日本語組版を一級市民に。** 禁則・ルビ・縦書き、書記素クラスタを壊さない文字アニメーションを段階的に実装する。
- **テンプレート。** 公開入力と保護時間区間（intro / outro）を持ち、尺・文言・縦横比を変えて再利用できる。
- **GUI / CLI / MCP 同格。** 計画 → プレビュー → 検証 → 適用のトランザクション API を全入口で共有する。
- **黙って劣化しない。** 未対応機能・式の失敗・フォント欠落は最終レンダーをエラーにし、代替表示で誤魔化さない。

## 技術基盤

| 領域 | 採用・候補 |
|---|---|
| コア | Rust（Cargo workspace、`kronello-*` crate） |
| GPU | wgpu。macOS (Metal) を最初の保証経路とする |
| メディア I/O | FFmpeg（LGPL 構成を動的リンク） |
| 保存 | 単一 SQLite ファイル `.kronello` |
| GUI | OS ごとのネイティブフレームワーク + 同一プロセス FFI（macOS: SwiftUI / AppKit を先行） |
| 自動化 | 機械向け CLI `kronello`、MCP サーバー |

## ドキュメント

入口は [docs/README.md](docs/README.md)。

| 文書 | 内容 |
|---|---|
| [docs/architecture/](docs/architecture/00-overview.md) | 設計仕様（章ごとに分割） |
| [docs/adr/](docs/adr/README.md) | アーキテクチャ決定記録 |
| [docs/roadmap/milestones.md](docs/roadmap/milestones.md) | マイルストーン M0〜M6 |
| [docs/roadmap/vertical-slice.md](docs/roadmap/vertical-slice.md) | 最初の縦断テスト作品 |
| [docs/backlog/](docs/backlog/README.md) | 実装バックログ（正本は `backlog.json`） |
| [docs/roadmap/implementation-status.md](docs/roadmap/implementation-status.md) | 現在の実装範囲、未完了・未検証・保証外と後続タスク |
| [docs/open-questions.md](docs/open-questions.md) | 未決事項 |
| [docs/glossary.md](docs/glossary.md) | 用語集 |
| [AGENTS.md](AGENTS.md) | コーディングエージェント向けの作業規約 |

## 開発時の検証

Rust は `rust-toolchain.toml` の 1.95.0（edition 2024）に固定する。rustup を導入した環境で、リポジトリのルートから実行する。

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/backlog.py check
```

CI は macOS (Apple Silicon) と Linux (Mesa lavapipe) でworkspaceを検証し、Windowsでは固定LGPL runtimeとCLI/MCPのbuild、実media、worker、保存層を検証する。Linux Vulkan / Windows DX12のsoftware adapter画像比較も別jobで実行する。GPU画素は [固定環境のgolden比較手順](docs/testing/golden-comparison.md) と [最終Metal 40 scene比較の記録](docs/testing/m3-acceptance.md) を参照。

macOSの開発アプリは [apps/macos/README.md](apps/macos/README.md) の手順で `python3 scripts/build_macos_app.py --release` を実行すると、`target/macos/Kronello.app` に組み立てる。配布用signing / notarization済みの製品ではない。

## ライセンス

以下のいずれかを選択できるデュアルライセンス。

- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）
- MIT License（[LICENSE-MIT](LICENSE-MIT)）

FFmpeg など外部依存のライセンス条件は [docs/architecture/12-platform-dependencies.md](docs/architecture/12-platform-dependencies.md) を参照。
