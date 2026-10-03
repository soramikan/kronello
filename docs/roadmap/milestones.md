# マイルストーン

状態: M0・M1 は完了（2026-10-03）。M1 の統合点は `scripts/demo_cli_m1.py`（[CLI-001 の検証](../testing/cli-001.md)）。M2 以降は未着手。タスクの詳細は [backlog](../backlog/BACKLOG.md)。

| 段階 | 成果物 | 主な完了条件 | タスク数 |
|---|---|---|---:|
| M0 | 基盤契約・テスト素材・CI・技術スパイク | 有理数時刻 / ID / Property / 色 / alpha 規約、ツールチェーンと CI、2D title から GPU 出力の最短経路（macOS） | 6 |
| M1 | Headless 2D Motion Core | Shape / Text / Group / Null、キーフレーム、任意時刻レンダー、画像連番 | 10 |
| M2 | NLE 統合・CLI / MCP | CompositionClip、日本語 title、基本音声、基本エフェクト、固定 snapshot、計画 / 適用、書き出し、縦断デモ第 1 段階 | 10 |
| M3 | 実用的な Motion Authoring | macOS ネイティブ GUI（canvas / curve editor）、テンプレート拡張、基本式、responsive layout、リアルタイム再生、縦断デモ第 2 段階 | 10 |
| M4 | 高品質・高解像度 | サブフレームブラー、temporal cache、8K / HDR 品質、GPU 経路診断 | 6 |
| M5 | 高度な 2D Motion | Repeater、path 演出、音声連動、ルビ・縦書き、Simulation | 5 |
| M6 | 拡張 | 2.5D、外部レンダー、互換アダプター、プラグイン、分散 | 4 |

## 方針

- M0 で GPU interop の困難さを確認するが、zero-copy の完全達成を M1 の CPU 検証版まで阻害する必須条件にはしない。
- M1 / M2 は互換経路でも実装を進め、転送コストを明示する。GPU 経路の保証はプラットフォーム / 形式ごとに昇格する。
- macOS (Apple Silicon) を先行する。Windows / Linux は互換経路で CI を通し、M4（GPU-003）以降に保証経路を昇格する。
- M0〜M2 は GUI なしで進める。GUI は M3 で macOS から着手する。
- 各マイルストーンの完了は、属するタスクの受け入れ条件がすべて確認できたことで判定する。

## マイルストーンごとの統合点

| 段階 | 統合の確認 |
|---|---|
| M1 | CLI-001: GUI なしで Shape と日本語 Text のアニメーション連番を生成 |
| M2 | INTEGRATION-001: [縦断デモ第 1 段階](vertical-slice.md) |
| M3 | INTEGRATION-002: [縦断デモ第 2 段階](vertical-slice.md)、QA-002: GUI / CLI / MCP 同等性 |
| M4 | PERF-001: 参照シーンの benchmark |
