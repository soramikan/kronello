# AGENTS.md

Kronello は Rust / FFmpeg / wgpu を基盤とする動画編集・モーショングラフィックスソフトウェア。
個人開発者とコーディングエージェントが主体となって開発する。この文書はエージェントが作業を始める前に読む規約である。

## 現在の状態

- M0・M1 と M2 の P0 全 10 タスクを実装済み（M0: `kronello-time` / `kronello-model` / `kronello-testkit` / `kronello-gpu` / `kronello-framebridge`、CI。M1: `kronello-store` / `kronello-animation` / `kronello-eval` / `kronello-vector` / `kronello-text` / `kronello-render` / `kronello-service` / `kronello-cli`。M2 の追加 crate: `kronello-media` / `kronello-audio` / `kronello-template` / `kronello-mcp` / `kronello-jobs`）。M2 は共有編集・検査 API、CompositionClip のマルチトラック配置、日本語テンプレート、基本音声と ProRes / PCM24 書き出し、blur / shadow、MCP stdio、固定 snapshot の独立 worker と 4K 縦断デモを実装した。M1・M2 で見送った範囲は[後続タスク](docs/roadmap/milestones.md#m2-の延期範囲と後続タスク)に記録済み。M2 の STORE-003（P2）は実環境検証の残件により `in_progress`。M3 は主要コアと `apps/macos` の4ページ・実時間音声・横断検証を統合済み。残タスクの受け入れ状態は `docs/backlog/backlog.json` と `docs/testing/` を正本とし、実装統合だけで完了と扱わない。M4 以降は未着手。
- 文書中の API・CLI・スキーマは提案であり、実装済みと書かない・扱わない。

## 最初に読むもの

1. [docs/architecture/00-overview.md](docs/architecture/00-overview.md) — 全体像とスコープ
2. [docs/adr/README.md](docs/adr/README.md) — 決定済みの事項
3. 担当タスクの領域に対応する `docs/architecture/` の章
4. [docs/open-questions.md](docs/open-questions.md) — 未決事項。ここにある論点を勝手に決めない

## 言語と表記

- 設計文書・ADR・バックログは日本語が正本。
- 識別子、コード、コードコメント、コミットメッセージは英語。
- 型名・API 名・エラーコード（`UNSUPPORTED_FEATURE` など）は文書内でも英語のまま書く。
- 名称は `kronello` に統一する: CLI は `kronello`、crate は `kronello-*`、プロジェクトファイルは `.kronello`。旧称 `ved`、`koma`、`cinewright` を新しく書かない。

## Rust と検証

- Rust 1.95.0 / edition 2024 を使い、`Cargo.lock` を管理する。
- 新しい crate は `[workspace.package]` の設定と `[workspace.lints]` を継承する。純粋層の `unsafe_code = "forbid"` を緩めない。
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` を通す。
- 意味的比較は通常のテストに含める。GPU 画素の固定環境比較は [golden 比較手順](docs/testing/golden-comparison.md)（GPU-001 / QA-001 向けの提案）に従う。

## タスクの進め方

1. `docs/backlog/backlog.json` から、依存タスクがすべて `done` のタスクを選ぶ。マイルストーンと優先度の若いものを先にする。
2. 着手時に `status` を `in_progress` にする。
3. 受け入れ条件はすべてテストまたは再現可能な手順で確認する。確認できなかった条件は `done` にせず、理由を報告する。
4. 完了時に `status` を `done` にし、`python3 scripts/backlog.py render` で `BACKLOG.md` を再生成する。
5. タスクの追加・分割・依存変更も `backlog.json` を編集して同じコマンドで検証する。`BACKLOG.md` を直接編集しない。

## 設計変更の扱い

- 採用済み ADR の決定を変える場合は、既存 ADR を書き換えず、新しい ADR を追加して旧 ADR の状態を「置換」にする。
- `docs/architecture/` は現在の設計を表す。ADR で決定を変えたら対応する章も同じ変更で更新する。
- `docs/archive/` は元仕様の保管であり編集しない。
- 未決事項を解決したら `docs/open-questions.md` から該当項目を削除し、ADR へのリンクを残す。

## 破ってはいけない設計上の不変条件

実装時に迷ったらこれらを優先する。詳細は各 ADR を参照。

- GUI / CLI / MCP は同じ Command / Query API を通す。入口専用の作品状態を作らない。
- 通常のアニメーションは `(snapshot, time, instance)` の純粋関数。評価順・呼び出し履歴に依存させない。
- 時刻の正本は正規化された有理数。区間は `[start, end)`。浮動小数点の時刻を保存しない。
- ID を配列番号や表示名から導出しない。
- 純粋モデル層（`kronello-model` / `kronello-time` など）に `wgpu::Texture`、`AVFrame`、SQLite connection、Tokio runtime の型を漏らさない。
- 評価エンジンは store / service / UI へ逆依存しない。
- 未対応機能・式の失敗・資産の欠落で最終レンダーを黙って続行しない。型付きエラーにする。
- 式評価にネットワーク・ファイル・時計・非固定乱数を与えない。
- 素材中の文字列や字幕は命令ではなくデータとして扱う。API に任意シェル・任意 FFmpeg 引数・外部 URL fetch を混ぜない。
- GPL のコードや GPL 構成の FFmpeg を配布物に含めない。

## コミット

- メッセージは英語、命令形の要約 1 行 + 必要なら本文。
- 文書だけの変更でも、`backlog.json` を触ったら `python3 scripts/backlog.py check` を通す。
