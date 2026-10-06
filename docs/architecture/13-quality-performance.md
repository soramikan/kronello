# 13 品質・性能の検証

縦断テスト作品は [roadmap/vertical-slice.md](../roadmap/vertical-slice.md) を参照。

## 比較の方針

- 値とレイアウトの比較は厳密に行う（意味的比較）。
- GPU 画素の比較は、Apple Silicon + Metal 共通基準と Linux Vulkan / Windows Direct3D12 の環境別基準で行う。許容誤差は ADR-0088 に従い、hardware / software adapter の結果を区別する。
- 異なる GPU / CPU 間の浮動小数点のビット一致は約束しない。

## テスト素材

[ADR-0039](../adr/0039-test-fixtures.md) による。

- 映像と音声は、可能な限りスクリプトで生成する（テストパターン、サイン波、VFR など）。
- 生成できないものは CC0 または自作に限る。フォントは OFL のもの（Noto Sans JP、Noto Serif JP など）を版を固定して使う。
- 小さいファイルはリポジトリに含め、大きいファイルは hash を固定した取得スクリプトで外部から取得する。取得できない場合、該当テストは失敗として扱う。
- 素材ごとの出典とライセンスを台帳に記録する。

## 実行環境

- CI（GitHub Actions）では、値とレイアウトの意味的比較を必須とする。
- GPU 画素の golden 比較は Apple Silicon ネイティブ + Metal の共通基準と Linux Vulkan / Windows Direct3D12 の環境別基準で実行する（[ADR-0088](../adr/0088-platform-golden-baselines.md)）。機種・OS・driver・Rust / wgpu 版は provenance として記録し、比較の可否判定に使わない。

共通基準の provenance、実行・明示採用コマンド、許容誤差 `2^-10` と失敗時の扱いは [golden 比較手順](../testing/golden-comparison.md) に記載する。[QA-003](../testing/qa-003.md) で初回 M1 基準登録と 21 シーン比較に成功した。性能計測の M4 Mac mini 基準は変更しない。

## 正しさ

- 24、25、30、30000/1001、60000/1001 fps、VFR、48kHz 音声、長尺。
- ランダムアクセス、逆順、ネスト、loop、freeze、境界時刻。
- 結合濁点、IVS、絵文字、異体字、禁則、ルビ、縦書き。
- Group opacity、マット、blur halo、alpha edge、HDR → SDR 表示と出力分離。
- 文字変更による範囲セレクターの再割り当て。
- template version update、未知機能の保持、migration 失敗時の元データ保全。
- GUI / CLI / MCP の同じ編集操作が同じ snapshot に到達すること。
- 複数プロセスからの同時書き込みで、古い `base_revision` が必ず拒否されること。

ARC-001 で固定した規約の具体的な検証契約は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md)（TIME-001 / PROP-001）、[ADR-0044](../adr/0044-color-and-alpha-contracts.md)（GPU-001 / COLOR-001）、[ADR-0045](../adr/0045-snapshot-compatibility-boundaries.md)（PROP-001 / STORE-001 / RENDER-001 / JOB-001）を参照。これらは今後の受け入れ検証項目であり、実行済みテストの結果ではない。

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


## M4 の測定方法と未達条件

[ADR-0090](../adr/0090-release-performance-evidence-and-snapshot-policy.md) に従い、release・固定作品・既定 cache・quiet host で cold/warm を各21回測定し、全画素の参照照合を行う。停止画面の cache hit と画素が変わる時刻列を区別する。native preview、tile final、movie encoding は別の経路として記録し、GPU descriptor payload、idle pool、CPU Vec、OS sampled RSS/footprint、保守的 admission estimate を混同しない。

M4 Mac mini 32GB の動く基本4K native preview は p50 43.956ms / p95 45.782msで、暫定33.3msを満たさなかった。GUI presentation を含むFPS保証ではない。complex lower-third のnative1080p/4Kは既存512MiB admissionで明示拒否され、proxy preview と tiled final を使用する。OQ-14は未決のままとし、数値目標を実測に合わせて自動変更しない。[全測定結果](../testing/perf-001.md)を参照。
