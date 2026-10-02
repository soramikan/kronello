# ADR-0015: macOS (Apple Silicon) を先行プラットフォームとする

- 状態: 採用
- 日付: 2026-10-02

## 背景

FrameBridge とハードウェアデコード / エンコードは OS・GPU ごとに実装と検証が必要で、3 OS を同じ水準で同時に進めると M0〜M2 の検証コストが大きい。開発機は macOS。

## 決定

- macOS (Apple Silicon) の Metal + VideoToolbox を最初の保証経路とする。
- Windows / Linux は互換経路（CPU 往復を許容）で CI を通し、結果の意味が同じであることを保つ。GPU 経路の保証は順次昇格する。
- GUI も macOS から実装する。

## 影響

- Windows / Linux では当面、性能が劣る経路になる。使用経路は `render.explain` で報告する。
- プラットフォーム固有の前提がコアに漏れないよう、相互運用は `kronello-framebridge` に隔離する。

## 関連

- [12 プラットフォームと依存](../architecture/12-platform-dependencies.md)
- [ADR-0008](0008-explicit-cpu-gpu-transfer-paths.md)
