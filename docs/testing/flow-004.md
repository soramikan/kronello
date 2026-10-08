# FLOW-004 キャプチャ/インジェスト — 受け入れ記録

状態: `done`（`m10-lane-e` の作業ツリーで受け入れた。main への統合・実機 ScreenCaptureKit/デッキ検証とは区別する）。[ADR-0135](../adr/0135-capture-ingest.md) と [ADR-0025](../adr/0025-detached-render-workers.md) に基づく。正本の条件は[バックログ](../backlog/backlog.json)の「録画・デッキ取り込みの経路を実装する」である。

## 受け入れ対応

- `capture.start` / `capture.stop` / `capture.status` / `capture.deck_probe` を共有 Command API に追加し、GUI・CLI・MCP が同一経路を使う。CLI は `capture start|stop|status|deck_probe` 動詞、MCP は `command_registry()` 由来のツールとして露出する。ジョブ系 API として 58xx 帯の命名規則に従う。
- capture 本体は detached worker が担う。service/UI プロセスは固定入力（`FixedInput::capture`、source・format・事前確保 asset id・destination を含む `CaptureJobInput`）を持つジョブを提出・制御・照合するだけで、ScreenCaptureKit セッションを掴まない。worker は種別不一致を `JOB_INPUT_HASH_MISMATCH` で拒否する。
- 録画は `<stem>.capture/<job>.provisional` の RGBA8 スプールへ書き、`capture.stop`（`stop.request` マーカー）または `max_frames` で終端し、フレームを既定 fps で ProRes MOV へエンコード → probe 検証（寸法・duration）→ SHA-256 content hash → asset 登録 → `publish_path` の原子的 no-clobber 公開の順で確定する。確定前に asset は作品状態に現れない。
- `capture.stop` は graceful end-of-input で、記録済みフレームを確定して成功ジョブとして commit する。`job.cancel` は中断であり公開しない。アイドルなソース（poll タイムアウト）は `Produced::Idle` として扱い、stop/cancel の観測を妨げない。
- `capture.status` はプロジェクト単位のセッション一覧（job record・asset id・source・stop 要求・live spool・登録済みフラグ）と、`<stem>.capture/` 配下の型付き orphan（終了済み・不明 job の provisional）を返す。他プロジェクト・非 capture job の指定は `CAPTURE_NOT_FOUND`。
- orphan 復旧は既存の heartbeat 照合と `job.resume` に従う。中断セッションのスプールは attempt 固有であり、resume した worker は再録ではなくスプールの確定を行う（ライブソースは巻き戻せない）。断片テールはフレーム境界で切り詰め、0 フレームは `CAPTURE_EMPTY`。
- 合成ソース（`source: {"kind":"synthetic"}`）は `(x, y, index)` の純関数で RGBA8 を生成し、宣言 fps で送出をペースする。内容は時計に依存せず、実機なしで spool→encode→hash→asset 登録の全経路を検証できる。
- デッキ取り込み（`{"kind":"deck","device":"decklink"|"rs422"}`）と `capture.deck_probe` は ADR-0134 型の vendor SDK 境界。SDK adapter をリンクしない本ビルドでは `UNSUPPORTED_FEATURE` の型付き拒否であり、黙った代替や空のデバイス一覧を返さない。
- ScreenCaptureKit は `kronello-framebridge` の型付き境界（`CaptureTarget::Screen|Window|Application`、`ScreenCapture::next_rgba` のタイムアウト poll）と、`native/capture.m` の Objective-C adapter（`SCStream`・直列 dispatch queue・有界 `NSCondition` キュー・BGRA→RGBA8 変換）で実現する。必須 SDK 宣言のないビルドではコンパイル時に無効化し、実行時は型付き unavailable を返す。

## 確認したテスト

