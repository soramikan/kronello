# 02 時間の規約

## 基本規約

- 正本の時刻・長さは秒を単位とする正規化された有理数。分母は正、分子・分母の最大公約数は 1、ゼロは `0/1`。負時刻は表現できるが duration は非負。
- 整数演算は checked とし、中間計算は必要に応じて i128 を使う。ゼロ分母・overflow は型付きエラーとする。
- JSON 上の分子・分母は 10 進文字列とし、JavaScript の整数精度の制限を受けない。
- 区間は原則 `[start, end)`。
- フレームレートは 30000/1001 のように正確に保持する。
- 編集用フレームグリッドと評価時刻は別。フレーム間でも Property を評価できる。
- 音声のサンプル位置は絶対時刻から計算し、映像フレームごとの丸め誤差を累積させない。

既存規約と今回固定した正規化・単位の区別、TIME-001 の検証契約は [ADR-0043](../adr/0043-semantic-dependencies-and-units.md)「単位と座標」「後続タスクの検証契約」を参照。非線形 TimeMap の量子化・丸めの詳細は TIME-001 で設計する。

```json
{"time":{"num":"1","den":"60"}}
```

## 時間階層

```text
Sequence time
 -> clip placement offset
 -> clip TimeMap
 -> Composition local time
 -> nested instance TimeMap
 -> animation local time
 -> media PTS / feature-data time
```

基本写像は `local = source_in + time_map(parent_time - placement_start)`。

TimeMap の初版は線形と区分線形。M3 の TEMPLATE-002 は保護した中間区間の hold / loop を追加した。逆再生と汎用の非線形写像は後続範囲とする。
非線形写像は浮動小数点計算や求根を伴うため、完全に有理数だけで解けるとは扱わない。量子化精度・丸め・評価アルゴリズムの版を固定する。

Composition は既定で親の連続時刻で評価する。編集レートが 24fps でも 60fps 出力時に整数の 24fps フレームへ勝手に丸めない。
コマ撮りのように内部レートを保持したい場合だけ、明示的な posterize / hold サンプリングを指定する。
動画素材の保持・補間・オプティカルフローは別の機能であり、ベクターの連続時間評価と混同しない。

## TIME-001 の具体的な境界規約

- `Rational` は正規化された `i64` の分子・分母を持ち、`Time` は秒として解釈する別名とする。`Duration` は非負を検証する専用型。算術は `checked_*` が `Result` を返し、`i128` の中間値を約分した後に `i64` へ変換する。演算子による暗黙の panic・飽和・丸めは提供しない。
- JSON の有理数は `{"num":"1","den":"2"}` とする。文字列は省略可能な `-` と ASCII 数字だけを許し、入力成分は `i64` の範囲内とする。JSON 数値は拒否し、入力の約分・分母の符号・ゼロを正規化する。正規化後に表現できない値はエラー。duration・区間・レート・TimeMap の復元も各型の検証を通す。
- 空区間 `[t, t)` は許し、どの時刻も含まない。接触する区間・空区間の intersection は `None`。adjacency は両区間が非空で端点が一致する場合とする。区間を表現できても差の duration が表現範囲を超える場合は overflow を返す。
- `frame_floor` と `sample_floor` は負時刻にも数学的 floor を適用する。フレーム・サンプルの原点は時刻ゼロ。フレームから時刻への変換は整数フレーム境界を返し、時刻からフレームへの厳密変換はサブフレーム位置を保持する。
- 音声バッチは絶対時刻に対する `floor(start * sample_rate)..floor(end * sample_rate)` とする。隣接フレームは同じ境界を共有し、丸め済みのフレーム長を積算しない。これはバッチ境界の規約であり、各サンプルの時刻が量子化前の区間に含まれるという規約ではない。
- 線形 TimeMap は `local = offset + parent * speed`、`speed > 0` とする。区分線形は厳密増加する親時刻と非減少のローカル時刻を持つ 2 点以上の制御点を必要とし、隣接点の間を有理数で厳密補間する。制御点を含む閉区間を評価 domain とし、端点そのものは保存した値を返す。内部の各有理数演算が表現できなければ overflow、domain 外は型付きエラーとし、外挿・clamp・フレームへの量子化をしない。この domain は配置の半開区間とは区別する。
- TimeMap の JSON は `kind` に `linear` / `piecewise_linear` / `protected` を持つ。`linear` は `offset` / `speed`、`piecewise_linear` は `points` 配列の `parent` / `local` を保存する。TIME-001 時点の enum は将来の拡張を許し、逆再生・loop・停止・非線形を表す variant は提供しなかった。負の傾きは拒否する。浮動小数点時刻で代替しない。

