# ADR-0060: 非表示原因と実行前レンダー計画を読み取り Query で共有する

- 状態: 採用
- 日付: 2026-10-04
- 対象: INSPECT-001

## 背景

GUI の hidden layer と render path 表示には、診断文の解析ではなく安定した原因 code、責任を持つ node / InstancePath、依存辺、転送・面・cache の区別が必要である。
ADR-0005 / 0006 / 0008 / 0043 / 0053 / 0057 / 0058 の純粋評価、意味的 snapshot、明示 backend、転送、layout と式の依存契約は維持する。本 ADR はこれらを置換しない。

## 決定

- 共通 service に読み取り `node.explain` と `render.explain` を追加する。CLI、MCP、FFI は同じ Request / Response / registry を使う。一つの保存 revision を取得し、処理途中で Project を再読込しない。GPU / codec / システムフォントを初期化せず、明示されたローカル font locator だけを読む。
- `node.explain` は root Composition、`{instance_path,node}`、有理数 time を受け、原因を `VisibilityCode` enum と `category` / `impact` / `subject` / `details` で返す。message は診断用。元の型付き失敗は `error: ServiceError` として code / details を保持する。enum は Rust の `non_exhaustive` とし、将来の原因追加時に field の設計を変えない。GUI-001 の `enabled` は別 branch の文書契約であり、この変更では追加しない。merge 時に disabled / hidden ancestor の原因を統合する。
- containment の active range / opacity と transform parent の変換依存を分ける。transform parent の非アクティブ・opacity 0 だけで子を hidden にしない。collapsed world transform は明示原因とする。非アクティブな containment 祖先から別 instance scope へ進む場合は local TimeMap を実行せず、local_time を null とする。同じ scope の独立した原因は併記する。
- asset / font / unsupported / expression の失敗で既定値へ戻さない。欠落した Shape / Text content、font locator / hash、opaque effect 等は node の構造化原因。Composition 全体の compiler failure は `render_diagnostics` に分け、無関係な text の font 欠落を問い合わせた Shape 自身の欠落として扱わない。
- assessment は `hidden` / `blocked` / `potentially_visible` / `indeterminate`。blocked を優先し、原因配列は消さない。potentially_visible は意味的な hiding cause を検出しなかったという意味であり、画素被覆・他レイヤーによる occlusion の測定ではない。`pixel_visibility_observed` は false。式の静的依存 closure と renderer の layout 宣言辺、containment / transform / matte と content resource の参照を明示する。
- 文書の matte field は現時点で存在しない。任意の `mattes` は既存の transient `MatteBinding` 入力であり、saved Project へ保存しない。matte-only、opacity 0、透明 leaf の zero coverage、黒い leaf の zero luminance、非アクティブ参照を分ける。未測定の alpha / luminance coverage は `MASK_COVERAGE_UNRESOLVED`。非アクティブ matte は最終 renderer が拒否するため、透明成功として扱わない。Query の診断に matte を渡しても通常の render.frame へ暗黙に伝播しない。
- `render.explain` は既存 `RenderInput` と time を受け、Composition / Sequence を最終 renderer と同じ snapshot / lowering / Scene IR / DAG compiler へ渡す。backend は service の明示選択を読むが device を生成しない。注入 backend は `unknown` とし、未知経路の転送・面使用量を 0 と捏造しない。compiler failure は `plan:null` と型付き `diagnostics`。Query 応答の成功はレンダー成功を意味しない。
- frame execution と inspection の tile 分割を `frame_tiles` で共有する。tile ごとに DAG stages / inputs / SceneKey、要求領域と halo を含む実行領域、RGBA16F 面サイズ、backend と同じ安全予算式による面数を返す。メモリは必ず `_estimate` として示し、allocator overhead、driver allocation、font bytes、geometry、process RSS、実測 peak、待ち時間を含まない。CPU の面 payload は float32 16 bytes/pixel、GPU の面 payload は RGBA16F 8 bytes/pixel。共通の最終 linear / display host 面は合計 32 bytes/pixel。512 MiB は既存の安全予算であり OQ-14 の性能目標を確定するものではない。
- 転送は control upload、image upload、GPU 内 image copy、image / validation status readback を分ける。組込 GPU の frame export は同じ scene を linear / display 用に二回描画し、tile ごとに padded RGBA16F image と 4-byte status を各二回 readback する。行 padding は 256 bytes。geometry / control upload の量は今回未推定で null。CPU 経路の GPU 転送は 0。device 可用性・実転送・性能は未測定で、`executed:false`。
- 各 service Query は新しい `RenderCache` を使って破棄する。renderer の cache / LRU 順序 / counters / entries を参照・変更しない。`compilation_cache` はこの隔離 compile 中の CACHE-001 実 counters（values / layout / geometry / raster の hits / misses / inserts / evictions / entries / payload bytes）。`cache_scope="isolated_query_compilation"`、`raster_cache_observed=false`。raster の 0 counters は未実行であり runtime cache miss の証拠ではない。warm runtime の観測や画素 cache 探索は後続の計測 API に残す。

## 影響と検証

tile の `surface_bytes_estimate` は基準となる RGBA16F 面の payload（8 bytes/pixel）。CPU の `intermediate_bytes_estimate` は float32 面（16 bytes/pixel）から別途計算する。これらは backend 全体の実 allocation ではない。

既存の最終出力は同じ型付き失敗で停止する。診断は opacity 0 でも処理される node、二回の GPU 描画、halo 拡張、推定面予算超過を安定 code で示す。性能目標・実測値・GPU resident preview / FrameBridge の保証へ読み替えない。

公開 API schema は Rust から生成し、Swift の generated DTO も同じ schema から再生成する。受け入れ条件と実行結果は [INSPECT-001 検証](../testing/inspect-001.md) に記録する。
