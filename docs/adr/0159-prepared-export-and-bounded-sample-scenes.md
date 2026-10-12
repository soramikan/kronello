# ADR-0159: 書き出し準備と有界sample SceneIR共有

状態: 採用（PERF-007受け入れ進行中）
日付: 2026-10-12
部分置換: ADR-0144のprevalidated経路の不変hash計算、ADR-0150のMovie sample評価境界。linear専用出力・転送・float64積算の決定は維持する。

## 決定

- 同期exportと固定workerの共通media export入口で`ExportRenderSession`を一度だけ作る。検証済みowned snapshot、static sequence lowering、canonical/evaluation hash、LUT構造検証、font identityと不変bytes、metadataの共通部分を保持する。通常のraw snapshot入口は従来どおり検証する。
- `VerifiedFonts`はhash/face identityを照合したbytesを`Arc`で所有する。exportのlayoutはこの不変の所有に結び付いたproofを使う。通常の`RenderCache::layout`は、proof自身が保持する同一bytes/identity以外を受け取った場合は従来の検証へ戻す。missing/changed/duplicate fontをwarm hitで隠さない。
- 一意な有理数sample時刻のSceneIRを同じsnapshotのsession内で共有する。DAGは従来のtile ROI/haloごとに構築する。sample順・有理数weight・half-open cut・f64乗算/加算順・最後のf32変換を維持する。中央時刻がsampleと同じなら共有する。
- SceneIRは最大16件・64MiBのserialized semantic payloadのLRUとする。weightは全fieldを容量上限付きwriterへ直列化して数え、巨大な二つ目のJSON bufferを作らない。上限超過のsceneは保持せず実行する。大きいsample数は再評価を許し、無制限の先行準備をしない。評価中のscene・中央scene・tile scratchはcache予算とは別に存在する。
- 意味cacheの容量はvalue/simulation各8MiB、layout/geometry各16MiB。raster/complete temporal frame cacheは無効。logical payloadとallocator/device/RSSを区別し、診断にSceneIR/font/prepared input保持量を記録する。
- SceneIR等の`Serialize`はweight観測専用の内部表現で、文書・公開API・永続cacheの版を新設しない。sceneの永続化やDeserializeは提供しない。

## 検証

通常/temporalの日本語全画素bit、3/8 samplesの評価回数、不変処理の回数、cut/ROI/blur halo、4096 samplesの件数/byte上限、font所有と失敗伝播をテストする。固定release binaryの実Metal worker A/Bでsnapshot identity・全decoded bits・PTS/layout・encoded bytesとstage counterを比較する。受け入れ証拠は[PERF-007](../testing/perf-007.md)に残す。
