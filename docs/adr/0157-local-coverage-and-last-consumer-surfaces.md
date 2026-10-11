# ADR-0157: 保守的な局所coverage面と最終consumerでの解放

- 状態: 採用（PERF-005の最終縦断受け入れは別管理）
- 日付: 2026-10-11
- 部分置換: [ADR-0141](0141-peak-live-render-surfaces.md)のGPU shared memoをpass末尾まで保持する動作と、すべてのcoverage面をexecution union全面で確保する動作。事前予算式は従来の保守的上界を維持する。

## 決定

GPU executorだけに閉じたruntime `SceneSurfaceMode`を導入し、既定を`LocalCoverageAndLastUse`とする。純粋モデル、保存schema、Command/Queryへdevice型や選択状態を追加しない。比較用`FullCanvas`は従来の全面allocationとmemo寿命を選ぶ。runtime namespaceを更新して旧texture/disk cacheと混ぜない。

trusted fill-only pathの既存coverage boundsだけを使い、guardを含む整数output pixel矩形で面を確保する。sizeは最小32、2の冪のbucketへ切り上げ、canvasに収める。端ではoriginを移して全boundを含める。グローバルの4×4サンプル格子とf32演算を変更せず、textureへの書き込み座標だけを局所化する。読み手はoriginを引き、面外をtransparentとして扱う。全面inputには同じ直接textureLoadのfast pathを残す。

stroke、gradient、極端座標/不正scale、不確実なbound、空またはcanvas外のboundは全面へ戻す。Effect/EffectMapの直接入力も全面のまま確保し、既存halo、filter clamp、displacement/global samplingを維持する。他のproducerは全面である。部分面を全面へ復元する追加GPU copyは行わない。復元copyを使った初期試作は画素が一致したが、交互計測でeffect作品の時間が悪化したため不採用とする。

original edgesと重複rootのconsumer数を既存`surface_references`から求め、利用のたびに減らす。最後のconsumer後にshared memoを捨てる。呼び出し中のlease、返却面、ordered command queue、cache ownershipは独立して生きる。parentのsemantic cache hitが下流を省略した場合の参照数は過大なまま残してpass末尾で捨てる。省略した入力の所有権を早く捨てない。

局所leafのsemantic keyは元digestに実origin/extent/pixel dimensions/working spaceを加えて分離する。cache lookupは実寸法を検査する。親は従来どおり完全な意味的出力を表し、全passのsticky異常値検査成功後だけ最大8件を公開する。poolは既存の有界予算とlease ownershipに従う。異常値を透明領域や不透明parentで隠してcacheへ公開しない。

事前`tile_surface_bytes`はunion全面・shared memo全期間の上界を維持する。局所面の実GPU descriptor payload/ピークは観測値として別に報告する。driver、allocator、idle pool、native、whole-process RSSの上限や削減とは呼ばない。CPU executorの意味・寿命・予算は変更しない。

## 検証

[PERF-005](../testing/perf-005.md)に固定before binary、同じdeviceでのFullCanvas/最適化の交互cold/warm、全linear/display f32 bits、複雑な日本語project/font/layout/DAG、共有mask/effect、node別資源ピーク、RSSを記録する。4K、anisotropic scale、fractional clipping、Evenodd、implicit close、negative coordinates、stroke/gradient fallback、dirty pool、cache hit、sticky failure、shared最後の利用後の解放を実機で検証する。quiet host/FPSの保証と区別する。
