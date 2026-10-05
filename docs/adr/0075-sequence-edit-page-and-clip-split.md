# ADR-0075: Sequence 編集ページと共有 Clip 分割

- 状態: 採用（画面・Metal・SwiftPM の受け入れはホスト検証待ち）
- 日付: 2026-10-05
- 対象: GUI-003

## 決定

監督が以下の API と操作範囲を承認した。配置・トリム・同一トラック内の移動・分割は
GUI-001 の `EditorModel.apply`、共有 `edit.plan` / `edit.apply`、通知と session Undo を使う。
ドラッグ中は immutable query に候補 geometry を重ね、release で一つの Command / Event。
読み込み途中は中立の進捗と古い表示である旨を示し、query の失敗を空の成功へ置き換えない。
外部変更は revision の一致した文書と `sequence.query` を採用する。操作開始時の revision は
候補に保持し、競合は明示した破棄 / 再適用、Undo の競合は既存 Dialog で扱う。

- `TimelineCommand::ClipSplit {sequence, clip, time, right_clip}` を既存の編集 payload に追加する。
  `time` は Sequence の絶対有理数時刻。厳密な内点だけを許し、境界・範囲外・重複 right ID は
  `INVALID_EDIT`。左は元 ID、右は呼び出し側が一度だけ確保した UUID を持つ。
- 両側を `Clip::trimmed` で作り、linear / piecewise retime の source mapping を維持する。
  右側の所有 Property・volume・Modifier は新しい stable ID、effect の Property 参照も付け替える。
  Curve / Expression の参照と descriptor は共有のまま。Effect 自体に所有 UUID はない。
  plan と apply の独立実行で同じ候補になるよう、右 clip UUID と元の所有 UUID を
  `kronello.clip-split-owned-v1` の SHA-256 に入れ、RFC variant の UUID v8 を生成する。
  ID を配列番号・表示名・時計から導出しない。
- linked clip は `LINKED_EDIT_REQUIRED`、transition の endpoint は
  `TRANSITION_EDIT_CONFLICT`。暗黙の連鎖分割や transition 修正は行わない。
  一つの Event の逆操作で元の Clip と所有物を復元し、既存 selective Undo を使う。
  トップレベル registry の操作数は38のまま。CLI / MCP / FFI は同じ variant を受け取る。
- `sequence.query.asset_status` は Project 全体の素材を一括診断する read-only の追加フィールド。
  render と同じ `kronello-media::locate_asset` の相対優先・絶対 fallback と regular-file stat を使う。
  `present_unverified` は存在・size だけの確認であり hash 検証の成功ではない。
  `missing` / `error` には型付き `ServiceError` を付ける。保存済み hash の byte size や検証 cache は
  現行モデルにないため、この query から hash mismatch を推測しない。
  render / collect は引き続き `resolve_asset` で内容全体を hash 検証し、誤った相対候補を
  正しい絶対候補へ黙って代替しない。query ごとの全内容読み込みはしない。
- 素材配置は明示した stream の1倍速。種類に合う track にだけ候補を出す。
  trim は既存の非空 subset の意味を維持する。同一 track の move は既存 linked move。
  track の表示・mute はモデル/API がないため tooltip 付き disabled、lock はユーザー UI 状態だけ。
  reverse・opacity/blend は read-only と理由を表示する。speed もこの実装では表示のみ。
- 配置は edit.md の左280px / 右296px / 下312px、track header200px。
  種類色はアイコン・2px下線だけ、選択は青、現在時刻は琥珀、欠落はdanger破線・icon・code。
  Inspector の配置時刻には中立色の `KRTimecodeField(currentTime:false)` を使う。
  focus は各 control の自身の状態だけに従う。保護された FocusRing 修正は GUI-002 の統合側が担当する。
- Composition クリップの「モーションで開く」は document Command を発行せず、参照先 Composition と
  Motion ページを開く。Edit の Sequence / clip 選択は UI 状態に保持する。
- AUDIO-002 の統合点は `activatePlayback(for:)` 一箇所。統合後は
  `configurePlayback(target:.sequence(id),rateNum:,rateDen:)` へ接続する。
  Edit Viewer に独立した再生タイマーや毎フレームの scene / sequence reload は追加しない。

## 検証

### supervisor review 後の追加決定（2026-10-05）

監督は `FrameRenderRequest.backend` の省略可能な closed enum（`gpu` / `cpu_reference`）を承認した。
省略時は従来の Service の選択を維持する。共有 current-frame renderer と `VideoRenderBackend` が
CLI `--backend cpu-reference` と同じ decode・pixels を作る。native surface は明示 CPU の linear pixels を
texture に upload し、preview result の `backend` に実際の `cpu_reference_float32` を返す。
GPU が `UNSUPPORTED_FEATURE: video requires explicit media backend` を返した場合だけ、Viewer は
「CPU 参照で表示」を提示する。ユーザーの選択後はその Sequence tab の session UI 状態とし、
ink-muted の「CPU 参照」を継続表示する。project や永続 UI state に backend を保存しない。
GPU の無言 fallback はない。その他の typed failure は元のまま表示する。

native request は一度に一件、未処理の変更は最新 frame に集約し、superseded completion / error を捨てる。
CPU 参照では再生中に新しい frame request を出さず、最後の frame と stale の注記を表示する。
停止後に現在 frame 一件を要求する。native redraw 自体は中断できず、協調 cancellation は後続課題。
Viewer error は target / revision / rational time に結び、target 変更時に消し、古い completion を採用しない。

速度と逆再生は time map の string rational を読む。ruler は Motion と同じ seconds / frames、header は
整数 fps または小数3桁。blade は clip Button より上の専用 AppKit hit area が mouse down/up を受け、
release 一回で split する。accessibility press は位置情報がないため clip の中央を一度分割する。
Inspector の checkbox は視覚 label を持たず、accessibility label だけを保持する。

[GUI-003 の検証](../testing/gui-003.md) に acceptance と actual checks、ホスト手順を記録する。
direct Swift の成功は SwiftPM runner・実画面・Metal・音声再生の受け入れを意味しない。
