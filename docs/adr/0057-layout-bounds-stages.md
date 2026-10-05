# ADR-0057: bounds の三段階を純粋値と明示した帯追従 policy で共有する

- 状態: 部分置換（[ADR-0067](0067-affine-gaussian-effects.md): effect version 2 の非一様 affine と visual support。version 1 とその他の bounds 規則は維持）
- 日付: 2026-10-04
- 対象: LAYOUT-001

## 背景

M2 の帯は wrap_width × 行数 × line_height の `layout_bounds` に追従する。
短文や空白を tight ink として扱う選択、エフェクトを含む範囲、canvas での三段階の検査が必要になった。
ADR-0043 の純粋評価・単位・依存境界と ADR-0053 の既定帯幅・既存 query 契約は維持し、選択を追加する。
縦横比 variant、式言語、エフェクトの非一様変換の意味はこの決定で拡張しない。

## 決定

- `kronello-model::BoundsStage` は `layout` / `ink` / `visual`。
  `TemplateBandBinding.bounds` を省略すると `layout`。既定値は保存 JSON に出力しないため、既存定義は同じ JSON で往復する。
  未知段階は template 全体の opaque 保持と最終出力の未対応診断を使う。
- `kronello-render::LayoutValue` は `layout_bounds` / `ink_bounds` / `visual_bounds` を持つ純粋な導出値。
  各値は `DesignBounds {min: [x,y], max: [x,y]}` または `None`、単位は `design_px`。
  一つの LayoutValue の全段階は同じ座標空間。Scene IR / query は root Composition 空間、帯の入力は共通の変換親空間とする。
  保存作品状態、バックエンド資源、時計、評価履歴を持たない。
- text の layout / ink は組版意味版 1 の box / glyph outline bounds を使う。Shape の layout は解析幾何 envelope、ink は paint がある幾何と中央線の support。
  Bezier の extrema を使い、一般の線の miter / square cap は保守的に広げる。矩形・楕円は半線幅。
  透明 paint / opacity による縮小や、穴・mask による tight な被覆探索はしない。
  変換した AABB と線の envelope は保守的な包含矩形であり、常に最小矩形とは約束しない。
- visual は変換後 ink と、順序付き blur / shadow の support を合成する。Group / Null / placement は各段階の子の union に自身の effect を適用する。
  renderer と同じ `map_effect` を共有し、sigma・offset を node の world transform で写す。正の Gaussian の非一様変換は既存の `UNSUPPORTED_FEATURE`。
  semantic support は連続 `3 * sigma`、DAG の pixel support は既存の `ceil(3 * sigma)` と shadow 移動の floor / ceil。
  AA・kernel・bilinear tap の外向き丸めは出力格子の処理であり、解像度変更で組版・帯寸法を変えない。
- 帯の対象 text は leaf node に限る。子の合成結果を組版時の字形 bounds へ混ぜず、子を持つ対象は `UNSUPPORTED_FEATURE` で拒否する。帯の対象でない text の子は通常の scene 合成と bounds 集約で扱う。
- 帯は選択した text の stage に padding を加える。layout / ink は text-local bounds を共通親へ写す。
  visual は Composition 空間の visual AABB を親の逆変換で写し、親が特異なら `LAYOUT_SINGULAR_TRANSFORM`。
  空白・空本文で ink / visual が空なら、変換した text 原点に padding だけの帯を作る。layout へ代替しない。
- 上位 compiler が `RuntimePropertyKey::LayoutValue` と `DependencyDeclarations` を生成し、text Property → bounds projection → 帯 size / position と接続する。
  visual の projection は変換親・外側 placement の Property も宣言する。
  `DependencyGraph::dependency_order` の静的順序で projection を供給し、帯の定義順を評価順にしない。
  逆向きの wrap 依存を宣言すると、graph compile が全キーの閉経路を持つ `PROPERTY_DEPENDENCY_CYCLE` を返す。
  現在の公開 API は任意の逆向き依存や式を authoring する入口を追加しない。静的依存宣言は後続の式 compiler の接続点である。
- max_lines 超過の `TEMPLATE_OVERFLOW` は維持する。分割不能な行の幅超過は、template と通常 text の最終 compiler で `LAYOUT_OVERFLOW`。
  node / instance_path / 0 始まり line / advance / wrap_width を診断し、query も同じ失敗を返す。
  `kronello-text::layout` 自体は従来の `LayoutLine.overflow` を保持する。clip / 縮小・成功扱いによる続行はしない。
- `scene.query` の active node の `evaluated.bounds` は三段階を同時に返す。inactive は従来どおり evaluated なし。
  既存 `evaluated.layout_bounds` は text-local のまま維持する。`bounds` は matte 適用前の envelope であり、画素の tight な alpha bounding box ではない。
  `SemanticVersions.bounds = 1` を snapshot に固定し、未知版は拒否。旧 snapshot のフィールド省略は初版 1 として読み、最新への置換には使わない。

## 影響

GUI / CLI / MCP は同じ組版・compiler・Query API の値を使う。eval から text / render / store / service への逆依存を作らない。
既定帯の幅と版固定を保ちつつ、ink / visual を明示指定できる。
三段階の意味的比較、CPU 画素の包含、静的循環、overflow と成果物未公開を通常テストで確認する。
GPU、OS 別実機、responsive variant と glow の実行は今回の実装範囲に含めない。

## 関連

- [ADR-0043](0043-semantic-dependencies-and-units.md)、[ADR-0053](0053-integration-evaluated-queries-and-render-tiles.md)
- [04 ベクター・テキスト・レイアウト](../architecture/04-vector-text-layout.md)
- [07 テンプレート](../architecture/07-templates.md)、[08 API](../architecture/08-api-cli-mcp.md)
- [LAYOUT-001 検証](../testing/layout-001.md)

