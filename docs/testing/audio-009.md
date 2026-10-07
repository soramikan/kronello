# AUDIO-009 音声 UI（VU メーター・オーディオスクラブ・ミキサーパネル）の受け入れ記録

状態: `m8-lane-e` 作業ツリーで確認済み（2026-10-08）。実装は ADR-0117 の決定に従う。

## 実装範囲

### メーター経路（評価器が計測した値をそのまま運ぶ）

- `kronello-audio`: `AdvancedAudioPlan::mix_metered` / `DocumentAudioPlan::mix_metered` を追加。ミックスと同一ループでトラック別・マスターのピーク/RMS を計測する（`StereoMeter` / `TrackMeter` / `BusMeters`）。レガシー経路はマスターのみ。
- `kronello-service`: `PreparedAudio::render_block_metered` が `render_block` と同一の評価経路で PCM を埋めつつ `BlockMeters`（`master_peak` / `master_rms` / `tracks[{track,peak,rms}]`、TrackId はシーケンスのオーサリング済み ID）を返す。
- `kronello-ffi`: `kronello_audio_render_metered` を追加。PCM に加えて所有権付き JSON 文字列でメーターを返す（`kronello_free` で解放）。ヘッダ宣言・boundary テストに登録済み。
- `apps/macos`: `NativePreparedAudio.renderMetered` が FFI JSON を `PlaybackMeters` にデコード。`AudioProducer` は fill パスの最終ブロックのメーターを epoch タグ付きで publish し、`RealtimePlayback.onMeters` が main actor の `EditorModel.playbackMeters`（`@Published`）へ反映。停止時は `.silent` を publish してメーターが決定的に 0 に戻る。
- GUI: `KRMeterBar`（KronelloDesign、-60…0 dBFS のピーク/RMS バー、オーバーで danger 色）をトラックヘッダ（`KRTrackHeader.meter`）とミキサー各ストリップに表示。

### ミキサーパネル

- Edit ページの Sequence パネルにミキサー行（トグルボタン `sliders-horizontal`）を追加。オーディオトラックごとのストリップ（メーター・フェーダー・ミュート）+ Master ストリップ（マスターメーター・モニター音量）。
- **フェーダーのマッピング決定**: モデルにトラックレベルのゲイン欄は存在しないため、トラック上の全クリップの共有 `kronello.audio.volume` Property へ同一の線形ゲインを書き込む `clip_set_volume` コマンド群とし、1 回のフェーダーコミット = 1 件の `edit.apply` イベント（Undo 可能）。`EditorModel.setTrackVolume` / `trackVolume`（全クリップ一致時のみ表示値、混在時は nil）。これは評価器と書き出しが既に理解するパラメータであり、UI 専用の音声状態を導入しない。
- ミュートは既存の `setTrackOutput`（トラック state）を再利用。Master 側は再生モニタリングの `playbackMuted` を再利用（ドキュメントを汚さない）。
- busy/ロック/候補保持中は既存のガードで無効化。ストアへの直接アクセスなし。

### オーディオスクラブ

- `audioScrubEnabled` トグル（Edit ページの `audio-lines` ボタン、デフォルト ON）。再生中でない seek/scrub 時に `RealtimePlayback.scrub` が短区間の再生を行う: 30 ms の結合遅延後に通常の prepare/render/ring/device パイプラインで開始し、160 ms 後に自ら停止する有界ラン。専用の評価経路は作らない。
- 再生中・ミュート中・busy・ロック相当（pendingCandidate・シーケンス未ロード・失敗中）の seek は無音のまま。スクラブはトランスポート（`playing`）もドキュメントも変更しない。

## 受け入れ証拠

この作業ツリーで実行（2026-10-08）:

```
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos --disable-sandbox   # Build complete
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift test --package-path apps/macos --filter AudioMixerTests   # 3 件成功
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer python3 scripts/check_gui_swift.py --run-checks --disable-plugin-sandbox --swiftc /Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc   # 全チェック成功
cargo test -p kronello-audio -p kronello-service -p kronello-ffi --locked   # 全て成功
```

| 確認項目 | 内容 |
|---|---|
| メーター経路の同一性 | `tests/playback.rs::metered_render_reports_per_track_and_master_levels`: `render_block_metered` の PCM が `render_block` と完全一致し、tone440 の 0.25 振幅がトラック/マスターのピーク・RMS として読める。不正バッファは型付きエラー |
| FFI + GUI | `AudioMixerChecks::verifyMeteredRender`: `renderMetered` の PCM が plain render と一致し、TrackId でキーされたトラックメーターとマスター値が届く。クリップ範囲外は無音 |
| スクラブ | `verifyScrubPublishesMetersAndStops`: 有効時の seek が共有パイプラインの有界ランを開始し、メーターがモデルに届き、自ら停止する。`playing`/フレーム/リビジョンを変えず、無効時・ミュート時は無音。停止後はメーターが 0 に戻る |
| フェーダー | `verifyTrackVolumeSharedEdit`: 1 回のコミットが 1 件の `edit.apply`（`clip_set_volume`、キー `kronello.audio.volume`）として記録され、リロード後の値とセッション Undo による復元を確認 |
| ガード | `setTrackVolume` はロック中トラック・非オーディオトラック・非有限/負ゲインを拒否。MixerStrip は `busy`/`pendingCandidate`/ロックで disabled |

## 残件

- 実 GUI（実ウィンドウでのメーター描画・スクラブ聴感）の受け入れはモデル層・FFI 層の検証で担保し、別途実機デモの記録は行っていない。縦断デモ統合（INTEGRATION-005）でフォローする想定。
- スクラブは固定 30 ms 結合 + 160 ms 有界ラン。プレビュー品質の調整（長さ・追従性）は未実装。
- トラックゲインは「全クリップ同一 volume」による代理実装のため、クリップごとに異なる volume を持つトラックではフェーダーが混合状態（表示 1.0・コミットで全クリップ統一）になる。トラック単位バスゲインはモデル拡張が要る。
