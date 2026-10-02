# 名称の衝突調査

調査日: 2026-10-02。[ADR-0023](adr/0023-naming-cinewright.md) で名称を Cinewright に変更したが、その後の追加調査を受けて再検討中（[OQ-02](open-questions.md)）。

調べた範囲は crates.io、npm、PyPI、Homebrew（formula / cask）、GitHub のリポジトリ検索、Web 検索。
**商標データベース（J-PlatPat、USPTO、EUIPO）は照会していない。** public にする前に確認する（[OQ-02](open-questions.md)）。

## `koma`（撤回）

| 対象 | 結果 |
|---|---|
| crates.io | `koma`、`koma-cli`、`koma-model`、`koma-time`、`koma-core` はいずれも未登録 |
| Homebrew | なし |
| npm | 取得済み（別物の Web フレームワーク系モジュール、2022 年以降更新なし） |
| PyPI | 取得済み（別物、macOS 関連の AI 向けパッケージ） |

同名・類似名のソフトウェア:

| 名前 | 内容 | 衝突の種類 |
|---|---|---|
| [aula-id/koma](https://github.com/aula-id/koma)（[koma.run](https://koma.run/)） | Rust 製の AI コーディングエージェント。2026-06 開始、★60、活発 | 実行ファイル名 `koma` が同じ |
| [yukiteruamano/koma](https://github.com/yukiteruamano/koma) | Go 製の CLI マンガダウンローダー | 実行ファイル名 `koma` が同じ |
| [KOMA KOMA](https://komakoma.org/) | コマ撮りアニメーションアプリ。2010 年から。iPad / Windows / Mac / Web（[KOMA KOMA×日文](https://www21.nichibun-g.co.jp/komakoma/)）。教育現場で普及 | 同じ分野・日本市場での混同 |
| [Dytschgo/koma-motion](https://github.com/Dytschgo/koma-motion) | 「Koma Motion」。AI でスライドとモーションを作るデスクトップアプリ。2026-09-29 作成、★0 | 同じ分野 |
| KOMA-Script | 著名な LaTeX 文書クラス | 検索での埋没 |
| [kyonifer/koma](https://github.com/kyonifer/koma)、[KomaMRI.jl](https://github.com/JuliaHealth/KomaMRI.jl) | Kotlin の科学計算ライブラリ、MRI シミュレーター | 検索での埋没 |

実行ファイル名の衝突と、同じ分野の既存製品との混同の両方があるため撤回した。

## `cinewright`（採用後に再検討）

採用時の検索は他の候補名と OR でまとめていたため、同名の事業者を見落とした。単独で検索し直して見つかったため、名称を再検討することにした（[OQ-02](open-questions.md)）。以後の調査では、候補ごとに単独で Web 検索し、主要ドメインも確認する。


| 対象 | 結果 |
|---|---|
| crates.io | 未登録 |
| npm | 未登録 |
| PyPI | 未登録 |
| Homebrew | formula / cask ともなし |
| GitHub | 同名のリポジトリは検索に現れず |
| Web 検索（採用時） | 他の候補とまとめた検索では同名の製品・サービスが見つからなかった |
| Web 検索（採用後、単独で再検索） | **同名の事業者あり。** 米国メイン州の [Cinewright](https://cinewright.com/who-we-are/)（cinewright.com）。ドキュメンタリー映画制作のワークショップを運営。ソフトウェアは提供していない。サイト上に ™ / ® の表記は見当たらないが、商標登録の有無は未確認 |

拡張子:

| 候補 | 結果 |
|---|---|
| `.cwr` | 不採用。SAP Crystal Reports のレポート、SolidWorks Simulation の結果ファイルなどが使用 |
| `.cinewright` | 採用 |

## 検討した他の候補

いずれも各レジストリの登録状況と GitHub の上位リポジトリを確認した。

| 候補 | 状況 |
|---|---|
| Hakobi、Komaori、Tokiori | 全レジストリ空き。和風の名前は見送り |
| Tweenery | 全レジストリ空き、同名の製品なし。モーション寄りでカット編集の印象が薄い |
| Multiplane | 全レジストリ空き。OpenToonz、TVPaint、Toon Boom などが機能名として使う一般語 |
| Pegbar | 全レジストリ空き。同名の手描きアニメ OSS と、同名のアニメスタジオが存在 |
| Intercut | レジストリは空きだが、同名の動画関連製品が複数存在（intercut.ai など） |
| Timewright、Cutwright、Smashcut、Mutoscope、Xsheet、Dopesheet、Trimbin | レジストリは空き。Web での詳細調査は未実施 |
| Flatbed、Sprocket、Splicer、Interlock、Conform、Armature、Motus、Tessera、Kinema、Subframe、Rostrum | crates.io ほかで取得済み |

## 出典

- [KOMA KOMA LAB](https://komakoma.org/)
- [KOMA KOMA×日文（日本文教出版）](https://www21.nichibun-g.co.jp/komakoma/)
- [KOMA KOMA for iPad（App Store）](https://apps.apple.com/us/app/koma-koma-for-ipad/id635794784)
- [Dytschgo/koma-motion](https://github.com/Dytschgo/koma-motion)
- [koma.run](https://koma.run/)
- [yukiteruamano/koma（pkg.go.dev）](https://pkg.go.dev/github.com/yukiteruamano/koma)
- [CWR File Extension（filext.com）](https://filext.com/file-extension/CWR)
- [intercut.ai](https://www.intercut.ai/)
- [danallison/PEGBAR](https://github.com/danallison/PEGBAR)
