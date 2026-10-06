# RENDER-002 / CACHE-002 検証

対象: root-scoped temporal sampling version 1（ADR-0080）。

## 再現手順

```sh
cargo test -p kronello-render --test render temporal --locked
cargo test -p kronello-service --test api temporal_profile --locked
cargo test -p kronello-cli --test jobs temporal_job --features kronello-service/test-job-control --locked
python3 scripts/generate_swift_api.py --check
```

`temporal_whole_composition_matches_independent_subtime_baseline_and_tiles` は
各 root subtime の通常 executor 出力を独立に平均し、temporal executor の線形
premultiplied 全画素と一致させる。表示変換は平均後の一回と比較する。
同じ構成の tile sink 出力、sequence の正確な標本 metadata と rate mismatch 拒否も確認する。

`temporal_cut_policy_clips_to_incoming_half_open_interval` は境界で incoming のみを露光し、
明示 allow_crossing では赤 / 青の平均を得る。
`temporal_crossfade_endpoints_are_not_hard_cuts` は crossfade 両端で露光を不当に分断しない。
`temporal_negative_ntsc_exposure_and_nested_scope_are_exact` は負時刻・30000/1001 の
厳密標本値と、ネストしても root 標本数だけの backend 呼出しを確認する。
ゼロ角度は一時刻へ縮退し、過大標本数は拒否する。

`temporal_cache_region_shutter_and_nested_time_map_invalidate_and_remain_bounded` は
暖機出力の同値性、ROI / 位相 / nested TimeMap の失効、entry / byte 上限と clear を確認する。
cache key は `snapshot.evaluation_content_hash` に含まれる effect / font / color / semantic
version と、各標本の要求・実行 ROI を保持する。動画は外部資産の実ファイル検証を
省略しないため temporal 出力 cache の対象外とする。

service の JSON テストは `RenderInput.profile.temporal` を共通 render.frame / render.sequence
へ渡し、job request が同じ設定を直列化することを確認する。
GUI の単一 DAG native preview と render.explain は temporal plan が未対応のため
`UNSUPPORTED_FEATURE` を返す。通常描画への暗黙 fallback は行わない。

## 実行記録

2026-10-06、macOS、Rust 1.95.0、作業 branch `codex/m4-completion`（base `8375caf`）。
- native Metal を使用した `temporal` suite: 10 passed、exit 0。sandbox 内では Metal adapter を取得できず、device access を許可した実行で検証した。skip / backend fallback は行っていない。
- 共通 service JSON profile test: 1 passed、exit 0。
- 独立 CLI worker test: 1 passed、exit 0。`temporal_sampling_v1` required feature、固定 snapshot の設定 / 意味版、元 project 削除後の 3-frame 出力と全 frame metadata を確認した。
- 公開 schema の regeneration / 照合、Swift generated API `--check`: exit 0。
- `cargo fmt --all --check`: exit 0。

GPU / CPU temporal 出力と GPU warm cache の同値性、変更された effect halo の失効と
暖機済み cache が missing font を隠さないことも suite に含む。
旧固定 snapshot で temporal pin がない場合は serialization / hash を保持し、
temporal profile があるのに pin がない場合と将来版は明示拒否する。
`require_gpu_resident` は CPU accumulation を許可しないため temporal を明示拒否する。
Workspace 全体の clippy / tests と GUI 検証は最終統合記録に記載する。
