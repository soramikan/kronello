# ADR-0090: release 性能の観測と snapshot 方針の再評価

- 状態: 採用
- 日付: 2026-10-06
- 対象: PERF-001 の測定方法・保存方針。採用時点で未決だったOQ-14は、後続の [ADR-0093](0093-m4-reference-preview-performance-target.md) で解決した。

## 決定

性能の根拠は release binary と固定作品を使い、他の build/test/GUI を停止した逐次測定にする。binary、production/harness source manifest、作品の SHA256、実行 revision/dirty、実機を記録する。cold は要求の時計内で GPU context/pipeline/resource cache を新規作成する意味であり、OS disk cache の purge を意味しない。warm は既定64MiB texture cache /64MiB pool の持続 context、除外warmup1回の後に測る。cold/warmとも21samples、p50/p95は nearest rank とする。

停止画面の cache hit と、実際に画素が変わる30fps要求を分ける。異なる時刻だけで animation の証拠にせず、全 linear/display hash の種類数を記録する。native previewはDAGのhaloを正確にcropして共有 finalの全linear画素と厳密照合する。未対応は typed errorとn=0を保存し、他作品への置換やsurface budgetの増加によって隠さない。caseごとにcheckpointを出し、後続failureで既に検証した測定を失わない。

GPU allocationは実際に保持したdescriptor payloadの分類別live/peakとnode peakを示す。graph/cacheの共有所有物、idle pool、CPU出力Vec、OS RSS/physical footprintは別指標として扱う。admission surface estimateは保守的なpotential面数であり実測同時peakと呼ばない。driver/private codecの未知byte、未観測stage時間をゼロに置換しない。media decode/frontend/codec込みprocessとGPU融合は別の実測workloadを併記する。

## 観測

Apple M4 / RAM32GiB / Metal の実測を [PERF-001検証](../testing/perf-001.md) と [永続化JSON](../testing/perf-001-measurements.json) に保存した。basic shapeのnative4K previewは動く21時刻/21hash、p50 43.956ms /p95 45.782ms。停止画面は8.729/9.548ms。33.3msを達成したと扱わない。complex lower-thirdはlayoutによって同じ画素になり、異時刻caseはmotion性能の証拠にしない。native1080/4Kは既存512MiB admissionを超えるtyped unsupported、bounded tileを使う4K finalは全要求が成功した。

独立した before/after は [GPU融合](../testing/perf-001-gpu-fusion.md) と [media seek](../testing/perf-001-media-seek.md) にある。異なる作品・解像度・入口の時間を比較して最適化の効果と呼ばない。GUI presentationと実時間音声を含む実再生の時間はこのharnessでは測らない。

## 保存方針の再評価

同じ実lower-thirdをrelease共有APIで129 edits（property32/template入力32を含む）、32 selective undo、128 history query、42 historical restoresした。復元のcold/warm各21samplesは完全参照文書と一致し、p95は23.717/23.263ms。頻度はscripted workloadの回数であり人間のtelemetryではない。

固定周期のsnapshot3件/35,791bytes、DB851,968bytesを観測した。実OS process diskioのediting writeは40,759,296bytesであり、mutations/inverseの論理payloadやclose後WAL0bytesとは区別する。VFS別byte/fsync回数は未観測。ADR-0052のdebug合成候補比較は統計量とworkloadが異なるため、このrelease値のbaselineとして直接差分を取らない。

**ADR-0052の既定不採用を維持する。** 初期revision0、64revision周期、明示compact基点、履歴自動削除禁止を変更しない。今回の固定周期では最大63patchを含む復元が約24ms以内のp95となった。一方、自動追加snapshot候補の同じ実作品での物理I/O・容量と利用者の復元頻度は比較できていないため、全作品へ複製書込みを増やす既定を導入する根拠は不足する。頻繁な大型作品復元や確定した復元目標が得られたら、同じrelease/通常編集workloadで候補の物理I/Oを比較する。

本ADRは測定方法と既存保存方針の維持を決める。この時点で未決だったOQ-14のframe予算は後続のADR-0093で確定した。測定方法とsnapshot方針の決定は維持する。

## 関連

- [ADR-0052](0052-snapshot-policy-evaluation.md)
- [ADR-0091](0091-exact-forward-decoder-and-bounded-render-scope.md)
- [ADR-0092](0092-single-graph-gpu-final-output-and-observations.md)
