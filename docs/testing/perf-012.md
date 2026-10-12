# PERF-012: status/画像待機統合と有界staging再利用

状態: `in_progress`。全計画は[依頼計画](performance-export-plan.md)で追跡する。GPU色量子化・temporal加算は変更しない。

## 実装と寿命

Movie/GIFとshutter sampleのlinear境界は、sticky statusと画像を同じencoderでcopyし、両mapを開始して一度の完了待ちで読む。callback成功とstatus正常を確認する前に画素を返したりcacheを公開しない。異常statusは従来と同じ型付きエラーになる。完了したreadbackのDropは全mapped viewの解放後にunmapする。未使用・map待機中・失敗したbufferはdestroyしてpoolへ返さない。

staging poolは全managed buffer（貸出中/idle）の合計4件/16MiB。size一致で再利用し、idleからの退避でadmissionを調整する。大きい/競合中のbufferは従来の一回限りのreadback allocationとし、poolへ残さない。GPU完了と全map callback成功後だけ再利用可能にする。失敗/未完了bufferをpoolへ返さない。返却された画素は独立したVecで、CPU consumerが保持中でもGPU bufferは影響しない。cache clearはidle stagingも回収し、allocation trackerはidleと貸出を含むactual descriptorを数える。

## 実測

[全測定](perf-012-measurements.json)と[保持出力のpacket再検証](perf-012-packet-identity.json)は固定4K source・0/3/8 shutter samples、実Metal worker、交互3pairsずつの初期比較。各試行は新しいworkerであり、同process warmと区別する。before=`284b2e17…`、wait統合のみ=`f21aa50c…`、poolあり=`53da21e4…`。静穏hostを保証しないため短いwall差を性能保証としない。

| disk persistence無効 | readback bytes（両側一致） | readback回数（両側一致） | wait before→after | staging allocation/reuse after |
|---|---:|---:|---:|---:|
| 0 samples | 66,355,360 | 80 | 80→40 | 5/75 |
| 3 samples | 199,066,080 | 240 | 240→120 | 5/235 |
| 8 samples | 530,842,880 | 640 | 640→320 | 5/635 |

poolの最大managed payloadは各3,604,484bytes。全decoded画素/PCM、PTS/DTS/duration、snapshot hash/tags、chapters、compressed packet/file bytesが一致した。後続worker p50は約973→920ms、2461→2400ms、4345→4175msだったが、各側の後続試行は2件で、compile/検証の並行を含む探索値である。RSSと各工程は源資料に別記する。

最初のwait統合のみの比較はdisk persistence有効で、cache保存用の追加readbackを含む。0/3/8 samplesの総waitは160→120、480→360、1280→960、bytes/readback数は不変。これをstatus/画像だけの80→40へ書き換えず、後者は明示`--disable-raster-cache`の比較で証明する。

## 検証と残る条件

実Metalのlinear_delivery（4件）はSDR/HDR/temporal/haloのbit比較、転送bytes・wait数、保持中のCPU画素の不変性、staging再利用とclear後の回収を確認した。sticky失敗/cache試験は透明/不透明な後段で隠したoverflowを繰返し拒否し、異常cache insert=0を確認した。

未完了leaseの追加試験で、map開始前のbufferへ無条件unmapする初期実装がwgpu validation panicとなることを検出した（`target/perf-012-pending-tests.log`）。未完了bufferのdestroyへ修正し、未使用5件（4件managed/1件一時）、map待機中の取消callback、16MiB超過の非保持、入力format不一致での早期失敗とallocation回収を実Metalで確認した。修正後は寿命1件・linear_delivery4件・sticky status1件が成功した。全workspace Clippy、fmt、生成Swift整合、backlog checkも成功した。修正前に開始したworkspace testsの結果とは区別する。

修正後の固定binary `1f05c4382d8400f22b87883c50ab8104f9ab0c26ae0bcd22acb99b2213a034d4`で4K・3 samplesを交互3pairs再測定した（`target/perf-012/fixed-samples3/evidence.json`）。全decoded bits/PTS/layout/chapters/packet/file bytesが一致し、後続worker p50は2584.5→2511.7ms、p95は2669.2→2632.3msだった。検証/buildとの並行を含むため壁時計の一般保証としない。

- 持続処理/RSSの比較を広げる。
- GPU/CPU色変換の重なりは現行の同期tile境界とnative encode consumerを区別して工程実測から判断する。新しいasync tile queueは未採用であり、poolだけでCPU packingとの重なりを保証しない。
- ADRと現在の設計、全workspace Clippy/testsの結果を確認し、受け入れ前にdoneにしない。
