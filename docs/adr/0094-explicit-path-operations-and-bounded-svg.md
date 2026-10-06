# ADR-0094: 明示した Path 操作と有界 SVG 交換

状態: 採用（設計契約の採用。workspace 統合・VEC-002 の受け入れは検証記録と backlog を別に参照）
日付: 2026-10-06
対象: VEC-002

## 背景

Trim path と morph は通常の Property 評価を通し、SVG は外部取得や黙示的な機能省略を避ける必要がある。

## 契約

- `ShapeGeometry::MorphPath { from, to, progress }` はノード所有の Path / Path / Scalar Property を参照する。progress は `[0,1]`。target Path には `kronello.shape.morph_target` descriptor を用意し、ノードの singleton `kronello.shape.path` と独立に共通編集できる。Path の command 数と保存順の command kind を一致させ、endpoint と control point を同じ順序で線形補間する。MoveTo / Close の対応も検査する。自動 remeshing・始点選択・向きの補正は行わない。不一致は `ShapeError::MorphCorrespondence { index }`、共有 service / render のコードは `PATH_MORPH_CORRESPONDENCE`。
- `ShapeGeometry::TrimmedPath { path, start, end, offset }` は Path と Scalar Property を参照する。start / end は `[0,1]`、`start <= end`、offset は有限の周期 fraction。既存の request tolerance で flatten したローカル `design_px` の全 contour を保存順に連結した弧長を使う。部分選択の contour は open、全長選択は元の closure、start と end が同じなら空。offset は `rem_euclid(1)` で wrap する。異なる contour は接続しない。単一の closed contour で offset が seam を跨ぐ場合は同じ partial open contour の中へ seam を結合し、seam に cap を追加しない。逆転は `PATH_TRIM_RANGE`。長さ計算前の flatten budget と trim edge 65,536 の上限を守る。
- 通常の `ShapeSet` / `NodePropertyInsert` / Property / Curve 編集と同じ保存・revision・undo 経路を使う。morph は評価した `BezierPath`、trim は `ResolvedGeometry::TrimmedPath` として renderer へ渡す。geometry cache は trim の Path と start / end / offset および `vec002-trim-v1` を含む。bounds は元 Path の保守的 envelope を維持し、trim 被覆を tight bounds と主張しない。
- snapshot は新しい `path_operations: Some(1)` を固定する。省略した既存 snapshot は拡張 Path 操作のない文書だけに許可し、未知版・必要な pin の欠落を拒否する。既存 `vector` flatten 版は維持する。

## SVG の対応表

| 入力 | 扱い |
|---|---|
| `svg`、直接の `path` | 対応 |
| Path の M / L / H / V / Q / T / C / S / A / Z | kurbo SVG parser によって局所 Path へ変換。arc は cubic になる |
| solid `black` / `#RGB` / `#RRGGBB` / `none` | 対応 |
| `fill-rule` nonzero / evenodd | 対応 |
| `id` | 描画意味を持たない属性として許可。保存 ID は呼出側の明示 ID |
| root `xmlns`、正の有限 unitless width / height | 許可。viewport の再写像は行わない |
| viewBox / transform / group / CSS / stroke / gradient / opacity / text / image / filter / clipPath / use 等 | `unsupported` に列挙、import plan は拒否 |
| href / xlink:href / `url(...)` | `external_references` に列挙、import plan は拒否。内部 fragment 参照もこの subset では拒否 |
| script / event attribute / XML entities / DTD / processing instruction | `unsupported` として拒否。取得・展開・実行はしない |
| export の HDR / 非 sRGB / 非8bit表現色 / alpha paint | `UNSUPPORTED_FEATURE`。黙示変換しない |

入力は 1 MiB、Path は 4,096、segment 合計は 65,536、XML 深さは64、属性は256を上限とする。XML は引用符付き属性・厳密な閉じタグを持つ限定 parser で、汎用 SVG/XML 互換を保証しない。コメントは無視する。DTD 等を検出した場合の report はその時点までであり、後続参照の全列挙は保証しない。

共有 `svg.inspect` は対応 report、`svg.export` はこの subset の `SvgPath` の SVG 文字列を返す。`svg.import_plan` は呼出側の Composition・Node・Content・Property ID と配置順から通常の `NodeAdd` / `ShapeSet` の `EditPlan` を作る。書込は `edit.apply` を経由する。外部参照・未対応機能を含む SVG は report を error details に含めて計画を拒否する。

## 検証と残件

[検証記録](../testing/vec-002.md) を正本とする。SVG 全互換、viewport 変換、ブラウザーとの差異、GUI 編集・GPU golden はこの変更から保証しない。
