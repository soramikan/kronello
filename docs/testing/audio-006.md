# AUDIO-006 クリップ波形（audio.analyze 連携・キャッシュ・ズーム追従描画）の受け入れ記録

状態: モデル層・共有API接続は `m7-lane-d1` 作業ツリーで確認済み（2026-10-07）。実GUIでの波形表示確認（CUA）は親タスク側で実施する残件とする。

## 実装範囲

- `ClipWaveform`: `project.audio_analyses` の `AudioAnalysisDataAsset` から RMS フレーム列を取り出すデコード済み表現。キーは `"assetID:streamIndex"`。`peaks(from:to:columns:)` がソース秒区間を任意の列数へピーク RMS で再サンプリングし、解析域外は 0（無音）として扱う。
- `EditorModel` に `waveforms`（キャッシュ）・`waveformFailures`（型付き失敗コードで無限再試行を抑止）・`waveformPending`（同一ソースの多重リクエスト抑止）を追加。`adopt()` でストア済み解析をキャッシュへ移し（`refreshWaveformCache`）、Edit ページのオーディオクリップ全件に対して不足分を `audio.analyze` で非同期に発行する（`ensureAudioWaveforms` / `ensureWaveform`）。
- `audio.analyze` は document を更新する（`audio_analyses` への追記・revision 前進）ため、編集トランザクション中（`busy` / 候補保持中）は発行せず、古い base での `REVISION_CONFLICT` は次回リロードで自然に再試行される。解析設定は 48 kHz・window/hop 1024・帯域なし・恒等 time_map の固定値（`EditorModel.waveformConfig`）。
- 描画: `clipWaveform` がクリップの `waveformRange(for:)`（linear time map の offset/speed から求めたソース秒窓、逆再生は窓を反転）をピクセル列数へ再サンプリングし、クリップ内下段へ RMS バーを `Canvas` で描く。`editScale`/`frameWidth` の変更は SwiftUI の再レイアウトでそのままズーム追従する。非線形 time map のクリップは誤った波形を見せないよう描画しない。
- クリップビューの `.task(id: revision)` が `ensureWaveform` を呼び、結果がキャッシュにあればリクエストは発行されない（重複抑止は `waveforms`/`waveformPending`/`waveformFailures` の3段）。

## 受け入れ証拠

`apps/macos` で実行（2026-10-07、この作業ツリー）:

```
swift build --package-path apps/macos --disable-sandbox   # Build complete
swift test --package-path apps/macos --filter WaveformTests
```

| テスト | 内容 |
|---|---|
| `testAnalyzeCacheAndResample` | 実 48 kHz/16bit WAV（内容hash検証通過）の asset を持つ fixture で `audio.analyze` が 1 回だけ発行され、`ClipWaveform`（48 kHz・hop 1024・RMS>0）がキャッシュされる。`ensureWaveform` の再呼出・`ensureAudioWaveforms` の再実行は追加リクエストを発行しない。`peaks` は要求列数どおりを返し、同一窓のピークは解像度不変、部分窓・域外窓も正しく再サンプリングされる。speed 1/2 のクリップはソース窓 0…0.5 秒に写像され、非線形 time map は nil（描画なし）となる |
| `testCacheSurvivesReload` | 解析が document に保存された後の `reload()` でキャッシュが再構築され、`audio.analyze` の再発行がない |

## 残件

- 実 GUI（CUA）で確認: オーディオクリップ内に波形バーが描かれること、ズームで分解能が追従すること、解析中は波形なしで編集が妨げられないこと。
- 解析は asset stream 全体（約95 s/48 kHz まで、共有側の work budget 上限）。長尺ソースは型付きエラーを記録して描画しない。必要になれば範囲分割または band 付き設定の検討が要る。
- `audio.analyze` が mutating で revision を進める点に伴い、他操作との衝突時は REVISION_CONFLICT → 次回リロード再試行という緩い再試行のみ。連続操作時のバックオフは未実装。