- `cargo test -p kronello-cli --test capture --locked`（9 tests、実 CLI + 実 detached worker）:
  - `synthetic_capture_publishes_registered_asset` — `capture.start`→`max_frames` 自己終了→destination `<stem>.capture/<asset>.mov` の原子的公開、job record の `total_frames`/`completed_frames` 確定、登録 asset の `content_hash` が公開バイトの SHA-256 と一致、container probe が宣言 format（16×8, 6/24s）と一致、`capture.status` が sessions・no spool・no orphans を報告。
  - `capture_stop_publishes_partial_recording` — 連続録画中の `capture.stop` が記録済みフレームを確定して `succeeded` にする。
  - `capture_stop_rejects_foreign_jobs` — 不明 job の `JOB_NOT_FOUND`、終了済みセッションへの再 stop が冪等 no-op。
  - `capture_status_reports_orphans_for_canceled_and_unknown_sessions` — cancel 済みスプール（job link + `canceled` 状態）と record 不明スプールの型付き orphan、非 provisional ファイルの除外。
  - `interrupted_capture_resume_publishes_spool` — worker 強制終了→heartbeat 期限切れ→`interrupted` 化→`job.resume` がスプールのフレームを確定・公開・asset 登録し orphan が消える。
  - `capture_deck_and_start_are_typed_unsupported` — `capture deck_probe` と deck source の `capture.start` が `UNSUPPORTED_FEATURE`。
  - `capture_start_validates_and_replays_idempotent_key` — 奇数寸法・`max_frames:0`・0 fps の `INVALID_REQUEST`、idempotency key の replay と `IDEMPOTENCY_KEY_REUSED`。
  - `synthetic_source_output_is_deterministic` — 同一入力の 2 セッションが同一 content hash。
  - `capture_requests_use_local_locators` — project の URI 指定を `INVALID_REQUEST` で拒否。
- `cargo test -p kronello-service --test api --locked` — `every_request_payload_and_envelope_matches_schema_and_denies_execution_fields`（4 op の request schema・envelope・実行フィールド否認）、`actual_results_for_every_command_match_envelope_and_registry_schemas`（`capture.start` 提出・`capture.stop`・`capture.status` の成功応答、`capture.deck_probe` の型付き `UNSUPPORTED_FEATURE`、registry 網羅）、`public_schema_matches_and_all_registry_schemas_are_safe`（`schemas/api-v1.schema.json` 再生成済み）。
- `cargo test -p kronello-service --lib --locked` — `synthetic_source_is_deterministic_and_opaque`（合成フレームの決定性・サイズ・alpha・index 変化）。
- `cargo test -p kronello-jobs --locked` — `request_stop`/`stop_requested`/`publish_attempt_frames` を含む job store 回帰。

## 保証範囲外

- 実機の ScreenCaptureKit 録画（実ディスプレイ・ウィンドウ・画面収録権限を要求）は手順検証の対象で、決定的テストの要件ではない。macOS 以外では source 指定が `UNSUPPORTED_FEATURE` を返すことを型で保証する。実機手順: `kronello capture start` に `{"kind":"screen"}` を与え、`capture stop` で確定すること。
- DeckLink/RS-422 の実デバイス取り込みは vendor SDK adapter build を要求する。本ビルドでは境界と型付き拒否のみを検証した。
- 録画中のドロップ/リタイム補正は行わない（ADR-0135 の決定）。コンテナ時刻は送出 index に 1 tick を割り当てる。
- `capture.status` の orphan 報告は検出と型付けまで。orphan の削除は管理者操作として意図的に API に入れていない。

## この作業時点の実行記録

2026-10-09（作業ツリー `kronello-m10-lane-e`）:

- `cargo fmt --all --check`: 成功。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: 成功。
- `cargo test --workspace --locked`: 全 suite 0 失敗（capture 9 tests・api 12 tests・jobs state の session テスト・mcp stdio 17 tests を含む）。
- `python3 scripts/backlog.py check`: ok（143 tasks）、`render` 再生成済み。
- `cargo test -p kronello-cli --test capture --locked`: 9 tests・0 失敗。
- `cargo test -p kronello-service --test api --locked`: 12 tests・0 失敗。
- `cargo test -p kronello-mcp --locked`: 25 tests・0 失敗（`capture.deck_probe` の空ペイラードが型付き `UNSUPPORTED_FEATURE` を返すことを schema 検証付きで確認）。
- 検証には外部 fixture（`python3 scripts/fetch_fixtures.py` で NotoSansCJKjp-Regular.otf、`python3 scripts/fixtures.py generate` で generated media）が必要。
