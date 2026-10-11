# MODEL-004: 既知不正文書の診断と CSS easing

日付: 2026-10-11。初回実装ではコミットを行わず、作業開始時の他エージェントの変更を維持した。PR取り込み時の追加確認は末尾に記録する。

## 変更と受け入れ証拠

- `crates/kronello-model/src/project.rs`: decode に失敗した候補を公開型 schema で分類し、未知種類・フィールド・意味版を保持、既知不正は `ProjectError::InvalidObject(ObjectDiagnostic)` で拒否する。保存・import・履歴復元で同じ `validate_storage` を使う。`ensure_editable` はすべての opaque / 未対応 object を ID と集合名付きで列挙する。
- `crates/kronello-store/src/store.rs` と `crates/kronello-service/src/lib.rs` / `edit.rs`: `INVALID_DOCUMENT_OBJECT` と構造化 `details` を共有 API へ伝える。編集不可の `UNSUPPORTED_FEATURE` も対象リストを失わない。通常編集では先に domain の構造検証を行い、既存の `STROKE_INVALID_DASH` / `INVALID_EDIT` を維持する。
- `curve.rs` / `kronello-animation/src/lib.rs`: 各 x を独立に `[0,1]` に制限する。x 単調性の証明は ADR-0145。既存の有効曲線の計算は維持し、従来無効だった x1=1 / x2=0 だけ中央の stationary point を解析的に反転する。補間意味版は1のまま。
- `shape.rs` / `text.rs`: 既知の intrinsic geometry / text coverage・selector 構造も保存前に診断する。未知の geometry / layout 意味版には既知の制約を推測して適用しない。
- macOS `KeyframeEditing.swift`: 相手ハンドルの x に依存する clamp を削除し、表示評価の stationary point もコアと一致させる。
- ADR-0145 で ADR-0046 と ADR-0070 を部分置換し、architecture 01 / 03 / 09 と公開 project / API schema、GeneratedAPI.swift を更新した。保存構造・Swift transport 型の変更はなく、生成 Swift の変更は schema checksum。

| 受け入れ条件 | 再現可能なテスト |
|---|---|
| 既知不正 curve / shape / text の集合・ID・理由 | model `project_diagnostics::known_invalid_objects_have_typed_storage_diagnostics`、service `project_changes::known_invalid_objects_are_rejected_by_create_import_and_plans_atomically`。4入口を検証し、create target 不在・import revision / 文書不変を確認する。decodable text の不正 coverage も含む |
| 保存の原子性 | store `storage::invalid_known_opaque_import_returns_addressed_error_without_writing`。revision・文書・event 不変 |
| 未知版・kind・nested field の lossless 保持 | model `future_versions_kinds_and_nested_fields_remain_lossless_and_are_addressed` / `unknown_map_payloads_are_preserved_but_known_invalid_map_payloads_are_rejected` と既存 shapes / store round-trip。service `future_version_opaque_import_is_lossless_and_edit_error_names_object` は create/import/両plan/export/edit.plan を検証する |
| すべての編集不可 object を列挙 | model `every_opaque_object_is_listed`、service の上記テストで `details.objects` も照合 |
| CSS easing の評価と編集可否 | animation `crossed_css_handles_match_reference_and_remain_monotonic` は `.65/.35`, `.83/.17`, `1/0` を各1001点・中央近傍・JSON往復で参照評価と比較。service `css_crossed_easing_survives_create_import_and_remains_editable` は4入口と keyframe_replace plan を確認 |
| GUI の独立 x clamp・y overshoot・中央表示 | Swift `MotionAuthoringTests.testCrossedTimeHandlesRemainIndependentDuringTangentEditing` |

## 検証コマンドと結果

