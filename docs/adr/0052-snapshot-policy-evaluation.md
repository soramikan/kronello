# ADR-0052: サイズ閾値による追加 snapshot の既定採用を見送る

- 状態: 採用
- 日付: 2026-10-04
- 対象: STORE-003

## 背景

ADR-0046 が STORE-003 に委ねた「直前の完全 snapshot 以降の patch 累計が現在文書のサイズを超えたら追加 snapshot」を、64 revision 周期と比較した。ADR-0030 の履歴自動削除禁止と、STORE-002 の最大 63 patch 復元・旧ファイル互換性を維持する必要がある。

## 評価

`tests/snapshot_policy.rs` の再現可能な合成履歴を使い、実際の SQLite DB と `ProjectStore::snapshot_at` を比較した。候補は評価用 DB にだけ追加 snapshot を保存する。patch は保存済み mutations の compact UTF-8 JSON、文書は正規化された Project の compact UTF-8 JSON とし、inverse / changed keys を閾値に含めない。現在 revision の patch を累計に含め、**厳密に `>`** の場合に保存して累計をゼロにする。周期 snapshot でもゼロにする。

macOS arm64 / Rust 1.95.0 / debug build の測定（2026-10-04、HEAD `0775916` に本変更を加えた worktree）。各履歴の全 revision を完全な参照文書と照合し、再 open 後の復元を 3 周測定した。時間は各周の平均の中央値。最終 DB 容量は close / checkpoint 後。詳細な byte 数・手順・限界は [STORE-003 の検証](../testing/store-003.md) に記載する。

| 履歴 | 復元平均 ms: 固定 → 候補 | DB 容量増加 | 論理書き込み量増加 | snapshot 数: 固定 → 候補 |
|---|---:|---:|---:|---:|
| 小さな名前変更 256 回 | 0.810 → 0.060 | 17.4% | 13.2% | 5 → 89 |
| 約 192 KiB の文書を root import 12 回 | 83.126 → 7.116 | 49.7% | 34.3% | 1 → 13 |
| 32 定義・128 instance のテンプレート文書、32 回の import / 集合変更 | 170.818 → 13.674 | 40.6% | 22.9% | 1 → 17 |

## 決定

**この閾値による自動追加 snapshot を既定方式として採用しない。** 初期 revision 0、`revision % 64 == 0`、明示的な `compact(r)` 基点を維持する。

候補は今回の全ケースで復元を大きく短縮した。一方、root `Set` は patch の外枠だけで文書サイズを超えるため毎回追加保存になる。大きな集合を繰り返し置換するケースもほぼ隔 revision の保存になり、文書・patch・inverse に加えて完全 snapshot の複製を増やす。履歴を自動削除しないため、DB 容量と書き込み量の増加は長期利用で蓄積する。現在は履歴復元の確定した遅延目標・実作品での復元頻度・物理 I/O の測定がなく、この増加を全プロジェクトの既定にする根拠は不足している。

復元を頻繁に使う大きな文書には再検討の価値がある。今回の不採用は将来の opt-in、snapshot 数・容量の予算、root patch の重複削減などを否定するものではない。それらはこのタスクでは実装・仕様化しない。

## 影響と検証の境界

- 保存層の production API、SQLite schema / `user_version=1`、snapshot 作成条件に変更はない。ADR-0046 を補完し、既存の採用決定は置換しない。
- revision 照合・inverse・receipt・idempotency・compact の意味と、旧全 revision snapshot ファイルの非破壊 open を既存テストで継続確認する。
- 評価用の追加 INSERT は別 transaction。最終配置と復元性能の実験であり、採用実装の atomicity / 書き込み遅延を検証したものではない。論理書き込み量は payload の計数で、SQLite / WAL の物理 I/O・fsync 回数ではない。
- debug build の合成保存履歴を用いた比較であり、Service の template 操作・意味検証や release build の実作品性能の証拠にはしない。
- 実同期フォルダ・ネットワーク FS、Linux / Windows 実行は別の受け入れ条件として未確認。STORE-003 は `in_progress` に保つ。

## 関連

- [ADR-0046](0046-store-format-and-location-policy.md)
- [09 保存と同時編集](../architecture/09-storage-concurrency.md)
- [STORE-003 の検証](../testing/store-003.md)
