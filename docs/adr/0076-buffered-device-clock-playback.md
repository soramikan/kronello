# ADR-0076: 固定文書音声を SPSC buffer で再生し、出力デバイスのクロックへ映像を合わせる

- 状態: 採用
- 日付: 2026-10-05
- 対象: AUDIO-002
- 決定者: supervisor。実デバイス / Metal の受け入れは pending host run。

## 決定

作品更新と realtime callback を分離する。`AVAudioEngine` の `AVAudioSourceNode` は
48 kHz stereo non-interleaved f32。render block は事前確保した native C11 SPSC buffer の
copy / silence と lock-free atomic の更新だけを行う。Swift closure は既存 buffer を渡すだけで、
collection / closure の生成、blocking lock、Rust FFI、JSON、disk、式評価を呼ばない。
`atomic_is_lock_free` が成立しなければ `AUDIO_RING_UNAVAILABLE`。

実行系は次のように分ける。

| 実行系 | 責務 |
|---|---|
| MainActor | Transport / UI、出力クロックに基づく約120 Hzの表示要求、診断と検証ログ |
| `kronello.audio.prepare` queue | revision 照合、owned plan の compile、hash 照合済み source decode |
| `kronello.audio.producer` queue | evaluator 2 の最大4096-frame block、binary f32、block 単位の公開 |
| OS audio render thread | SPSC consume / silence、device sample / host timestamp と underrun counters |
| 既存 Rust session worker | 編集 / Query / Metal preview。audio producer はこの FIFO を待たない |

`PreparedAudio::prepare(project, target, expected_revision)` / `render_block(start_sample, output)` は
`kronello-service` の **preview runtime resource API**。Metal preview / FrameBridge と同じ境界であり、
process-local resource を共有 Command / Query registry の entry にしない。
`kronello_audio_prepare` / `kronello_audio_render` / `kronello_audio_free` は producer 専用 C ABI。
prepare は strict JSON の `{project,target,expected_revision}`、render は caller-owned interleaved f32。
error は `ServiceError` JSON を `kronello_free` で一度だけ返却する。
Project / public API schema、registry と GeneratedAPI.swift は変更しない。
既存 export / CLI と同じ `DocumentAudioPlan::compile_version(...,2)` / `mix` を使い、
PCM24 quantisation 前の sample bits を parity test で比較する。Generator に偽 asset を作らない。

## Snapshot と budget

prepare 時に store の read-only snapshot を一度読む。plan は Property / Curve / TimeMap を所有し、
decoded source は immutable。後の作品編集 / 削除 / sample request 順序で block の意味を変えない。
revision 通知では新しい plan を別 queue で準備する。最新の revision 要求へ集約し、
producer queue が現在の block を公開した後で resource を差し替える。
一つの block は write index の release store 一回で公開し、consumer は acquire で取得する。
asset source の差し替え中に部分的な block を見せない。音声を最後に追加 / 削除した場合は
master / graph を現在の整数 sample で停止・再準備し、device / host clock を切り替える。

`KR_AUDIO_CAPACITY = 32768` frames（682.667 ms）、block は4096 frames（85.333 ms）。
producer は5 ms timer、一回最大8 blocks。buffer は stereo f32 と absolute sample / revision の
24-byte entry、合計786432 bytes + atomic / clock fields。producer scratch は32768 bytes。
作品編集から audible な変更までの buffer 待ちは最大約682.667 ms + output latency。
これに snapshot preparation の時間を加える。低 latency の保証値とはしない。

decoded source の合計は既存 `MAX_AUDIO_FRAMES = 28800000`（600秒の stereo、payload 230400000 bytes）。
`decode_asset_audio_bounded` は残 budget を source ごとに渡し、append 前に
`AUDIO_BUDGET_EXCEEDED`。切り詰め / silence への代替はしない。Vec capacity / decoder chunk / native
codec scratch は payload と別に存在する。通常の snapshot 更新では current + candidate の二つまで
を保持し、producer が差し替えるまで次の candidate を準備しない。
plan の placements / source work / Curve budgets は evaluator 2 の既存制限を継承する。
長尺 source streaming、pitch preservation は追加しない。

