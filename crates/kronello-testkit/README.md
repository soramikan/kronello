# kronello-testkit

意味値の厳密比較と、線形 premultiplied RGBA の CPU 画素比較を提供する test 用 crate。
GPU、FFmpeg、編集モデルに依存しない。scene の解析的期待値と比較 API を通常テストで検証する。
`resolve_fixture` は manifest の ID から同梱・外部素材を解決し、byte 数と SHA-256 を検証する。
外部素材は Python で事前取得する。未取得の場合は通常の Rust テストも失敗する。
実際の renderer や Metal の基準画像は未実装。

[fixture と検証手順](../../docs/testing/fixtures.md) を参照。

```sh
python3 scripts/fetch_fixtures.py
cargo test -p kronello-testkit --locked
```
