# ADR-0074: movie export の payload を有界 streaming で処理する

- 状態: 採用
- 日付: 2026-10-06
- 対象: RENDER-003
- 部分置換: ADR-0049 の export 全 source / Bus 10 分・映像 256 MiB 制限、ADR-0050 の同制限の継承、ADR-0053 の movie export 最終面保持。その他の時間・色・codec・publication・通常 frame / image sequence 契約は維持する。

## 決定

- 同期 `render.export` と固定 worker の movie export は同じ `export_av_with_checkpoint` を使う。owned snapshot、asset hash の decode 前後照合、明示 codec / backend、SDR / alpha / PCM24 量子化の意味を変えない。
- 選択 audio source を一つずつ decode / resample / drain し、destination volume の一時 directory に stereo f32 little-endian の不変 spool を作る。source 先頭は最初の decoded sample のまま、PTS continuity・finite samples・layout / rate 変更の検証を共有 `decode_audio_stream` で行う。source / aggregate の 28,800,000 frames 制限を movie export に適用しない。単体 `decode_audio` / `AudioBuffer` の既存制限は維持する。
- `AudioSourceReader` は frame count と indexed sample の read-only 契約だけを純粋 mixer に渡す。media の実装は source ごとに 4,096 frames の窓だけを保持し、逆再生・piecewise map・補間 tail の要求を同じ絶対 source index で読む。read failure を `AUDIO_SOURCE_READ` にし、無音に置換しない。全 authored source range の検査は batch / mute と独立に維持する。
- Bus は codec block ごとに生成・量子化・encode して破棄する。PCM24 は 4,096 frames、ALAC は native block（現在4,096 frames、上限65,536）と最後の partial block。native encoder は一つの frame / packet を所有し、partial は最後だけ、PTS は累積 sample count、zero priming と exact final duration を維持する。Bus / operations の既存安全予算は batch ごとに適用する。
- batch 境界は要求の絶対 `floor(start × 48000)..floor(end × 48000)` を整数 sample index で分け、その index から rational time を生成する。映像は time-zero frame grid、評価時刻は元の絶対時刻、encoder PTS は range.start を引く。境界の sample / frame を重複・欠落させない。
- movie frame は最大512×512の tile sink から一つの opaque RGBA8 buffer に合成して即座に encode する。全画面の float32 linear / display 面も、全映像 frames の payload も保持しない。元の画素格子、ROI / halo、metadata は従来と同じ。一般 `render_frame` と image sequence の二つの全画面 float32 出力は維持する。
- node 別 tile allocation は実行 ROI の union を使う保守的な計算とする。image stage は1面、Group は child 数 + accumulator + output、effect は output + 3 temporary、root / reserve は4面。`RenderDag::tile_surface_bytes(16)` が最大の CPU float32 面を数え、総額512 MiBを超える cumulative halo を backend allocation 前に `UNSUPPORTED_FEATURE` で拒否する。backend 自体の node / depth / edge / surface 制限も維持する。各 node の異なる ROI だけを allocation する最適化は導入しない。
- native decoder の既存 input 上限1,048,576 samples、resampler / Rust chunk 上限2,097,152 framesを維持する。source 窓は最大1,024 sources × 4,096 × 8 bytes、movie RGBA8 buffer は既存の最大16,777,216 output pixels × 4 bytes。allocator / geometry / font / driver / codec の内的資源はこれら payload 計算と区別する。
- frame grid と report の frame metadata は既存1,000,000 frame 上限の下で保持する。これらの metadata は frame 数に比例し、payload の有界処理と区別する。8K の output pixel 上限解除、metadata の外部 manifest 化、image sequence の最終面削減、性能合否（OQ-14）は今回決めない。
- source decode chunk、audio block、video frame、最終 mux 前に checkpoint を確認する。stage は destination volume に置き、decode / clipping / encode / I/O / cancel エラーでは RAII で回収する。probe 後の no-clobber publication と worker の lease / cancel fence を維持する。強制終了後の回収 / resume は RECOVERY-001。
- `AvExportReport.streaming` は任意の診断 field。成功した spool write / window read の論理 bytes、一時 audio / video file の実 byte 長、公開 file の実 byte 長（取得不可なら null）を返す。物理 disk I/O / RSS の推定値として扱わない。snapshot / API request の意味版を変更しない。

## 検証

[RENDER-003 検証記録](../testing/render-003.md) に601秒の実 asset decode / Bus / mux、4K旧payload制限超過、tile / halo / 負開始 / NTSC、全音声 samples、RSS / I/O、失敗時の回収を記録する。CPU export の測定は Metal / hardware encoder の性能保証を意味しない。
