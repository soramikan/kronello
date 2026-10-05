# ADR-0078: テンプレート比較と検査した revision の書き出し

- 状態: 採用（SwiftPM・Metal・画面の受け入れはホスト検証待ち）
- 日付: 2026-10-05
- 対象: GUI-004

## 決定

監督が以下の共有 API の追加と表示範囲を承認した。ADR-0054 / 0055 の配色・寸法と
screens/template.md / export.md を継承し、作品専用の GUI モデルを追加しない。
EditorWindow の変更は2行の page routing に限り、GUI-003 / AUDIO-002 / INTEGRATION-002 の
所有ファイルへ実装を混ぜない。ADR-0075 は未マージの GUI-003 worktree で読み、routing のみ参照した。

- 配色の「琥珀は今」は ADR-0054 のブランド例外を含む。Export の「書き出す」は
  ページで一つの primary、ほかは secondary / plain とする。選択・focus は selection、
  エラーは danger。supervisor が既存 primary の維持を確認した。
- `capabilities.get.export_profiles` を追加する。`JobOutput` の各 variant から
  format、profile_versions、audio_modes、閉じた audio_codecs、container_extension、
  execution、encoder_registered、device_availability、任意の型付き reason を返す。
  Swift はこれを選択肢として使い、任意 codec / encoder / FFmpeg 引数を送らない。
  ProRes/PCM24 は版1〜3、AV1 MP4 / H.264 MOV / HEVC MOV は版1・ALAC。
  image_sequence も既存の閉じた出力として公開する。AAC は採用待ちのため選択肢から省略する。
  ハードウェア encoder の登録は device が開けることを証明しない。登録なしは
  `ENCODER_UNAVAILABLE` / unavailable、登録ありは unverified。codec session を開いて探査しない。
- `RenderSubmitRequest.expected_revision` を任意で追加する。省略は従来の意味を維持し、
  指定時は取得した保存 snapshot の revision が一致しなければ `REVISION_CONFLICT`。
  ジョブ記録・worker 起動より前に拒否し、同じ型を使う `render.export` も捕捉した
  snapshot を backend 初期化前に照合する。GUI は意味的検査の revision を必ず指定し、
  競合は既存 `KRConflictBanner` に「再確認」を付けて示す。再確認で最新の状態を読み、
  自動投入しない。UI のチェックと投入の間に別セッションが作品を変える競合窓を閉じる。
- 事前確認は一時刻を受け取る共有 `render.explain` による範囲先頭の意味的検査。
  全フレーム・音声 clipping・codec open の成功とは表示しない。型付き error、設定不正、
  既存 destination、古い revision / 設定、未確認の間は Start を無効にする。
  profile の unverified は error と捏造せず、投入後の型付き失敗を JobRow に示す。
- 投入は既存の固定 snapshot / 独立 worker を再利用する。JobRow は service の revision /
  snapshot_hash と queued / running / succeeded / failed / canceled / interrupted を表示する。
  interrupted は中断として区別し、自動再開しない。進捗と失敗を shared `job.list` で読み、
  1秒以上の間隔で一覧を一度だけ取得する。active job がなくなるかページを閉じると停止する。
  手動更新 / ページ再入場で再読込できる。cancel は `job.cancel` の要求であり、確定まで中止と偽らない。
- Template は一つの変更につき variant ごとに一度 `template.preview` を問い合わせる。
  bounds segment の切替は取得済み layout / ink / visual を使い、frame timer に結び付けない。
  比較は明示した候補入力と有理数時刻で、画素の描画成功とは表示しない。
  overflow で共有 compiler が nodes を返せない場合は `TEMPLATE_OVERFLOW` と
  「この variant の bounds は取得できません」を出す。geometry は捏造しない。
- 公開入力の比較だけは読取り候補。配置への適用を明示したときだけ
  `TemplateCommand::SetInput` を一つの `edit.plan` → `edit.apply` として送る。
  String、数値、vector、Bool、Enum、Color、MediaSlot、inline DataTable の型付き欄を使い、
  table column を勝手に変えない。候補は開始 revision を維持し、外部変更で古くなれば
  `REVISION_CONFLICT` と明示した再適用へ進む。URL fetch・任意実行を持ち込まない。
- `set_duration` は配置の duration だけを変える。duration_policy の下書き（hold / loop /
  stretch と最低中間尺）は新 UUID・新 version の immutable definition を公開する。
  表示の秒ルーラーは共通 scale の2帯と保護区間を示す。描画用の float を保存時刻に戻さない。
  既存配置は変えない。公開と移行を別の操作として表示する。
  `template.migration_plan` の field / before / after と diagnostic を Dialog で確認し、
  明示した適用だけが共有 template migration command を送る。古い計画は適用できない。
  適用は EditorModel の通常の plan / apply、receipt、セッション Undo、競合処理を使う。

## 検証と範囲

[GUI-004 の検証](../testing/gui-004.md) に criterion、実行済み checks と pending host procedure を対応付ける。
Dark / Light、1440×900 の gallery screen は共通 layout と component を組み立てた例示データであり、
実プロジェクト・Metal の画面受け入れを代替しない。GPU / hardware codec の成功を sandbox の結果から導かない。
Template の画素 preview、overflow 時の構造化 geometry、作者用の variant 作成操作、
全範囲・音声 / encoder の export preflight は今回の実装の証拠に含めず、後続を別記する。
