# ADR-0049: 音声 Bus・サンプル格子・PCM24 の書き出しを固定する

- 状態: 部分置換（[ADR-0079](0079-bounded-streaming-movie-export.md)。movie export の全 source / Bus・映像 payload 上限のみ）
- 日付: 2026-10-04
- 対象: AUDIO-000

## 背景

基本音声の形式、クリップ音量の単位、最終量子化、音声コーデックを決める。ADR-0018 / 0035 / 0036 / 0048 の LGPL 動的リンクと映像コーデック、ADR-0043 の意味的な単位、TIME-001 の絶対時間格子は維持する。既存 ADR を置換しない。

## 決定

- `kronello-audio` は純粋なミックス・時刻計算を担当し、FFmpeg、I/O、store、service、GPU に依存しない。`kronello-media` の C shim がデコード・libswresample・符号化・mux を担当する。`libswresample` も明示した directory から動的ロードし、ABI / version / license / configuration を他の 4 library と同様に capabilities へ記録する。配布検証は 5 library の LGPL / GPL・nonfree 無効と PCM24 encoder の存在も要求する。
- Bus は **48,000 Hz、left/right の stereo、有限な f32 振幅**。mono は swresample で rate / sample format を変換した後、左右へ等倍複製する。stereo の FL / FR は順序を保持する。speaker mask を持たず channel count だけを持つ basic WAV 等は 1 channel を mono、2 channel を left/right と明示解釈する。多チャンネル、その他の未知・custom layout、途中の rate / layout / sample format 変更は `UNSUPPORTED_FEATURE`。暗黙の downmix は行わない。resampler の遅延を drain し、末尾も保持する。
- 音量は `Gain` の **無次元で非負の線形振幅倍率**。0 は mute、1 は unity。ADR-0043 の単位を明示する方針に従う。同 ADR 自体は音量の単位を指定していなかったため本 ADR で補足する。dB 表示の入口は明示的に倍率へ変換する。現在は snapshot に固定した定数だけを実装し、Curve / Property による音量アニメーションは後続で純粋評価した値を渡す。
- Bus では clip 順の f32 加算を行い、`[-1,1]` 超も保持する。NaN / infinity、積・和の overflow はエラー。limiter / normalize / 暗黙 clamp を入れない。
- サンプル境界は TIME-001 と同じ **数学的 floor**: `floor(time × 48000)`。負時刻も同じ。i128 の整数中間値を使い、overflow を型付きエラーにする。バッチは `floor(start × 48000)..floor(end × 48000)`。フレームごとに丸めた長さを累積しない。
- `AudioClip` は AssetId と明示 stream index、placement `[start,end)`、source_in、Gain を保持する。source_in は最初のデコード済みサンプルを原点とする非負の秒で、同じ floor 規約を適用する。unity speed の配置・トリムだけを実装する。clip の最初の sample は `floor(placement.start × 48000)`、source index は `floor(source_in × 48000) + absolute_sample - clip_start_sample`。配置外は silence、不足する source を silence に置換しない。
- デコードした source の最初の PTS は metadata に保持する。連続した source samples を要求し、PTS と累積 source sample count が demuxer の time_base の 1 tick 以上ずれれば失敗する。粗い timestamp 格子による 1 tick 未満の差だけを許容する。timestamp の gap を詰めたり、非連続素材を無音で補完したりしない。
- AUDIO-000 の音声付き納品形式は **MOV / ProRes + FFmpeg native `pcm_s24le`（48 kHz stereo PCM24）**。圧縮音声の priming / padding を避け、サンプル数・ゼロ開始 PTS を厳密検証できる形式をまず提供する。PCM は LGPL native encoder だけで処理できる。AAC / ALAC / AV1 配信用音声の追加は今回提供しない。検討した AAC は圧縮・互換性に利点があるが、遅延・終端 padding・特許の配布方針を別途固定する必要がある。ALAC は lossless だが追加の codec 契約を増やすため初期範囲にしない。
- PCM24 境界は nearest / ties away from zero、dither なし。`round(value × 8388608)` を signed 24-bit の範囲に量子化し、+1 は 8388607、-1 は -8388608。FFmpeg の packed S32 入力では上位 24 bit に格納する。既定の呼出例は `ClippingPolicy::Reject`。`abs(value) > 1` は `AUDIO_CLIPPING`。明示 `Saturate` だけが振幅を飽和し、変更した channel sample 数を返す。+1 の端点表現と過大振幅の飽和を区別する。
- 現行 RenderSnapshot schema 1 と文書型を変更せず、`AvExportSnapshot` schema 1 が RenderSnapshot の owned copy と音声配置を一つに固定する。素材はその RenderSnapshot の Project.assets からのみ解決し、全 hash を decode 前後で確認する。RenderSnapshot の content hash と、音声配置も含む export envelope 全体の content hash を区別し、MOV metadata と戻り値へ両方保存する。RenderSnapshot の hash だけで未収録の音声配置を識別できるとは扱わない。投下後の Project 編集を読み直さない。
- 映像は既存 `render_frame` と明示した `RenderBackend` を使う。SDR linear Rec.709 と明示背景で alpha を合成し、BT.709 transfer を適用して opaque RGBA8 に量子化する。sRGB のタグ付替えで BT.709 入力を作らない。HDR / gamut 外の値は失敗し、tone mapping / clipping を暗黙に行わない。
- 書き出し範囲は完全な映像フレーム境界を要求し、任意の負・正の絶対位置を許す。レンダー評価は元の絶対時刻、ファイル PTS は range.start を引いたゼロ原点。音声は同じ絶対 range の floor 境界から作り、ファイル PTS は 0 から sample count で進める。両 stream の開始は 0、duration 差は **1/48000 秒未満**。非整数境界の差は量子化誤差として明示する。
- 映像入力は各 frame の長さを EncodeRequest.time_base の 1 tick として native frame / packet に明示する。encoder が duration を省略した場合も、この入力契約から確定した長さを保持する。ProRes / PCM の独立した stage ファイルから packet を DTS 順に interleave し、rational PTS / duration を保持して一つの MOV へ mux する。probe で stream、codec、sample rate、channel、PTS、duration、snapshot metadata を検証してから no-clobber publication を行う。通常エラーでは stage を削除する。強制終了からの復旧、非同期 job、実時間再生は JOB / RECOVERY / 後続音声タスクの範囲。
- 一つの source と Bus はそれぞれ最大 28,800,000 stereo frames（10 分）。export の全 source 合計も同数、clip は 1,024、映像の保持 payload は 256 MiB が上限。長尺・大解像度の streaming は後続の実装で扱い、現行上限超過はエラーにする。

## 検証と影響

[音声設計](../architecture/audio-000.md) と [AUDIO-000 の検証](../testing/audio-000.md) に API、受け入れ条件との対応、開発用 FFmpeg と同梱 LGPL runtime の検証範囲を記録する。

この envelope は共有 service / CLI / MCP の編集状態を新設しない。既存 transport と service は変更せず、将来の NLE compiler / worker が同じライブラリ API に明示入力を渡す。
