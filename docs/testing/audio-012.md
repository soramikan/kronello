# AUDIO-012: 楽曲全体の音声解析と具体的な診断

状態: `done`。固有の受け入れ条件は下記の試験で確認済み。全workspaceのGPU環境依存失敗は別記する。契約は [ADR-0142](../adr/0142-full-song-audio-analysis.md)。

## 変更と受け入れ証拠

- `kronello-model/src/audio_analysis.rs`: 解析版2、共通preflight、field別 `AudioAnalysisError`。
  版1のhop/work制限は維持し、版2はhop > windowと1,500,000,000 workを受理する。
  `kronello-model/tests/audio_analysis.rs` の2試験で全field診断、sample数・窓数・workの
  required/allowed、10分・2048窓/1600hop/8帯域の1,474,560,000 workを検証する。
  10分設定は予算計算とpreflightの受理を確認したもので、10分PCMの実測ではない。
- `kronello-audio/tests/analysis.rs`: 版1/2の同設定の特徴量を完全一致比較する。
  hop > windowの間のsampleを無視すること、最終窓のpadding、hold、半開終端を確認する。
  入力長・窓数・workの検証はstereo変換・FFT・特徴量配列の確保前。
- `kronello-service/tests/audio_analysis.rs`:
  `full_song_pcm_is_persisted_and_evaluated_at_video_frame_times` は241.5秒、11,592,000 framesの
  48 kHz stereo PCM16 WAVを小さなblockから生成する。共有JSON `audio.analyze` のAsset経路で
  window 2048 / hop 1600 / 3帯域を解析し、7,245フレームの保存/reopen、750 Hz帯域の
  energyとRMSを確認する。保存済みDataAssetを使う式でRMS・帯域energy・onsetを
  映像frame 7244 / 0 / 3600 / 1 / 7244の順で評価し、241.5秒の終端を拒否する。
  元WAV削除後のreceipt再送、指定sessionの保存イベントへの伝播、別sessionで同一keyを
  使った場合の拒否も確認する。省略sessionは既存Bus試験で互換性を維持する。
- `shared_json_diagnostics_are_actionable_before_bus_preparation` は実在しないBus targetを指定し、
  準備に進む前に不正設定・各予算を診断することを確認する。codeは既存family、
  detailsにはvariantやresource / required / allowed、messageにはfieldと制限を返す。
- `kronello-model/src/project.rs` は既知版1/2を保存・編集可能とする。未知版99の保持と
  編集拒否、版1の保存・式参照・変更済みhash拒否の既存試験は維持する。
- `kronello-service/src/project.rs` に内部のsession指定付き保存入口を追加する。
  既存のimport利用者は自動session生成のまま。解析の省略sessionはcanonical JSONから
  省略し、過去のreceiptとの互換性を維持する。返却型 `ProjectInfo` は変更しない。
- 公開API schemaとSwift APIを再生成する。同時作業の `media.import` の変更も維持する。

## 検証コマンドと結果（2026-10-10）

- `cargo test -p kronello-audio -p kronello-model -p kronello-eval -p kronello-service --locked --test analysis --test audio_analysis --test expressions`: exit 0。audio 4、model 2、eval 12、service audio_analysis 5 / expressions 5、計28件成功・ignored 0。service audio_analysis suiteは12.73秒。
- `KRONELLO_UPDATE_API_SCHEMA=1 cargo test -p kronello-service --locked --test api public_schema_matches_and_all_registry_schemas_are_safe`: exit 0、schema再生成1件成功。
- `python3 scripts/generate_swift_api.py` と `--check`: exit 0。
- `python3 scripts/backlog.py check`: exit 0（146 tasks、着手時）。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos -j 3`: exit 1。`~/.cache/clang/ModuleCache` へのsandbox外書き込みが拒否され、manifestの標準ライブラリをロードできなかった。

- `cargo fmt --all --check`: exit 0。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0（48.07秒）。対象crate限定ではなくworkspace全体。
- `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer CLANG_MODULE_CACHE_PATH=/tmp/kronello-audio012-clang SWIFTPM_MODULECACHE_OVERRIDE=/tmp/kronello-audio012-swift swift build --package-path apps/macos -j 3 --disable-sandbox --cache-path /tmp/kronello-audio012-swiftpm`: exit 0（29.97秒）。書き込み可能なcacheとSwiftPMのmanifest sandbox無効化を使った再実行。実行環境全体の権限昇格はしていない。

- `cargo test -p kronello-audio -p kronello-model -p kronello-eval -p kronello-service --locked --test analysis --test audio_analysis --test expressions --test api`: exit 0。上記28件に公開API網羅12件を加えた40件成功、失敗0・ignored 0。再生成後のschemaと実応答を照合した。

ログは `target/audio-012/` の `tests.log`、`schema.log`、`fmt-final.log`、`clippy.log`、
`affected-final.log`、`swift-build.log`（初回失敗）、`swift-build-cache.log`（再実行成功）。

- `cargo test --workspace --locked`: exit 101。`kronello-cli --test machine` の
  `gpu_headless_default_backend_animated_shape_japanese_text_sequence` が `ADAPTER_UNAVAILABLE`
  （`metal found no adapters`）で失敗した。同suiteは25成功・1失敗。ログは
  `target/audio-012/workspace-tests.log`。この失敗以降のworkspace suiteは未到達であり、
  全workspace成功とは扱わない。GPU専用環境での再検証は今回実施していない。

以上をもってAUDIO-012の固有条件を受け入れ、backlogを `done` に変更した。
他エージェントの変更を戻す・整形する操作、コミットは行っていない。
