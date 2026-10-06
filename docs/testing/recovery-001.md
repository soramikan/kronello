# RECOVERY-001 検証記録

## 再現

```sh
cargo test -p kronello-jobs --features test-worker --locked
cargo test -p kronello-cli --test jobs --locked
cargo clippy -p kronello-jobs -p kronello-service -p kronello-cli --all-targets --locked -- -D warnings
```

CLI integration は本物の `kronello worker --job` を起動する。`test-job-control` は debug-only
で、通常の registry / worker を置き換えず checkpoint と rename の直後だけを停止可能にする。
Unix の hard terminate は SIGKILL、Windows は process handle による強制終了である。

## 受け入れ証拠

- `recovery_resume_rebuilds_killed_output_from_fixed_snapshot`: 1 frame 完了後に worker を強制停止。
  interrupted と owned stage を確認し、Project を revision 2 へ編集してから `job.resume`。
  同じ input / snapshot hash と revision 1 で 0 frame から 3 frame を再実行する。
  abandoned stage を回収し、隣の他 job 相当 directory は残す。
- `recovery_reconciles_rename_before_database_commit_only_after_artifact_verification`:
  image sequence と実 ProRes / PCM24 MOV を NOREPLACE rename 後、DB commit 前で停止し強制終了。
  metadata / movie byte 改変は `OUTPUT_VALIDATION_FAILED` になり、output と interrupted を維持。
  検証済み bytes を戻すと同じ worker PID 記録のまま succeeded に補正し、新 worker は起動しない。
- `recovery_capacity_failure_cleans_staging_and_resume_uses_new_attempt`: 実 worker の checkpoint に
  OS `StorageFull` error を注入し、Project bytes と destination を保護、owned stage を回収。
  fault を外した明示 resume は attempt 1 で完成する。実 volume を満杯にした測定ではない。
- `recovery_device_loss_cleans_staging_and_resume_preserves_completed_output`: 1 frame 完了後の
  checkpoint に `GPU_DEVICE_LOST` を注入し、Project と別 job の確定済み manifest が変わらない
  こと、temporary output 回収と新 attempt の成功を確認。物理 GPU の故障を起こした検証ではない。
- store recovery tests は同時 resume の winner が一つであること、旧 attempt の drop / publish の
  fence、cancel 後の publication 拒否、異なる input hash / owner marker の非削除、巨大 marker、
  symlink、DB に anchor された receipt の改変拒否、destination が無い partial receipt の再試行を確認。
- `slow_receipt_and_reconcile_validation_leave_peer_heartbeat_free_and_prune_keeps_result` は receipt
  書き込み後・DB anchor 前と、artifact 検証後・status commit 前の実行を停止し、その間の別 job の
  heartbeat writer が成功することを確認。prune 後の get / list でも完全な歴史的 result を保持する。
- `expired_resume_controller_cannot_launch_or_fail_a_new_attempt` は controller が PID 登録前に停止した
  queue を sleepなしで期限切れへ進め、別 resume の attempt 2 を作る。attempt 1 の起動と failure
  completion は `JOB_INTERRUPTED`、attempt 2 の queued/PID/error は変わらない。現在の attempt の
  本物の executable 欠落は `WORKER_DETACH_ERROR` として failed になることも確認する。submit /
  resume / worker failure が同じ attempt fence を通り、旧 by-ID Rust API の互換は維持する。
- hard-kill tests は process handle で終了を確認した後、その killed record の heartbeat だけを
  expiry 前へ進める。次 worker は production の 30 秒 timeout を保ち、CI 負荷に依存する
  1 秒 watchdog の誤ったテスト前提を置かない。生存 PID の expiry 拒否は別の実 process test で確認。
- 既存 CLI worker の asset hash、font lock、構造版、意味版、required feature、input hash の失敗
  regression に、同じ固定入力の `job.resume` が同じ typed error で拒否する確認を追加。
- 既存 slots / cancel / lease / writer contention / destination race tests も同じ job store を通す。
  確定済み output と Project は失敗時に上書きしない。

## ローカル結果

2026-10-06、macOS Apple Silicon、Rust 1.95.0。
CLI jobs は 31 passed / 2 ignored（既存の physical VideoToolbox H.264 / HEVC host run）、
jobs test-worker は 8 unit / 8 process / 8 state passed、別 volume を要する 1 process test は ignored。
jobs / service / CLI の scoped clippy と workspace fmt check は成功。
コマンド出力と検証時の source SHA-256 は `target/recovery-001-evidence/evidence.json` と同 directory の logs に保存する。
Windows / Linux の新 recovery process tests は CI 実行結果と区別する。

2026-10-06 の追加 attempt fence 修正: jobs通常テスト9 unit /8 state、CLIのrecovery4件（1.60秒）と通常submitの独立worker1件（0.90秒）、jobs/service scopedclippy（6.69秒）が成功。workspace全体の最終gateはroot担当の別結果である。
