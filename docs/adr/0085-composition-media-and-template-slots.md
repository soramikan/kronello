# ADR-0085: Composition の視覚 Media と Template MediaSlot

状態: 採用
日付: 2026-10-06
対象: COMP-002

## 決定

既存の `MediaNode` の素材・stream_index・source_in・TimeMap を視覚描画へ接続する。
音声 volume Property の保存構造は変えない。Image と Video の視覚 stream は
既存 Scene IR の外部画像入力、DAG の transform / effect / matte / Group opacity を使う。
CPU reference と明示 GPU の選択を維持し、入口専用の素材状態は作らない。

新しい snapshot は `SemanticVersions.composition_media = Some(1)` を固定する。
旧 snapshot の欠落 field は `None` として serialize/hash を維持する。
visual Media / MediaSlot を使う場合、版が欠けていれば `UNSUPPORTED_FEATURE` とし、
以前未対応だった入力を最新の実装で暗黙に解釈しない。未知版も拒否する。

Video の source time は
`source_in + time_map.map(owning_composition_local_time - node.active_range.start)`。
ネストは既存 InstancePath と CompositionInstance の TimeMap で先に local time を決める。
source time は locked stream の `[start_time,start_time+duration)` に含まれる必要があり、
外側を末尾 frame へ黙って clamp しない。Image は静止画のため source time に依存しない。

Template MediaSlot は明示 Null slot に解決済み AssetRef を束縛し、既定値と instance override を
InstancePath ごとに分離する。Video slot は最初の視覚 stream を選び、
`stream.start_time + slot の local time - slot.active_range.start` を使う。
Image slot は静止画として扱う。slot の変換・effect・matte は通常の node と同じである。

PNG native source は RGB / RGBA / grayscale / grayscale-alpha の 8/16-bit、
palette と低 bit grayscale の精度を保つ展開を対応範囲とする。
16-bit を RGBA8 へ縮小せず、有効 source gamma（sRGB または linear）を decode し、
D65 Rec.709 → working primaries の変換後に alpha を premultiply する。
ICC、異なる chromaticities、任意 gamma、APNG、他画像 codec は版 1 の型付き未対応。
locked dimensions / native format / color と source が一致することを検証する。
非対応の HDR tag を SDR として解釈しない。HDR は COLOR-001 の別の版契約で追加する。

ファイルは decode 前後に hash を検証する。missing は `ASSET_MISSING`、内容変更は
`ASSET_HASH_MISMATCH`。relative candidate の hash mismatch を absolute candidate へ
置き換えない。外部画像は temporal output cache を経由せず、毎回検証する。
素材の filesystem / native decoder handle は backend に閉じ込める。

Image と Video の視覚 stream 自体は document audio に配置しない。
同じ Video asset の別音声 stream は明示 MediaNode として従来どおり配置できる。
Template.preview の画素要求も共通 media backend を使い、semantic query と file decode を
区別する。region を省いた query は画像資産の実ファイルを読み込まない。

## 影響

共通 render.frame / sequence / movie / worker は新しい snapshot pin を通じて同じ描画を行う。
PNG は native image decoder、Video は既存明示 FFmpeg SDR decode を使う。
GPU upload / nearest sampling と CPU reference の色・alpha 一致を比較する。
`require_gpu_resident` の保証対象外画像は resident backend の型付き拒否を維持し、
CPU upload を GPU 常駐 decode と称さない。
