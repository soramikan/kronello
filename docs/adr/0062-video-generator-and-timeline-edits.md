# ADR-0062: 動画・Generator Clip と明示した Timeline 編集範囲

- 状態: 部分置換（ADR-0069、下記の追加範囲のみ）（GPU の受け入れは host run 待ち）
- AUDIO-004 の追加・部分置換: [ADR-0069](0069-versioned-stateless-audio.md)。audio track の Generator / effect Property / crossfade と明示 resample_v1 を追加し、movie profile 1/2 の実行意味は維持する。
- 日付: 2026-10-04
- 対象: NLE-002

## 背景

NLE-001 の Composition 配置、MEDIA-001 の presentation interval seek、FX-001 の Render DAG を接続する。入口ごとの状態・暗黙の映像代替・時間の fps 丸めを追加しない。本 ADR は ADR-0051 の「同一 track の overlap をすべて拒否」と未実装範囲、および ADR-0048 の「色変換は COLOR-001」のうち SDR 動画入力だけを部分置換する。時間写像、保護 retime、音声、LGPL、asset hash、HDR native plane 保持の契約は維持する。

## 決定

### 動画入力と色

- Video track の `SourceRef::Asset` は明示した `stream_index` の video stream を開く。別 stream への代替はしない。`source_in + time_map(t - placement_start)` は **絶対 presentation PTS の秒**。`StreamMetadata.start_time` は最初の decoded PTS、duration は stream の長さ。legacy の欠落 start_time は 0。audio の source_in は ADR-0049 の decoded sample 原点のままで、video の origin を加えない。
- CFR / VFR / B-frame は既存の stream-start seek / flush / decode-forward を使い、`[pts,next_pts)` を選ぶ。最終 frame は明示 duration が必要。平均 fps / DTS / request 順による frame 選択をしない。source bounds と piecewise map domain は配置の両端で検証する。
- 対応色は BT.709 primaries の SDR、BT.709 または sRGB transfer。YUV は BT.709 matrix と limited / full range、RGB は RGB matrix と full range。対応 packed format は rgb24 / bgr24 / rgba / bgra / gbrp と planar YUV 420 / 422 / 444 の 8-bit / 10-bit little endian。alpha を持つ対応 RGB format は straight alpha、それ以外は opaque。未知の format / alpha・明示した未知 tag・PQ / HLG・広色域入力は `UNSUPPORTED_FEATURE`。
- ADR-0044 はタグなし Color を sRGB とするが、その規則を情報不明の動画すべてへ広げないと明記している。ここでは独立した動画入力規則として、**欠落・unspecified の YUV tag は BT.709 / limited、RGB tag は BT.709 primaries / sRGB transfer / RGB matrix / full** に固定する。`sequence.query.clips[].video_color` は effective tags と `assumptions` を返す。画素から推測しない。未対応の場合は `unsupported_reason` を返す。asset metadata 自体を書き換えない。
- LGPL FFmpeg の既存 C shim / libswscale だけで明示的に **SDR RGBA8** へ変換し、CPU で inverse transfer、必要なら Rec.709→Rec.2020 matrix、premultiply を行う。これは 8-bit SDR 入力境界であり、10-bit の保持・extended-range の精度保証・HDR tone mapping の実装ではない。native plane decode API は従来どおり保持する。画素座標の nearest sample は output center から clip-local へ逆変換する。frame interpolation / optical flow は行わない。
- `SemanticVersions.video_input = nle002-sdr-rgba8-nearest-v1` を snapshot に固定する。CPU decode / color / sampling 後の working-space image を明示 GPU upload して、effect / composite / output transform は選択した GPU backend で実行する。CPU backend は明示選択時のみ使う。`FrameMetadata.input_path` と低層 `TransferStats.cpu_upload_pixel_*` で経路を示す。GPU-resident decode とは報告しない。
- native frame は 16,777,216 pixels / packed 128 MiB を上限とする。asset の全 SHA-256 を decode 前後で確認し、`ASSET_MISSING` / `ASSET_HASH_MISMATCH` を保持する。各 tile の動画 decode は現時点では独立実行であり、seek / hash の高速化を保証しない。

