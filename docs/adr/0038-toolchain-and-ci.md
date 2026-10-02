# ADR-0038: Rust は stable の特定版に固定し、CI は GitHub Actions とする

- 状態: 採用
- 日付: 2026-10-02

## 背景

コードの着手前に、ツールチェーンと検証の基盤を決める必要がある。GPU を使う画素比較はドライバーや機種で結果が変わる。

## 決定

- `rust-toolchain.toml` で stable の特定版に固定する。MSRV はその版とし、定期的に更新する。
- edition は 2024。
- rustfmt と clippy を必須とし、警告をエラーとして扱う。
- CI は GitHub Actions。macOS runner を主とし、Linux はソフトウェア実装の Vulkan で互換経路を検証する。
- CI では値とレイアウトの意味的比較を必須とする。GPU 画素の golden 比較は固定環境（参照機）で実行する。

## 影響

- ツールチェーンの更新で突然 CI が落ちることがない。
- 古い Rust でのビルドは保証しない。
- GPU 画素の golden 比較は CI の必須条件にならないため、参照機での実行を運用として回す必要がある。
- 検討した代替案: stable 最新追従、nightly の許容。

## 関連

- [11 ワークスペース](../architecture/11-workspace.md)
- [13 品質と性能](../architecture/13-quality-performance.md)
