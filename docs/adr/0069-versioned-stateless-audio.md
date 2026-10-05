# ADR-0069: リタイム・effect・Generator・crossfade 音声を明示 profile に固定する

- 状態: 採用
- 日付: 2026-10-05
- 対象: AUDIO-004
- 部分置換: ADR-0051 / ADR-0062 の audio-track Generator・effects・transition 制限、ADR-0063 の新しい profile の対応範囲。movie profile 1/2、既定値、旧 rounding / PCM24 は維持する。

## 背景

文書音声は unity mapping と volume を純粋評価できるが、retime / effects / Generator /
audible crossfade を拒否していた。旧 export の意味を最新の DSP へ暗黙に置換すると、固定 job
と sample の互換性を失う。小さい閉集合を新 profile に限定して提供する。

## 決定

### 版と固定入力

`DocumentAudioPlan::compile` / `compile_version(...,1)` は旧 evaluator。
`compile_version(...,2)` は新 evaluator。movie `profile_version:3` / AvExportSnapshot schema 3 が
この意味を固定し、`with_audio_profile` は2または3だけを受け取る。未知 version は型付き失敗。
new / with_audio の既存 schema 1/2 と省略 explicit / 1 は維持する。schema 3 の clip list は
asset decode requests を表し、Generator に偽 AssetId を作らない。owned RenderSnapshot が
TimeMap / effect Property / Generator / Transition を固定し、envelope の全体 hash に profile と
source selection を含める。再開時に Project を読み直さず、同じ version で再コンパイルする。

### リタイムと pitch

Audio track の直接 Asset / Generator Clip は positive Linear / PiecewiseLinear のみ。
Asset は `audio_retime:resample_v1` の明示 opt-in が必要。Reject は unity-speed だけ。
Clip source coordinate は `source_in + map(n/48000 - timeline_range.start)`、n は絶対整数 sample。
source coordinate ×48000 の floor と fractional part を checked rational で求める。
interpolation は f64 の `a*(1-f)+b*f` → f32、f=0 は a だけを使い不要な tail を要求しない。
container PTS は decoded source zero に二重加算しない。speed に比例して pitch が変わる。
anti-alias filter と pitch-preserving stretch は提供しない。pitch preservation は履歴を持つ
窓処理・latency・overlap と別 budget の設計が必要で、speed 1 に代替しない。

48 kHz 出力 range は既存 absolute floor / half-open 格子。fractional placement の最初の bucket
だけは authored start より前になるため、PWL の最初の segment を明示外挿する。先頭 parent
zero を要求し、負 source position / map domain 不足 / interpolation tail 不足は失敗する。
trim / stretch は共有 Clip 操作で rational map を変形し、sample coordinate は直接評価する。
fractional breakpoint を先頭へ trim した bucket は新しい先頭 slope で外挿するため、元の
breakpoint 前 slope を保存するとは約束しない。旧 unity / Reject の affine phase は変更しない。

### 閉集合の effect と Generator

`kronello.audio.gain` version 1 / `EffectParameters::AudioGain {gain:PropertyId}` を追加する。
Audio track Clip の properties にある `kronello.audio.volume` Scalar Constant / Curve を参照する。
Property descriptor の有限・非負・f32 range / Modifier・Expression 拒否を再利用し、Curve の
評価後にも Gain range を確認する。effect time は Sequence time、clip volume は source-local time。
volume → authored effects の順（各段 f32 乗算）→ transition → authored track / clip 順の加算。
未使用 audio Property、未知 effect / version、映像用 effect は型付き未対応。

SourceRef 構造を変えず `kronello.audio.silence` / `kronello.audio.tone440` version 1 を Audio track
に許す。silence は明示 generator、tone は440 Hz、両 channel 同値、amplitude 0.25。
phase は rational `source_time*440` の fractional cycles から f64 TAU / sin を使い、f32 へ丸める。
clock / random / history を使わない。同一 platform / engine の arbitrary batches は bit 単位で一致する。
platform の sin 実装を跨ぐ bit-identical 性は保証しない。source_in / map が位相と pitch を決める。
共通 color field は既定 opaque black のみ許可し、音声値へ転用しない。
Generator source time も非負を要求し、未知 id / version / color を無音へ代替しない。

### crossfade

共有 `TransitionKind::Crossfade` version 1 を Audio track と audible Video CompositionClip の
音声へ適用する。range 全体の floor 境界 a,b に対し n∈[a,b) で u=(n-a)/(b-a)。
outgoing weight=1-u、incoming weight=u を f64 で計算して f32 へ丸める。linear amplitude で
あり equal-power ではない。a では outgoing 1 / incoming 0、b の sample は transition 外。
実際の intersection 全体を指定し、同一 track の二 clip だけ・第三 clip 不在・順序条件は維持する。
zero-sample transition と未知 version を拒否する。nested Media の active range は引き続き交差する。

### 純粋評価と budget

plan は clip / effect Property / 使用 Curve を所有する。immutable hash-verified AudioSources、
range、compiled plan だけから Bus を作り、バッチ順・作品の後編集・ファイル削除に依存しない。
source の全 authored 範囲と interpolation tail を batch / mute と無関係に先行検証する。
不足を無音に置換せず、有限 headroom を保持し、非有限は AUDIO_OVERFLOW。PCM24 clipping は
既存 Reject / 明示 Saturate のまま。

上限は1024 tracks / authored clips / transitions / flattened placements、16 effects / clip、
1024 map points、effect Curve 4096 keys / curve・65536 keys / plan、Bus 28,800,000 frames、
100,000,000 sample operations / batch。operations は各 entry の overlap sample 数に
`1 + effects 数 + fades 数 + source cost` を掛けた和。source cost は直接 Asset / Generator が3、
継承 Composition / unity Asset は各 flattened placement の `1 + volume stages 数` の和。
継承 mixer は overlap がなくても request 全体の Bus を生成・有限検査するため、entry ごとに
`2 × request frames` も加算する。64 nested scopes と100000 nodes / scope、source decode の
既存 memory budget も維持する。出力 allocation 前に batch work を確認し、new budget は
AUDIO_BUDGET_EXCEEDED。legacy recursive compiler の budget code は INVALID_AUDIO_INPUT を維持する。

## 対応範囲と影響

Composition 内部の retime / node effects、retimed CompositionClip、audio effect を持つ video clip、
Protected / hold / loop map、pitch-preserving stretch、外部 effect / 任意 Generator は未対応。
Composition target の evaluator 2 は旧 recursive unity evaluator を使う。audio track の Composition
source は unity 継承と clip Gain / crossfade に対応する。純粋層へ codec / GPU / store 型を漏らさない。
Video CompositionClip の transform / opacity Property は従来どおり映像だけに適用し、
継承音声を拒否・mute する理由にしない。audio effect を持つ video clip の拒否とは区別する。

共有モデルは audio track の Generator / properties / effects / transitions を保存可能にする。
未知 effect / Generator の保存保持と最終実行拒否は維持する。AudioGain の映像 resolve は
UNSUPPORTED_FEATURE であり、映像へ無視して混ぜない。旧文書は有効なまま。
project / API schema1 と generated Swift を再生成し、新しい transport / editing state は作らない。

## 検証

[音声 architecture](../architecture/audio-000.md)、[data model](../architecture/01-data-model.md)、
[AUDIO-004 の受け入れ条件・command・残件](../testing/audio-004.md) に実装範囲と証拠を記録する。
