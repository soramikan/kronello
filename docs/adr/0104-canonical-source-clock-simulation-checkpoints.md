# ADR-0104: canonical source clock の固定刻み Simulation と checkpoint

- 状態: 採用（SIM-001 は依存タスクの受け入れ後、実装・検証中）
- 日付: 2026-10-06
- 関連: SIM-001、ADR-0003、ADR-0043、ADR-0045、ADR-0102、ADR-0103

## 文脈

粒子は状態を積み上げるため、通常 Property の任意時刻評価から分離する。ただし粒子の入力に別の静的設定専用系を作らず、Constant / Curve / Expression を含む通常の authored Property を使う。シークや逆再生で現在のレンダー時刻を過去の全 tick に流用すると、同じ source 時刻でも履歴に依存した結果になる。

## 設計方針

版付き ParticleSimulation は、固定 rational start / step / emission interval / lifetime、保存 emitter ID / seed、共有 `RepeatSource` と、SceneNode が所有する通常 PropertyId の dynamics 入力を保持する。origin、初速、初速 jitter、加速度、発生制御を各固定 tick の共有評価エンジンで解決する。位置・速度は版 1 の forward semi-implicit Euler で更新する。保存の正本に floating time、GPU 資源、乱数 generator の非固定状態を置かない。

particle identity は emitter UUID と simulation 内の birth event serial を基に固定する。state の配列位置は identity ではない。seed / algorithm version と共に生成順を固定し、同じ意味的入力で同じ event ID / state を得る。通常 compile は共有 Source の配置を生成し、Source の全編集オブジェクトを各 particle の文書へ複製しない。

## 二つの時計

actual playback mapping は、現在の要求から表示したい authored source 時刻 `q` を選ぶ。この経路には正方向、inverse / reverse sampling、protected loop / hold を含める。`q` は表示する forward state の tick を選ぶだけで、逆方向の数値積分は行わない。

physics input 用には現在のレンダー呼出しと独立した **canonical forward source-context graph** を作る。time-independent authored definition と instance context を使い、正の rational affine / monotone piecewise source clock は保持する。要求依存の reverse sampling は含めず、protected loop / hold は dynamics sampling では authoring identity clock に置き換える。固定 source tick の時刻を、この canonical monotone clock の正確な inverse で共有 Property evaluator の時刻へ戻す。

- parent Curve が `x(p)=p`、child affine clock が `q=2p` の場合、source tick `q=2` の入力は canonical parent clock `p=1` で `x=1`。actual reverse playback で `q=2` を表示する要求の parent render 時刻が違っても、physics の入力は変えない。
- protected loop で actual parent 時刻 `p=7` が source `q=2` を表示しても、physics では protected clock を identity に戻した canonical parent `p=2` の入力を使う。初回と後の周回の `q=2` は同じ state を参照する。
- hold は同じ `q` の state を繰り返す。outro への source 時刻の jump では、表示されなかった中間の forward ticks も積分する。
- reverse placement の要求が `q=3,2,1` なら、それぞれの forward state を取得する。`state(3)` を逆向きに積分して `state(2)` を作らない。

Property の Constant / Curve / Expression、parent の input binding、PropertySample の lookback、AudioFeature / DataAsset は、この canonical authoring-clock projection で解決する。通常の appearance animation は actual playback mapping を保持する。同じ `q`、immutable input、instance context であれば、forward / reverse / loop / hold の要求や呼出し順に依存しない physics を返す。

clock の範囲外やモデル自体が未対応の場合は型付き拒否とする。reverse / protected loop / hold を一括して未対応にしたり、現在フレームの値を全過去 tick へベイクしたりする実装にはしない。

## Checkpoint と無効化

checkpoint は simulation algorithm version、固定 clock、seed / instance context、dynamics の transitive immutable input hash と forward tick / state を持つ。入力 closure には Property / Modifier / input binding、Curve、Expression AST、参照される table / audio data の内容と版、canonical clock transform を含める。revision だけや direct Property 値だけを identity にしない。

target tick 以下で最大の、同じ dynamics identity を持つ checkpoint から forward ticks を再計算する。cold start、逐次再生、checkpoint seek、逆順・ランダム順の要求は同じ state / pixels に達する。cache が無効・削除済みでも start から同じ計算を行う。

入力変更では影響する checkpoint を無効化する。変更時刻の独立性を証明できない場合は全該当 state を保守的に無効化する。Source の色・文字等の appearance 変更で physics checkpoint を再利用するのは、dynamics input closure と独立であると証明した場合だけ。cache は bounded / deletable で文書の正本ではない。

分散実行が開始状態を自由に仮定して区間を分割することは許さない。固定 input identity の checkpoint または start からの forward warm-up を必要とする。worker で live project の最新状態を読む方式にはしない。

## 公開境界と検証予定

共有 edit transaction に Simulation の作成・変更・削除を接続し、保存、revision / idempotency、Undo を維持する。snapshot に simulation 意味版を固定し、必要な未知版や budget 超過を型付きエラーにする。純粋 simulation 層へ store / UI / wgpu の型を逆流させない。

受け入れでは、逐次・cold・checkpoint seek、stale checkpoint の拒否、依存入力変更、独立 appearance の再利用、seed / event identity、正の nested / affine / piecewise clocks、reverse / loop / hold、parent Curve / Expression binding、PropertySample / table / audio 入力を比較する。共有 API と実 CLI / MCP、固定 snapshot、CPU / Metal の実描画も確認する。

SIM-001 の依存タスクは受け入れ済みで、本決定に基づくモデル・純粋カーネル・共有レンダーの統合を実装中である。受け入れは未完了であり、検証結果と bounded algorithm / input 型の詳細は [SIM-001 検証記録](../testing/sim-001.md)へ反映する。