2026-10-05 の supervisor 確認では、BT.709 primaries / matrix の YUV に明示した sRGB transfer も許す。tag どおりに復号し、BT.709 transfer に置き換えない。次の各行は service `nle2::video_color_defaults_tagged_sdr_and_unknown_tags_are_explicit_in_query` の独立した parameter case / assertion で検証する。

| 入力 | primaries | transfer | matrix | range | 結果 |
|---|---|---|---|---|---|
| tagged YUV | bt709 | bt709 | bt709 | tv（limited） | 受理、assumptions は空 |
| tagged YUV | bt709 | bt709 | bt709 | pc（full） | 受理、assumptions は空 |
| tagged YUV | bt709 | iec61966-2-1（sRGB） | bt709 | tv | 受理、assumptions は空 |
| tagged YUV | bt709 | iec61966-2-1 | bt709 | pc | 受理、assumptions は空 |
| tagged RGB | bt709 | bt709 | gbr（RGB） | pc | 受理、assumptions は空 |
| tagged RGB | bt709 | iec61966-2-1 | gbr | pc | 受理、assumptions は空 |
| untagged YUV | 欠落 | 欠落 | 欠落 | 欠落 | bt709 / bt709 / bt709 / tv を明示 assumptions とともに返す |
| untagged RGB | 欠落 | 欠落 | 欠落 | 欠落 | bt709 / iec61966-2-1 / gbr / pc を明示 assumptions とともに返す |

| 拒否する組合せ（他の tags は上記対応値） | テスト case | 結果 |
|---|---|---|
| YUV / RGB、transfer = smpte2084（PQ）または arib-std-b67（HLG） | pq / hlg（各 format） | UNSUPPORTED_FEATURE |
| YUV / RGB、未知の primaries / transfer / matrix / range | primaries / transfer / matrix / range（各 field / format） | UNSUPPORTED_FEATURE |
| YUV / RGB、primaries = bt2020 | wide_primaries（各 format） | UNSUPPORTED_FEATURE |
| YUV + gbr matrix、RGB + bt709 matrix | wrong_matrix（各 format） | UNSUPPORTED_FEATURE |
| RGB + tv range | rgb_limited | UNSUPPORTED_FEATURE |
| 未知 pixel format または format 欠落 | format / missing_format（各 format） | UNSUPPORTED_FEATURE |

FFmpeg の `unknown` / `unspecified` / `N/A` / 空 tag は「値がない」marker として上記 untagged 規則を使う。明示した未認識 tag（例 `future-transfer-v2`）は拒否する。query の wire field は `result.value.clips[].video_color.assumptions`、effective tags は同じ `video_color` 内。拒否理由は `clips[].unsupported_reason` に返し、render の拒否を成功へ変えない。asset query を別に追加したものではない。

### Generator と clip effects

- `SourceRef::Generator` は generator id、version、straight `Color` を保存する。初期 built-in は **`kronello.solid` version 1**。Sequence extent の矩形を生成し、clip transform / effects を適用する。snapshot / 時刻 / placement がすべての入力であり、外部 I/O・時計・可変乱数を持たない。legacy の欠落 version / color は初期版 1 / opaque black に固定するが、未知 id の代替画像として black を使わない。
- `SemanticVersions.generators` に id→version を固定する。未知 id / version は選択した Sequence の snapshot 作成時から `UNSUPPORTED_FEATURE`。JSON に保存して保持することと、実行対応は別。
- `Clip.properties` は配置所有の通常の Property。transform と effect parameter を **Sequence time** で評価する。clip effects は source を隔離合成した後に順序どおり既存 DAG へ入り、既存 affine 制限・sigma halo・backward ROI・cache identity を使う。Composition source の effects を clip が共有定義へ書き込まない。
- 合成順は track 配列の下→上。同一 track の transition 内だけ、timeline start 順で outgoing→incoming を確定する。元の配列順から source 優先を推測しない。clip / effect / transform の UUID は既存 clip ID を lowering の transient node ID として使う。
- audio clip effects / properties、audio retime、Composition の音声再帰、image Clip の描画、字幕・adjustment・GUI 編集画面は本タスクの範囲外。query は image / audio / composition / generator / video を明示する。

