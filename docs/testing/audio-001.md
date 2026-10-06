# AUDIO-001 音声特徴量 DataAsset と連動

状態: `done`。設計は [ADR-0096](../adr/0096-offline-audio-feature-assets.md)。rootが固有条件と統合checkpointを確認して受け入れ済み。[M5統合記録](m5-acceptance.md)を参照。

## 再現可能な検証

- `cargo test -p kronello-audio --test analysis --locked`: 固定FFT binの937.5 Hz・amplitude0.5の
  stereo sineを生成し、RMS=0.5/√2、対象帯域energy=0.125、別帯域ほぼzeroを比較する。
  rational timestamp、任意順序のsample、保存time-map、half-open終端、zero padding、silence、
  onset/beatと不正窓・不正timestamp拒否を確認する。
- `cargo test -p kronello-eval --test expressions --locked`: AudioFeatureは旧expression1で拒否し、
  expression2の静的DataAsset依存と任意順序・有理数時刻の特徴量評価、範囲外エラーを確認する。AUDIO-001受け入れcheckpointでは新規snapshotは能力2を固定した。EXPR-003追加後の現在の`EXPRESSION_SUPPORTED_VERSION`は3であり、
  pin省略の旧snapshotは`EXPRESSION_VERSION=1`を維持する。旧v1の描画成功と、v2をpin1で
  描画した場合の拒否を共有service試験で確認する。
- `cargo test -p kronello-service --test audio_analysis --locked`: 公開JSON `audio.analyze` Command
  から固定composition Busを解析して`.kronello`へ保存・再openし、evaluator版・sample数・frame数・
  RMS・revision競合・保存receiptの同一応答再送・キー再利用拒否を確認する。未知の解析版は
  lossless保存し、編集・実行を拒否する。persisted DataAssetをrenderのSceneIR作成経路で複数回評価し、
  元Asset hash変更後の特徴量参照を拒否する。

描画で呼ぶのは既存不変dataのindex参照であり、`analyze_audio`・FFT・decodeを呼ばない。
FFTは共有Commandの準備段階に限る。Bus解析のsnapshot hashは解析前Projectを対象とし、
編集後のlive Bus追従ではない。変更した元素材を古い特徴量へ黙って接続しない。

beat版1はthresholded onset pulseであり、tempo/拍子推定・音楽全般の精度保証ではない。
窓数65536、100,000,000 analysis work（両channel FFT・全band/bin判定・窓入力を含む）、PCMは10分上限で失敗する。
既存FFmpeg hash検証付き素材decode経路を使う。AAC/Opusの出力採用は対象外。

## 受け入れ条件と証拠の対応

| 条件 | 実装と検証 |
| --- | --- |
| 分析版/窓/ホップ/時間写像と入力hashを固定する | `AudioAnalysisDataAsset` は config と有理数gridを保存する。Asset入力は保存hashと現在の入力hashを検証し、Bus入力は解析前Project・target・range・evaluator版の固定hashを保存する。固定信号試験、保存/reopen、元素材hash変更拒否、未知版保持試験で確認する。 |
| 同じ出力フレームごとに音声全体を再分析しない | FFT/decodeは `audio.analyze` 準備経路のみ。eval/renderは保存済みframe配列のindex参照で、静的DataAsset依存を解決する。`renderer_consumes_persisted_features_and_rejects_changed_source` は保存済み特徴量を複数時刻でrender SceneIRへ反映する。 |

上記の個別証拠は両条件を満たす。全workspaceの安定checkpointとrootの受け入れ判断は別に記録する。

## 2026-10-06の実行結果と残件

`analysis` 3件、`expressions` 9件、serviceの`audio_analysis` 3件が成功した。Command再送は
元要求のcanonical payloadに対する保存receiptをrevision照合・decodeより先に確認する。
同一要求の確定済み応答再取得、異なる要求によるキー再利用拒否、receiptとDataAssetの
同一transactionでの確定を実装し、共有JSON経路で検証した。再送の旧残件は解消した。
両channel FFT・全band/bin判定を含むwork予算と窓数を処理前に拒否する試験も追加した。

保存snapshotのExpression pinについて、旧省略時の意味1はv1の描画を維持し、AudioFeature
を含むv2をpin1で実行すると拒否する。AUDIO-001受け入れ時の新規pinは2だった。現在はEXPR-003により`EXPRESSION_SUPPORTED_VERSION=3`であり、
旧pin2でのAudioFeature描画もservice回帰試験で維持する。
serviceの`expressions` 3件とCLI固定workerのExpression試験1件が成功した。

公開schemaとSwift APIをGUI共有契約を含む時点で再生成し、一致検証を通した。
API要求・実応答の網羅試験は44 operationを含む12件が成功した
（`target/m5-acceptance/api-contracts-final.log`）。
同時点のworkspace all-targets clippy `-D warnings` とfmt checkはexit0
（`workspace-clippy-final.log` / `fmt-final.log`）。

全workspace testの初回は追加APIの網羅試験不足、次の実行は検査試験の旧generic
Matte診断期待によりexit101だった。各ログを
`target/m5-acceptance/workspace-test-before-api-coverage.log` と
`target/m5-acceptance/workspace-test-gui-freeze.log` に保持した。
これらの失敗を音声解析失敗へ読み替えず、全workspace成功とも扱わない。
API不足とMatte検査期待は修正・個別再検証済みで、未到達のservice/store/template/testkit/
text/time/vector群の追加検証は公開schema変更による一致試験で停止した。
`remaining-workspace.log`、`remaining-workspace-substantive.log`、
`remaining-workspace-substantive-2.log`はすべて終了しており、最後の実行は
service `nle` のschema一致1件でexit101、同suiteの意味的試験15件は成功した。
公開契約freeze後の最終schema再生成・全workspace検証を別checkpointとして実行する。

公開契約freeze後のschema/Swift再生成、API 12件とnle_schema 1件、fmt check、
workspace all-targets clippy `-D warnings` は成功した（`api-final-freeze.log`、
`fmt-check-acceptance.log`、`clippy-acceptance.log`、`swift-check-acceptance.log`）。
全workspaceの`workspace-acceptance.log`はCLI jobsで12件失敗したが、同時実行のGUI
`build_ffi.py`がCLIテスト用binaryを通常binaryへ上書きし、`test-job-control`の待機・
障害注入hookが外れたためだった。共有targetのbuildを停止した単独再試験は
`jobs-gate-isolation.log` 1件、`jobs-build-isolation.log` 31件成功・2件ignored。
期待値・production実装は変更せず、`workspace-build-isolation.log`で全体を再検証し、
`cargo test --workspace --locked` はexit0で終了した。全119 suite・796件成功、
失敗0件・ignored 41件（専用環境試験等を含み、ignoredの全件実行とは扱わない）。

GUIのreverse/grid policyやblendを含むM5全体の最終CIと
受け入れstatusはrootの統合確認を待つ。この記録のみで`done`へ変更しない。

EXPR-003は `in_progress`。動的な過去Property sample・連続補間noiseの個別試験は成功し、
一般の固定table DataAsset参照と共有API/render統合・受け入れ検証を進めている。
AUDIO-001の完了をEXPR-003の完了とは扱わない。
