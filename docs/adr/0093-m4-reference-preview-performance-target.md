# ADR-0093: M4 基準プレビューの性能目標

- 状態: 採用
- 日付: 2026-10-06
- 解決: OQ-14
- 根拠: 基準機の実測後、利用者がこの合否基準を明示的に承認した。

## 決定

M4 の基本4K previewの合否基準を、M4 Mac mini 32GB / Metal、既定GPU texture cache64MiB・surface pool64MiBで、warm native previewのp95が33.3ms以下とする。参照作品は examples/ffi-preview.project.json、出力3840×2160、画素が変わる0..20/30秒の21要求を使う。p50も報告し、p95はnearest rankで求める。固定作品・版・binary/source hash・OS・GPUを記録する。

共有 Service::preview_dag のcompile/layoutからGPU texture作成、sticky numerical validationと明示完了待ちまでを測る。停止画面のcache hitだけを再生性能として扱わず、各時刻の全画素を参照と照合し、複数の異なる出力hashを必須とする。他のbuild/test/GUIを止めて測定する。

coldはGPU context/pipeline/resource cacheを新規にした値として別途報告し、33.3msの合否を適用しない。動画デコードとGUI presentationを含む物理display FPSの保証ではない。デコード・movie export・complex lower-thirdは別の測定結果と対応範囲を持つ。全作品や全エフェクトの4K30を保証する基準に拡張しない。

60fpsは同品質での追加目標を維持し、M4の必須合否にしない。8K/HDRは正しいoffline出力を要件とし、リアルタイム性能は別途検討する。既定メモリー予算・色/alpha・数値エラーの契約を緩めて目標を達成したと扱わない。

## 実測と適用範囲

保守的な被覆範囲外の計算省略後、warm animatedのp50は17.163ms、p95は22.139ms、cold staticのp95は34.166msだった。全21時刻の画素が変化し、最適化前後の44組の解像度/時刻別linear/display hashも完全一致した。この測定は採用したwarm基準に合格する。正式な再現記録は [PERF-001](../testing/perf-001.md) に保存する。

complex lower-thirdのnative1080p/4Kは既存512MiBの保守的admissionで未対応とする。proxyとtiled finalの検証を別に保持し、その作品を基本シーンへ置き換えて成功扱いにしない。

[ADR-0090](0090-release-performance-evidence-and-snapshot-policy.md) の測定方法・保存方針は維持し、本ADRでそこで未決だった数値基準だけを確定する。
