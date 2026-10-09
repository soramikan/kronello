# MEDIA-004 配信拡張: チャプター・マルチ出力・追加コーデック profile

## 実装

[ADR-0133](../adr/0133-delivery-chapters-multi-output-codec-profiles.md) の受け入れ対象。

- シーケンスの `chapter` ロールマーカーを MOV/MP4 出力のコンテナチャプターに転写する。
  区間は書き出し範囲にクリップされ、出力時刻に再基準化され、範囲端・非 chapter
  ロール・コメント/色は転写しない。WebM/MXF/GIF/MP3/FLAC のようにチャプターを
  持てないコンテナでは、レッグ report に `CHAPTERS_DROPPED` の型付き警告を記録する。
  明示的な `omit` 指定では警告を出さない。
- `AvExportRequest.outputs`（`DeliveryOutput`）で、1 回のレンダーを複数エンコーダに
  ファンアウトする。全レッグは同一 render snapshot を共有し、音声は 1 回デコードして
  各レッグで独立にエンコードする。全レッグを検証してから公開し、どれかが失敗すれば
  書き出し全体が失敗する。
- バージョン付き配信 profile を追加した: `DnxhdMovPcm24V1` / `Dnxhr{Lb,Sq,Hq,Hqx,444}`
  の MOV・MXF、`GifV1`、`Mp3V1`（libmp3lame CBR 256k、mono/stereo）、`FlacV1`。
  追加コンテナは `mxf` / `gif` / `mp3` / `flac`。
- 実行時エンコーダ能力チェック: `MediaCapabilities::select_encoder(EncodeCodec::Dnx)` と
  `require_encoder("gif"|"libmp3lame"|"flac")`。未登録エンコーダは
  `ENCODER_UNAVAILABLE` の型付きエラーになり、暗黙のフォールバックはしない。
- `render.submit` / `render.export` の固定ジョブ経路: 提出時に全出力 profile を固定
  input に凍結し、worker は staging 内に `leg_N.<container>` で各成果物を作り、
  receipt は全 publish 宛先を記録する。公開は atomic no-clobber rename。recovery は
  宛先ごとの stamp と receipt artifact hash を照合し、部分公開は reconciliation にせず
  `OUTPUT_EXISTS` / `OUTPUT_VALIDATION_FAILED` で拒否する。
- 既存の単一出力 MOV ジョブの互換性は維持する（staged artifact は profile の
  container 拡張子付き `output.<container>`、receipt の `destinations` は空で
  旧挙動にフォールバック）。

### 境界上の決定

- MOV/MP4 はチャプターを data トラックとして保持するため、mux 検証は
  「video 1 + audio 1 + （chapter 対応 profile のみ・chapters 非空の場合のみ）
  data トラック」を許す。
- elementary 出力（GIF/MP3/FLAC）と MXF は独自メタデータタグを往復しない。
  snapshot 同一性の埋め込み検証は `MovieProfile::embeds_snapshot_identity()` が
  真のコンテナだけに適用し、MXF/elementary レッグは probe shape と receipt の
  バイト hash で認証する（service 側の再検証も同じ述語を使う）。
- MP3 の Xing ヘッダはエンコーダ遅延を正の stream start として報告する。
  AudioOnly 検証は `audio_frame_slack` 以内の signalled priming を許し、
  lossless profile では厳密なゼロ起点を維持する。Xing を trim しない
  demuxer はエンコーダ遅延・末尾 padding・Xing フレームを含む生フレーム数の
  duration を報告するため、MP3 の duration slack は 3 codec フレームとする。
- GIF は muxer が stream time base を 1/100（センチ秒遅延）に固定するため、
  パケット時刻を codec time base から rescale してから書き込む。
- MXF muxer は video stream の frame rate を要求するため、remux 時に probed
  `avg_frame_rate`（欠落時は time base の逆数）を引き継ぐ。
- DNxHD（非 HR）は FFmpeg の CID テーブルにラスタ/フレームレートを拘束される。
  表外の条件は `ENCODE_ERROR` の型付き失敗で、フォールバックしない。

## 再現手順