## クロックと移動

callback の output sampleTime / hostTime の組を使用し、最初の device sampleTime を原点とする。
timestamp は有効な整数 sampleTime（Double が整数を厳密に保持する2^53未満）と単調なhostTimeを
要求し、device sample の逆行を型付き拒否する。長時間clock math testとdevice timestamp上限は別の確認。
MainActor はその組から `mach_absolute_time` の差を整数 timebase で48 kHz格子へ変換する。
host clock だけを独立に積算して audio clock と称することはしない。
clock の atomic read は最大3回の retry。publication と競合して取得できない回は直前の整数
position を保持し、seek origin へ戻さない。
source node は48 kHz、出力 format の変換は AVAudioEngine が行う。
output node の `outputPresentationLatency` を ceil した48 kHz sample 数だけ補償する。
表示 frame は `floor((audio_sample - latency_samples) * fps_num / (48000 * fps_den))`。
実装では samplePosition が latency を引き、frame math は補償済み sample を一度だけ使う。
中間は checked `__int128`、rate は各成分1..1000000000。時間 / frame 長を Float で積算しない。
device hostTime から過去の Metal submission time への signed 換算も同じ格子を使う。

seek は `floor(frame * fps_den * 48000 / fps_num)`。
engine stop と producer barrier の後だけ ring を flush / reset し、正確な target sample から prefill。
app は再開前に現在の Metal presentation task を待ち、seek 前の queued frame が新audio epochへ
遅れて表示されることを避ける。待機中はaudioとpresentation timerを停止する。
NTSC frame の floor bucket は rational frame start より1 sample未満だけ前にあるため、
その一 sample を frame floor へ再変換すると直前の frame になることがある。境界を ceil に変えない。
stop は補償済みの現在整数 sample を保持し、resume は displayed frame へ丸め直さない。

underrun は silence と回数 / missing frame 数を記録する。device clock は進み、遅着 block は
absolute sample tag により破棄する。遅れた音声を後から流して映像へ遅延を持ち越さない。
`AUDIO_UNDERRUN` は status の件数 / details に示す。producer の typed error、timestamp / format
異常 (`AUDIO_TIMESTAMP_INVALID`)、device change (`AUDIO_DEVICE_CHANGED`) は停止して表示する。
callback が進まなくなった場合は `AUDIO_CLOCK_STALLED` とし、古いtimestampから時間を進め続けない。
音声なし / 明示 mute / output format がないデバイスは host-clock master と理由を status に表示する。
音声付き作品の decode / evaluator error を host-clock fallback の成功へ変換しない。

## GUI と統合点

`EditorModel` が presentation timer を所有する。tick は ui.time の変更だけで、
scene / project / history の逐次 Query を frame ごとに呼ばない。
MetalPreview は最新の表示要求へ集約し、pixel size が変わったときだけ surface resize。
Motion の古い per-frame reload loop を削除する。新しい mute button は既存 KRButton の focus / color を継承する。

GUI-003 は `configurePlayback(target: .sequence(id), rateNum: ..., rateDen: ...)` を entry / target 変更で
呼ぶ。Motion へ戻るときは `target:nil`。`playing` binding と `tick()` は維持するが、ページ側の別
timer は不要。Sequence の frame / duration / extent は configured target から表示する。
`EditorWindow.swift` は既存 status string / diagnostic の最小変更だけ。

## 検証の境界

[AUDIO-002](../testing/audio-002.md) に test と host procedure を対応付ける。
native synthetic consumer / CPU parity / direct Swift compiler は realtime A/V の受け入れ証拠にしない。
`KronelloAudioHarness` は実 audio engine と実 Metal surface を40秒ずつ3 ratesで動かし、
seek / stop / resume、native Metal submission host time と device sample の対応を記録する。
max A/V frame offset / ms と underrun count を解析する。`queue.present` の直後の時刻は
**submission の proxy** であり、physical scanout / speaker の loopback measurement とは区別する。
本 sandbox は engine / Metal を実行していない。ホストの結果と supervisor の合否判断が必要。
