# Cinewright

カット編集（NLE）とモーショングラフィックスを、同じ時間・プロパティ・組版・合成・レンダー基盤の上で扱う動画編集ソフトウェア。
人間が使う GUI と、スクリプトや AI エージェントが使う CLI / MCP を同格の入口とし、どこから操作しても同じ Command / Query API・同じ revision・同じレンダー結果に到達することを設計の中心に置く。

> **状態: 設計段階。** このリポジトリには現在ドキュメントしかなく、ソフトウェアの実装・実機性能検証は未着手。
> 文書中の API・CLI・スキーマはすべて提案であり、稼働中の製品の仕様ではない。

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
| コア | Rust（Cargo workspace、`cinewright-*` crate） |
| GPU | wgpu。macOS (Metal) を最初の保証経路とする |
| メディア I/O | FFmpeg（LGPL 構成を動的リンク） |
| 保存 | 単一 SQLite ファイル `.cinewright` |
| GUI | OS ごとのネイティブフレームワーク + 同一プロセス FFI（macOS: SwiftUI / AppKit を先行） |
| 自動化 | 機械向け CLI `cinewright`、MCP サーバー |

## ドキュメント

入口は [docs/README.md](docs/README.md)。

| 文書 | 内容 |
|---|---|
| [docs/architecture/](docs/architecture/00-overview.md) | 設計仕様（章ごとに分割） |
| [docs/adr/](docs/adr/README.md) | アーキテクチャ決定記録 |
| [docs/roadmap/milestones.md](docs/roadmap/milestones.md) | マイルストーン M0〜M6 |
| [docs/roadmap/vertical-slice.md](docs/roadmap/vertical-slice.md) | 最初の縦断テスト作品 |
| [docs/backlog/](docs/backlog/README.md) | 実装バックログ（正本は `backlog.json`） |
| [docs/open-questions.md](docs/open-questions.md) | 未決事項 |
| [docs/glossary.md](docs/glossary.md) | 用語集 |
| [AGENTS.md](AGENTS.md) | コーディングエージェント向けの作業規約 |

## ライセンス

以下のいずれかを選択できるデュアルライセンス。

- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）
- MIT License（[LICENSE-MIT](LICENSE-MIT)）

FFmpeg など外部依存のライセンス条件は [docs/architecture/12-platform-dependencies.md](docs/architecture/12-platform-dependencies.md) を参照。
