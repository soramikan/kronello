# ADR-0023: 名称を Cinewright に変更する

- 状態: 置換（[ADR-0042](0042-naming-kronello.md)）
- 日付: 2026-10-02
- 置換対象: [ADR-0013](0013-naming-koma.md)

## 背景

ADR-0013 で名称を `koma` としたが、衝突調査（[naming.md](../naming.md)）で次が分かった。

- `koma` という実行ファイル名の CLI ツールが既に複数ある（Rust 製の AI コーディングエージェント、Go 製のマンガダウンローダー）。
- 同じ分野に、日本で普及しているコマ撮りアニメーションアプリ「KOMA KOMA」と、モーション付きプレゼン制作アプリ「Koma Motion」がある。
- npm と PyPI の `koma` は別のパッケージが取得済み。

## 決定

- プロダクト名は Cinewright（cine + wright。playwright、shipwright と同じ造語）。
- CLI の実行ファイルは `cinewright`、crate の接頭辞は `cinewright-`。
- プロジェクトファイルの拡張子は `.cinewright`。短縮形 `.cwr` は SAP Crystal Reports と SolidWorks Simulation が使っているため採らない。
- `koma`、`ved` という表記は新しく書かない（`docs/archive/`、ADR-0013、`naming.md` を除く）。
- GitHub リポジトリは `soramikan/cinewright` とする。

## 影響

- 調査時点で crates.io、npm、PyPI、Homebrew に `cinewright` はなく、GitHub と Web 検索でも同名の製品・リポジトリは見つからなかった。
- 商標データベース（J-PlatPat、USPTO、EUIPO）は未照会。public にする前に確認する（OQ-02）。
- crates.io の名前は先着順のため、公開の方針が固まった時点で早めに確保する（OQ-02）。
- 名前が長いので crate 名も長くなる（`cinewright-framebridge` など）。CLI は主にスクリプトとエージェントが呼ぶため、長さの実害は小さいと判断した。

## 関連

- [名称の衝突調査](../naming.md)
- [11 ワークスペース](../architecture/11-workspace.md)
- [未決事項](../open-questions.md)