### transition / ripple / link

- `Sequence.transitions` は省略可能な集合。version 1 の `crossfade` は **同一 video track** の outgoing / incoming と、その配置 intersection に厳密一致する非空の半開 range を持つ。incoming は outgoing より後に開始・終了する。第三 clip の重複と duplicate pair を拒否する。transition のない overlap は従来どおり `CLIP_OVERLAP`。
- crossfade の進捗は rational `(t-start)/(end-start)`。source の clip effects の後に incoming image 全体へ進捗 opacity を掛け、outgoing の上へ source-over する。半透明 source も source-over の意味であり、二画像の straight RGB の単純 lerp ではない。range.start は outgoing、range.end は incoming の半開境界へ切り替わる。unknown transition version は保存して保持するが実行・通常編集は拒否する。
- `TimelineCommand::ClipMove` は delta だけ配置を移し、source_in / TimeMap を変えない。`linked=true` は reciprocal links の transitive component 全体を動かす。`linked=false` で一部だけ動かす要求は `LINKED_EDIT_REQUIRED`。link は unique / reciprocal / 同一 Sequence 内。`ClipLink` は指定 group を完全相互リンクに置換し、旧 group 外への辺も両端で削除する。1 clip の group は unlink。
- `Ripple` は explicit track IDs / pivot / delta / linked を必要とする。指定 track で pivot 以降に開始する clip を動かす。pivot をまたぐ clip は拒否する。linked=true の closure は指定 track 外・pivot 前の linked clip を含みうる。空選択・欠落 track・移動後 overlap / source bounds は原子的に拒否する。
- transition の両端を同時に動かす場合だけ range も delta 移動する。一方だけの移動は `TRANSITION_EDIT_CONFLICT`。transition の除去＋移動、overlap clip の配置＋transition の作成を一つの plan に入れられる。検証は各 command の途中状態ではなく、全 command 適用後の候補を検証する。
- linked editing の初期範囲は move / ripple。既存 trim / stretch / protected retime の意味は変更しない。transition endpoint の trim / stretch が range を無効にした場合は候補検証で拒否する。

### 共通 API と競合

- 新しいトップレベル query は **`sequence.query`**（CLI `sequence query`、MCP registry 共通）。revision、Sequence、track ID、ClipKind、clip 本体、video color assumptions / unsupported reason を返す。名前から kind を推測させない。
- mutating 入口は既存 `edit.plan` / `edit.apply` 内の `TimelineCommand`。追加 variant は `clip_move`、`clip_link`、`ripple`、`transition_set`、`transition_remove`、`clip_set_effects`。`clip_set_effects` は配置 properties と effect stack を一緒に置換する。通常の `property_source_set` も clip Property を対象にできる。
- Timeline 構造編集の conflict key は `Structure(sequence_id, sequence_id)` と、影響した全 clip の `Structure(clip_id, track_id)`。同じ Sequence の構造編集は保守的に競合する。別 Sequence の構造変更、無関係な Composition Property は独立して Undo できる。SequenceCreate は従来の project container key を保持する。clip Property の value key は `(clip_id, property_id)` で、clip 構造変更と交差する。
- idempotency receipt、revision、plan hash、selective Undo、save/reload は既存 service / store の transaction を共有する。ADR-0026 の active inverse もイベントとして競合する規則を維持する。部分適用しない。public schema と generated Swift transport を Rust generator から同期する。

## 検証

[NLE-002 検証記録](../testing/nle-002.md) に CPU 実行、実 CLI / MCP / 固定 worker、pending host GPU を分けて記録する。sandbox の CPU 成功を GPU 条件の達成とは扱わない。
