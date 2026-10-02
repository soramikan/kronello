# ADR-0008: CPU / GPU 転送経路を明示する

- 状態: 採用（v0.2 仕様から継承。実装による検証は未了）
- 日付: 2026-10-01

## 背景

高解像度では、フレームが GPU 内に留まるか CPU を往復するかで性能が大きく変わる。暗黙の fallback は性能問題の原因を隠す。

## 決定

- GPU 内コピーと CPU 往復を区別し、使用した経路を `render.explain` で報告する。
- OS / GPU 依存の相互運用は `cinewright-framebridge` に隔離する。
- 非対応経路は明示的な fallback とし、`require_gpu_resident` 指定時はエラーにする。

## 影響

- 経路ごとの転送バイト数と待ち時間を計測できる。
- GPU 経路の保証はプラットフォーム / 形式ごとに段階的に昇格する。

## 関連

- [05 レンダラーと GPU](../architecture/05-render-gpu.md)
- [ADR-0015](0015-macos-first-platform-priority.md)
