# ADR-0031: FFI は細い C ABI と JSON payload で構成する

- 状態: 採用
- 日付: 2026-10-02

## 背景

ネイティブ GUI は Rust コアを同一プロセス FFI で呼ぶ（ADR-0014）。macOS は Swift、Windows は C#、Linux は C から使う（ADR-0032）。GUI も CLI / MCP と同じ Command / Query API を通らなければならない（ADR-0001）。

## 決定

- `cinewright-ffi` は少数の関数からなる C ABI を公開する: プロジェクトを開く・閉じる、Command / Query の呼び出し、通知の購読、プレビュー面の接続・サイズ変更・再描画要求、メモリの解放。
- Command / Query の要求と応答は、CLI / MCP と同じ JSON で受け渡す。
- 各言語の型付きラッパーは公開 JSON Schema（ADR-0029）から生成する。
- 画素は JSON を通さず、GPU の描画面で直接受け渡す。

## 影響

- 3 つの入口が同じ payload を通るため、同等性の検証（QA-002）が単純になる。
- C ABI はどの言語からも呼べる。
- 呼び出しごとに JSON の直列化コストがかかる。編集操作の頻度では問題にならない想定だが、ドラッグ中の連続プレビューなど高頻度の経路は FFI-001 で計測する。
- 検討した代替案: UniFFI、swift-bridge、スパイクで比較してから決定。

## 関連

- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
- [ADR-0029](0029-public-json-schema.md)