### TEMPLATE-002 の保護中間 map

TimeMap::Protected（kind: protected）は authoring / requested / intro / outro の Duration と
mode: hold | loop を保存する。中間長は正、map の domain は [0,requested]。
intro / outro は傾き 1、中間の hold は intro 時刻の pose、
loop は authoring-intro-outro を周期とする checked 有理数剰余へ写す。
outro 開始は authoring-outro へ明示的に切り替える。
PiecewiseLinear の正傾斜と既存 stretch の保存・意味は変更しない。
汎用 Clip の Protected map の trim / stretch / lowering は型付き未対応。
template_instance.retime は選択 variant の authoring 尺と公開 duration policy を使う。
詳細は [ADR-0059](../adr/0059-template-duration-variants-and-migration.md) と
[07 テンプレート](07-templates.md)。

### NLE-006 のホールド（freeze）区間と速度ランプ

[ADR-0112](../adr/0112-variable-retime-and-freeze-hold.md) で
`PiecewiseTimeMap` の local 制約を非減少に緩和した。`local[i] == local[i+1]`
の区間は hold（freeze）区間で、`map` はその区間で `local[i]` を返す。
逆単調は引き続き `UnsupportedMapSlope` で拒否するため、既存の厳密単調な
ドキュメントはそのまま読める。`PiecewiseTimeMap::is_hold` が parent の所属
区間を、`slope_at` が区間の有理数速度を返す。
`inverse_canonical` は非単射となるため、hold 区間の local 値にはその区間を
開始する parent（その local に写る最も早い parent）を返す決定的規則とする。
音声はソース時刻が進まない hold 区間を `ResampleV1` /
`ReverseResampleV1` でも無音として処理し、ランプ区間は各区間の線形速度で
逐次リサンプルする。`Reject` ポリシは非単位リタイムを計画時に拒否する。
速度ランプは `clip_time_set` が piecewise map 全体を受け取り、
`clip_freeze { sequence, clip, at }` がクリップを `at` で分割して右側に
hold map を持つクリップを生成する。検証は [NLE-006](../testing/nle-006.md)
を参照。

## 純粋評価

通常のアニメーションは任意時刻の純粋評価とする（[ADR-0003](../adr/0003-pure-evaluation-at-arbitrary-time.md)）。同じスナップショット・時刻・インスタンスに対する評価結果は、要求順（順方向・逆順・ランダム）に依存しない。状態を必要とする表現は Simulation（[06 拡張点](06-extensions.md)）へ分離する。

## トリムと長さ変更

`clip.trim`、`clip.stretch`、`template_instance.retime` は別操作とする。
尺の変更によって、保護されたイントロ・アウトロを黙って伸縮しない。
音声のリタイム方針も別に宣言する。複雑な非単調 TimeMap に対する音声処理が未対応なら検証で拒否する。

NLE-001 で三操作を実装した。trim は元区間の非空部分だけを残し、source_in / map 原点を移して同じ絶対時刻の source 内容と速度を保つ。stretch は source span と map の local 値を保ち、親時間を `new_duration / old_duration` 倍する。instance retime は内部 CompositionInstance の map だけを置換し、Clip の配置は変えない。template_instance.retime は TEMPLATE-001 の保護 intro / outro を保持する duration policy を再利用する。すべて有理数で処理し、map の両端 domain・source duration・overlap を適用前に検証する。音声は unity-speed の線形 map のみ対応し、速度変更は `UNSUPPORTED_FEATURE`。詳細は [ADR-0051](../adr/0051-nle-placement-and-retime.md)。

## AUDIO-000 の sample grid

実装した `kronello-audio::sample_index` / `sample_range` は TIME-001 の SampleRate を再利用する。sample 境界は絶対 rational 時刻の数学的 floor、整数中間演算は checked i128。24 / 30000/1001 / 60000/1001 fps、非整数 sample 境界、負時刻、長尺と分割要求を検証する。最終 A/V export は frame に整列した絶対 range を要求し、音声の floor 境界と映像の rational duration の差を 1 sample 未満として報告する。両 stream はファイル上で PTS 0 に揃える。source trim と codec の決定は [ADR-0049](../adr/0049-audio-bus-timing-and-codec.md)、手順は [AUDIO-000](../testing/audio-000.md)。
