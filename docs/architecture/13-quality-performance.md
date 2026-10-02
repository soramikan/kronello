# 13 品質・性能の検証

縦断テスト作品は [roadmap/vertical-slice.md](../roadmap/vertical-slice.md) を参照。

## 比較の方針

- 値とレイアウトの比較は厳密に行う（意味的比較）。
- GPU 画素の比較は、固定環境の基準画像と許容誤差で行う。
- 異なる GPU / CPU 間の浮動小数点のビット一致は約束しない。

## テスト素材

[ADR-0039](../adr/0039-test-fixtures.md) による。

- 映像と音声は、可能な限りスクリプトで生成する（テストパターン、サイン波、VFR など）。
- 生成できないものは CC0 または自作に限る。フォントは OFL のもの（Noto Sans JP、Noto Serif JP など）を版を固定して使う。
- 小さいファイルはリポジトリに含め、大きいファイルは hash を固定した取得スクリプトで外部から取得する。取得できない場合、該当テストは失敗として扱う。
- 素材ごとの出典とライセンスを台帳に記録する。

## 実行環境

- CI（GitHub Actions）では、値とレイアウトの意味的比較を必須とする。
- GPU 画素の golden 比較は固定環境（参照機）で実行する（[ADR-0038](../adr/0038-toolchain-and-ci.md)）。

## 正しさ

- 24、25、30、30000/1001、60000/1001 fps、VFR、48kHz 音声、長尺。
- ランダムアクセス、逆順、ネスト、loop、freeze、境界時刻。
- 結合濁点、IVS、絵文字、異体字、禁則、ルビ、縦書き。
- Group opacity、マット、blur halo、alpha edge、HDR → SDR 表示と出力分離。
- 文字変更による範囲セレクターの再割り当て。
- template version update、未知機能の保持、migration 失敗時の元データ保全。
- GUI / CLI / MCP の同じ編集操作が同じ snapshot に到達すること。
- 複数プロセスからの同時書き込みで、古い `base_revision` が必ず拒否されること。

## 障害

- GPU device lost、VRAM 不足、ディスク不足、素材 hash 不一致、フォント不足。
- 途中切断、重複要求、古い revision、取消、worker 再起動。
- 循環式、巨大 Path、過大複製数、長大テキスト、zip 展開上限、外部参照拒否。
- 失敗で Project や確定済み成果物が壊れないこと。出力は一時ファイルを検証してから確定名へ切り替える。

## 性能目標（暫定）

以下は実測前の暫定目標であり、設計判断の目安として使う。合否基準としての確定は、参照シーンを実測した後に PERF-001 で行う（[OQ-14](../open-questions.md)）。

- GPU を使わない単純な値 / レイアウト更新が UI 操作を長時間ブロックしないこと。
- 参照シーンを固定し、warm / cold、proxy / full、preview / final を別々に測定する。
- 基本 4K30 プレビューで 1 フレーム 33.3ms 内を目標とし、デコード待ちとレンダーのみを分けて p50 / p95 を報告する。
- 60fps は同じ品質での追加目標であり、8K、多重ブラー、全エフェクトについて一律保証しない。
- 8K / HDR はまず正しい offline 出力を合格条件にし、リアルタイム要件は別ベンチマークにする。

### 参照機の候補

各環境は別の結果を持つ。第一の基準機は M4 Mac mini 32GB とする。

- M4 Mac mini 32GB / macOS（第一の基準機）
- RTX 4060 Ti 16GB / Windows
- Linux / NVIDIA runner

### 計測項目

`compile_ms`、`eval_ms`、`layout_ms`、`raster_ms`、`gpu_ms`、`decode_wait_ms`、`encode_wait_ms`、CPU / GPU transfer bytes、`peak_memory`、`cache_hit_ratio`、`samples_per_frame`。
