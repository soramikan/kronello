# ADR-0019: ライセンスは MIT OR Apache-2.0 とする

- 状態: 採用
- 日付: 2026-10-02

## 背景

OSS として公開する方針。Rust エコシステムでは MIT と Apache-2.0 のデュアルライセンスが標準的で、依存 crate の多くと両立する。

## 決定

- Cinewright 本体は `MIT OR Apache-2.0` のデュアルライセンスとする。
- GPL / AGPL の依存を追加しない。

## 影響

- 採用側の制約が小さい。
- GPL のコーデック実装を同梱できない（ADR-0018）。

## 関連

- [12 プラットフォームと依存](../architecture/12-platform-dependencies.md)
