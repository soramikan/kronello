# ADR-0150: 書き出しとshutter sampleのlinear専用境界

- 状態: 採用
- 日付: 2026-10-11
- 対象: PERF-004
- 部分置換: ADR-0092の常時二面生成、ADR-0097のtemporal sampleごとの二面readback見積もり。通常frameのlinear/display契約、単一graph、validation、cache公開・所有権・観測scopeは維持する。

## 背景

Movie/GIFの最終色変換はlinear画素を入力とするが、従来は使わないdisplay面も変換・readbackしていた。temporal accumulationも各shutter sampleのlinearだけを足し、最後にdisplayを一回変換するため、中間sampleのdisplayが不要だった。

## 決定

- `RenderBackend::execute_linear_with_cache` / `execute_linear_with_inputs` はlinear画素だけを返す。注入backendの既定実装は従来の両面出力からlinearを取り、互換性を保つ。CPU/GPUの製品backendとsoftware/sequential/resident media adapterは専用経路を実装する。
- Movie/GIFは共有 `render_frame_linear_tiles` を使う。通常frame/PNG/previewの公開出力は両面のまま。FrameMetadataの色/alpha/pixel formatは契約記述を維持し、転送実測を要求単位で集計する。音声単独は映像入口自体を呼ばない。
- GPUは同じgraph、意味key、halo/crop、scene予算、sticky shader validationとcache公開順を使い、display dispatchとreadbackだけを省く。linear専用の中間texture copyを加えない。画像一面のrow-padded readbackと4-byte statusをそれぞれ一回待つ。disk persistenceによる追加転送は従来の明示policyのまま。
- 全temporal入口の中間sampleはlinear専用とし、同じ有理数sample/weightとfloat64 accumulationを維持する。通常frameは最後にdisplayを一回変換する。linear tile streamingはdisplayを変換せず、両面 `BackendFrame` を保持するtemporal cacheへentryを作らないため、片面cache hitが両面出力へ漏れない。
- strict resident + temporalの型付き未対応は維持する。未対応、missing media/font、数値異常、pixel数不整合、sink失敗を成功へ変換しない。
- 検査Queryの通常frameは従来の二画像+status、temporal sampleは一画像+statusを見積もる。final hostのlinear/displayとCPU accumulationを別に説明する。Movie専用selectorのない `render.explain` をMovie性能見積もりとして扱わない。

## 検証

SDR/HDR/temporalのCPU全画素、両面cacheのcold/warm、失敗伝播、実Metalのhalo・sample/tile・row padding・readback/wait・inspect推定を確認する。同じrelease binaryで画素hash・encoded MOV bytesを比較し、初回/反復p50/p95・転送・process memoryと測定限界を [PERF-004](../testing/perf-004.md) に記録する。
