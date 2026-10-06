# ADR-0047: GPU golden は Apple Silicon + Metal の共通基準で比較する

- 状態: 一部置換（比較環境・同梱合計サイズは [ADR-0088](0088-platform-golden-baselines.md)）
- 日付: 2026-10-03
- 部分置換: [ADR-0038](0038-toolchain-and-ci.md) の GPU 画素 golden 固定環境（参照機）の条項のみ

## 背景

M1 開発機では 21 シーンの GPU / CPU oracle 比較と候補生成を確認済みだが、M4 の機種・OS・driver を固定した基準画像は未登録だった。機種ごとの固定環境を要求すると開発機で回帰比較を運用できない。性能計測の基準機と画素回帰の対象環境を分ける。

## 決定

- GPU golden の比較環境は Apple Silicon ネイティブ（`aarch64-apple-darwin`）かつ選択 adapter の backend が `Metal` であることだけを要求する。Rosetta、Intel Mac、Vulkan はこの共通基準の対象外。
- `tests/golden/apple-silicon-metal/` に一つの基準集合を持ち、今回 M1 開発機の実測から登録する。機種・macOS 版ごとの基準は作らない。
- adapter 名、OS / build、機種、メモリ、driver、Rust / wgpu 版、revision、入力とコードの hash は provenance として保存し、比較の可否判定には使わない。描画入力・シーン設定と比較方式の版は厳密に照合する。
- `compare_pixels` の既定値 `2^-10` を維持し、全画素・全成分を検査する。CPU oracle 比較も維持する。基準欠落、対象ゼロ、非有限値を失敗とする。
- UPDATE は候補だけを生成する。採用は明示的な `scripts/golden_adopt.py` で行い、dirty working tree、dirty 候補、候補 revision と HEAD の不一致、hash 不一致を拒否する。基準 manifest は hash、シーン設定、比較方式・許容誤差の版と provenance を含む。
- ADR-0038 のツールチェーン・CI・意味的比較・golden を CI 必須にしない決定は維持する。性能計測の第一基準機は M4 Mac mini 32GB のまま。

## 影響

- M1 を含む Apple Silicon + Metal で同一基準を比較できる。OS / driver 更新は provenance の差分として調査できる。
- 同一環境の M1 比較は世代間の許容誤差校正を保証しない。Vulkan / Windows の基準と世代間の校正は QA-004 で行う。
- 基準採用前にコードをコミットする必要がある。基準は別コミットで登録し、生成 revision を保存する。
- 同梱 fixture は 1 ファイル 256 KiB・合計 1 MiB 以下を採用スクリプトで検証する。

## 関連

- [golden 比較手順](../testing/golden-comparison.md)
- [13 品質と性能](../architecture/13-quality-performance.md)
- [ADR-0039](0039-test-fixtures.md)
