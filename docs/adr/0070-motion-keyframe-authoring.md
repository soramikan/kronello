# ADR-0070: Dope sheet と時間イージングの編集

- 状態: 採用
- 日付: 2026-10-05

## 背景

GUI-002 は GUI-001 の共有 Command / Query、候補表示、ADR-0061 のセッション Undo をキー編集へ接続する。
画面・部品は ADR-0054 / 0055 と design-system の CurveEditor、Keyframe、InspectorRow、motion、states に従う。
監督が GUI-002 の以下の判断を確定した。API を追加せず、作品モデルの意味も変更しない。

## 決定

- キーの選択は CurveId と正確な有理数時刻で参照する。最後のキーを切り離す対象には、選択したレーンの stable PropertyId を UI の文脈として保持する。クリックは単独選択、Shift は既存キーの選択を拡張、Command は追加 / 解除、空白クリックは解除、ドラッグの矩形で範囲選択する。Keyframe.md の操作規則もこの判断に合わせて更新した。キーの追加は Inspector / Dope sheet のナビゲータ diamond だけから行う。
- 時刻の移動は edit rate のフレームへスナップし、有理数として確定する。スナップ有効時は playhead / 他キーも候補にする。ドラッグ中は位置の候補だけを表示し、release で一回の plan / apply。複数キーの元時刻をすべて remove してから insert する一つの transaction とし、移動先の非選択キーとの重複は型付きエラーで全体を拒否する。
- Linear / Cubic / Hold は既存 `keyframe_replace` で変更する。Cubic の `TimeBezier` は次キーまでの区間の一組であり、Vec2 の X / Y は共通の時間イージングを使う。片方のチャンネルでの接線編集が両方に作用することを Curve editor の `ink-muted` の常設注記で示す。
- 揃える / 分けるは永続化しない UI 補助。操作中チャンネルの時間・値グラフで、選択キーの左右の傾きが一致するかを導出する。揃えるでは左右の隣接区間を二つの `keyframe_replace` として一つの transaction で変える。分けるでは片側だけ。傾きは区間時間と値差を含めて計算する。時間ハンドルの `0 <= x1 <= x2 <= 1` を保ち、垂直または値差ゼロで揃えられない場合は `INVALID_EDIT`。
- 最後のキーを削除するときは、同じ transaction で編集した Property だけを `property_source_set` の Constant に戻す。値は削除時刻の共有 `property.sample` の評価値を使う。別の Property owner または Expression の `CurveSample` が参照していれば、Curve とキーを変更せず保持する。他の Property の Source は変えない。編集した Property が単独の消費者だった場合だけキーを remove する。評価できない場合は適用しない。一回の Undo で元の Source（単独消費者の場合はキーも）を復元する。
- 空間パスは Viewer にだけ表示する。Composition の各フレーム時刻と正確なキー時刻を、一回の共有 `property.sample` で選択ノードの Position として評価する。600 点を超えるフレームは先頭・最後の有効 frame を含めて等間隔に間引く。Position は親空間の軌跡であり、選択ノードの local transform を Anchor に適用した位置と一致するため Anchor の追加評価は不要。描画時は現在の playhead の共有 scene にある親ノードの `world_transform`（一つの2×3 matrix）を適用する。祖先がアニメーションする場合も、軌跡は playhead の親空間に対する局所移動として表示する。Swift で transform の合成を再実装しない。revision / 選択 / Composition の変更で軌跡を再評価し、playhead の変更は親 matrix が変わったときだけ cached points を再配置する。`scene.query` を各時刻に発行しない。active range / Composition 外のキーも主値源の Property として評価するため、render-consistent sampling の `fonts` は指定しない。欠落ノード / 親 matrix / sample は黙って空パスにせず既存 `KRErrorLine` に型付きエラーを示す。線は `selection` の1px、キー位置は5pxの正方形。GUI-002 では読取り専用。Curve editor は数値の主値源 Curve の時間イージングだけを表示・編集する。Expression は「式のため編集不可」とし曲線を表示しない。速度グラフは表示のみで、編集コマンドを発行しない。
- Dope sheet と Curve editor は下段の segmented で切り替え、時間表示範囲・拡大率・キー選択を共有する。チャンネル列は216px。Property の名前・単位・倍率は Inspector と同じ `PropertyPresentation`。曲線は操作中が `ink` の1.5px実線、他は `ink-muted` の1px破線と名前で識別する。
- 色は琥珀が「今」、青が選択 / フォーカス、赤が型付きエラー。キー選択と現在時刻上のキーの有無を混同しない。再生ヘッドはルーラーからレーンを貫く一本、つまみはルーラーだけ。左右の `space-2` の余白とラベルのない末尾目盛りを保つ。アニメーションしない設定は `KRInspectorSettingRow` を使う。
- Curve editor の playhead readout は現在値または現在速度（Property の単位/秒）を小数1桁で示す。速度が表示のみである説明は共有 easing 注記と同じ footer に `ink-muted` で常設する。軸ラベルの実測幅を readout の左余白として予約し、右端では readout を playhead の左へ反転する。focus ring は focus を実際に保持する control / container の `FocusState` だけで決め、祖先の `Environment.isFocused` を子へ継承しない。container の focus は一つの inset ring で示す。
- GUI-001 の `EditorModel.apply` と Undo を再利用する。`REVISION_CONFLICT` は release 時の batch を保持し、既存バナーで破棄 / 再適用を選ぶ。`UNDO_CONFLICT` は部分的に戻さず既存 Dialog で示す。過去 / 外部セッションの Event を Undo stack に追加しない。

## 影響と検証

新しいコア API / 永続フィールド / 補間意味版は不要。選択、接線モード、表示・ドラッグ候補は UI のみ。
数値 Curve の描画用サンプルは Swift の presentation geometry であり、作品への適用・最後のキーの値・空間パスの評価は共有 Rust サービスを使う。
実装検査とホストでの SwiftPM / Metal / 画面受け入れを区別する。受け入れ条件と手順は [GUI-002 の検証](../testing/gui-002.md)。