```text
python3 scripts/fixtures.py generate --output target/fixtures/generated
cargo test -p kronello-media --test delivery --locked
cargo test -p kronello-cli --test jobs --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

### 追加した検証

`crates/kronello-media/tests/delivery.rs`（実 FFmpeg runtime を使用）:

- `chapter_markers_transfer_into_mov_and_probe_roundtrips`: chapter ロールだけが
  MOV に転写され、範囲外・非 chapter・コメントが除外され、probe が往復する。
- `chapters_drop_with_typed_warning_on_incapable_containers`: GIF は
  `CHAPTERS_DROPPED` 警告で転写せず、`omit` では警告なし。
- `one_render_fans_out_to_mov_gif_mp3_and_flac`: 1 render → MOV+GIF+MP3+FLAC の
  4 レッグ、全 probe verify と snapshot 同一性。
- `legs_validate_before_render_and_never_publish_partially`: 既存宛先・宛先重複・
  誤ったコンテナ拡張子は render 前に失敗し、途中失敗で部分公開しない。
- `dnx_mov_and_mxf_profiles_encode_and_probe`: DNxHR MOV/MXF の codec probe、
  MOV ではチャプター転写・MXF では警告、DNxHD 表外ラスタの型付き失敗。
- `mp3_and_flac_elementary_deliveries_probe_and_verify`: elementary 音声の codec /
  48 kHz / レイアウト検証、MP3 の 5.1 拒否、FLAC の closed 多ch。
- `missing_closed_profile_encoders_are_typed_capability_failures`: 未登録
  `dnxhd` / `gif` / `libmp3lame` / `flac` は `ENCODER_UNAVAILABLE`。

`crates/kronello-cli/tests/jobs.rs`（実 worker プロセスの固定ジョブ）:

- `multi_output_fixed_job_publishes_and_authenticates_every_leg`: MOV+GIF+MP3 の
  3 レッグが 1 ジョブで公開され、fixed input の `outputs` 凍結、チャプター転写 /
  警告、全レッグの probe 検証、成功済みジョブの `job.resume` が全宛先を receipt
  で照合して reconciliation、改竄レッグと欠落レッグが型付き拒否になる。
- `multi_output_submission_rejects_invalid_leg_destinations`: 占有宛先
  `OUTPUT_EXISTS`、宛先重複、拡張子不一致 `INVALID_MEDIA_INPUT`、非 movie
  プライマリ `UNSUPPORTED_FEATURE` は worker spawn 前に失敗する。

## 受け入れ証拠

開発機（macOS arm64、FFmpeg 9.x LGPL shared runtime）で実施。個別記録:

- `cargo test -p kronello-media --test delivery --locked`: 7 passed; 0 failed。
- `cargo test -p kronello-cli --test jobs --locked`: 36 passed; 0 failed; 2 ignored
  （multi-output 2 件を含む）。
- `cargo fmt --all --check`: 差分なし。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: 警告なし。
- `cargo test --workspace --locked`: 全 184 test binary 成功、0 failed。
- `python3 scripts/backlog.py check`: ok。
- `KRONELLO_SCHEMA_UPDATE=1` で `schemas/project-v1.schema.json` と
  `schemas/api-v1.schema.json` を再生成し、`scripts/generate_swift_api.py --check`
  で GeneratedAPI.swift の一致を確認。
- 互換性確認: movie レッグの共有領域は正の偶数ピクセルを要求する規約を
  preflight 検証に復元し（`export.rs` の leg 検証ループ）、既存の
  `streaming.rs` 奇数幅拒否テストが通ることを確認した。

## 残件 / 明示的な未対応

- DNxHD の CID ラスタ/フレームレート表外、MP3 の mono/stereo 以外、
  チャプター非対応コンテナへの転写はいずれも型付きエラーまたは警告であり、
  フォールバックは設けない。
- GIF のセンチ秒遅延量子化により、コンテナ duration は 1 フレームあたり
  1cs 未満の丸め誤差を許す検証（範囲との差分は `frames+1` cs 以内）。
- elementary / MXF レッグはコンテナ内 snapshot 同一性タグを持たない。
  受け入れモデル上の認証は receipt hash と probe shape で行う。