| コマンド | 結果 |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 最後の再実行は exit 0 |
| `cargo test -p kronello-model -p kronello-animation -p kronello-store -p kronello-service --locked --no-fail-fast` | exit 101。533 passed / 1 failed / 9 ignored。失敗は Metal adapter 不在の既存 GPU テストのみ |
| `python3 scripts/generate_swift_api.py --check` | exit 0 |
| `python3 scripts/backlog.py check` | exit 0、151 tasks、MODEL-004 完了後も exit 0 |
| `git diff --check` | exit 0 |

検証途中の workspace clippy は、並行作業中の `kronello-media/src/render.rs` の引数不一致 `E0061`（3箇所）で一度失敗した。本件では当該コードを編集せず、その並行変更の解消後に同じ command を再実行して成功した。
初回全体テストで見つかった編集の error code 2件は検証順で従来の code を維持し、create の不正 node tags は新しい object 診断へ期待値を更新した。修正後の `editing` 21件、`paging` 5件、`project_changes` 5件は別途 exit 0。新しい model 診断テストは4件 exit 0。

対象4 crate の最終集計（通常テストと doc-tests）:

| crate | passed | failed | ignored |
|---|---:|---:|---:|
| kronello-model | 204 | 0 | 0 |
| kronello-animation | 15 | 0 | 0 |
| kronello-store | 39 | 0 | 1 |
| kronello-service | 275 | 1 | 8 |
| 合計 | 533 | 1 | 9 |

失敗は `kronello-service --test nle2` の
`gpu_nle2_clip_effects_and_explicit_video_upload_match_cpu` で、
`AdapterUnavailable("No suitable graphics adapter found; ... metal found no adapters ...")`。
Metal adapter が利用できない実行環境による失敗であり、この診断・補間修正の失敗として扱わない。
8件の ignored は明示された実 GPU / VideoToolbox hardware 検証、残り1件は
`kronello-store` の `snapshot_policy_evaluation`（明示実行が必要な測定）。
これらの実行を今回の成功証拠には含めない。

Swift の指定コマンド
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --package-path apps/macos -j 3`
は `~/.cache/clang/ModuleCache` の書き込みが sandbox で拒否され、manifest をコンパイルできなかった。
キャッシュを `/private/tmp` に変更した次の試行では SwiftPM の `sandbox-exec: sandbox_apply: Operation not permitted` によって停止した。
この制約に対し、書き込み可能なキャッシュと SwiftPM `--disable-sandbox` を指定して検証した。外側の実行環境の sandbox は変更していない。

```sh
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
CLANG_MODULE_CACHE_PATH=/private/tmp/kronello-clang-cache \
SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/kronello-swift-cache \
swift build --disable-sandbox --package-path apps/macos -j 3

DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
CLANG_MODULE_CACHE_PATH=/private/tmp/kronello-clang-cache \
SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/kronello-swift-cache \
swift test --disable-sandbox --package-path apps/macos -j 3 \
  --filter MotionAuthoringTests.testCrossedTimeHandlesRemainIndependentDuringTangentEditing
```

上記 build は exit 0。選択した Swift テストは1 passed / 0 failed。
画面の操作確認・GPU golden 比較は今回の受け入れ証拠には含めない。

## PR取り込み時の追加検証（2026-10-11）

性能改善との共有ファイルを整理し、PR #18を親にMODEL-004だけの差分とした。Rust/Swift/native/GPUを含む589ソースのSHA-256は、[最終全体検証](perf-005-final-validation.json)に記録したものとすべて一致した。同ソースではfmt・all-targets Clippy・workspace tests（通常1325件、doctest2件）が成功し、47 ignoredは成功数に含めない。上記のsandboxでの初期失敗と、このMetal利用可能な環境での全体成功を区別する。今回、同じソースの全体テストを再実行したとは扱わない。

今回のfmt・Swift API生成チェック・backlog再生成/チェック（151件）・差分チェックは成功した。既存releaseテストバイナリで交差ハンドルのGUI回帰を再実行し、1 passed / 0 failedを確認した。source照合・実行コマンド・binary/log hashは[取り込み検証記録](model-004-pr-validation.json)を参照する。
