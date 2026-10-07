# 現在の実装範囲と残件

確認日: 2026-10-07。受け入れ済み実装の基点はPR #8のマージcommit `0c0822c639c165596b6244fe9ffbfec690cfb72a`、文書監査の基点はPR #9（`adc84d3`）。参照会話「M3残タスク完了」の調査を手がかりに、現行コード・採用ADR・バックログ・実行ログを照合した記録である。会話内の推測を受け入れ証拠や設計決定として扱わない。以下のM5進捗は `codex/m5-completion` の未コミット作業ツリーを含み、mainの保証範囲へ加算しない。

正本は [backlog.json](../backlog/backlog.json)、その生成一覧は [BACKLOG.md](../backlog/BACKLOG.md)。本書は保証範囲と追跡先の索引であり、今後のstatus変更では正本を更新する。M0・M1・M2のP0・M3・M4の完了は、それぞれの受け入れ範囲の完了を意味し、全設計機能・全OS GUI・一般向け配布の完了を意味しない。

## 確認時点の集計

PR #9の監査では既存77件のstatusを維持し、未割当だった16件をplannedとして追加した。その後のM5着手を反映した正本は、合計140件、done 79 / in_progress 2 / planned 59。M5は13件中12件がdone、1件（NAME-001）がin_progress。旧M6はM11へ改番し、M7〜M10の機能拡張47件を新たにplannedとして追加した。M7〜M11はすべて未着手である。M5以降の配置・優先度は開発計画であり、納期の約束や未決事項の採用判断ではない。

参照会話の「77件・未完了10件」はPR #9より前の集計であり、現在の件数として転記しない。比較する対象を次のように固定する。

| 対象 | 総数 | done | in_progress | planned |
|---|---:|---:|---:|---:|
| 文書監査の統合基点 `adc84d3`（PR #9） | 93 | 67 | 1 | 25 |
| M5の未コミット作業ツリー（本書の確認時点） | 93 | 79 | 2 | 12 |

基点の件数は `git show adc84d3:docs/backlog/backlog.json`、作業ツリーの件数は現在のJSONから再計算できる。後者のdoneは記録した受け入れ時点の判定であり、変更中の全ファイルや未実行のCIまで検証済みという意味ではない。

- **実装済み・確認済み:** M3全25件、M4全12件。[M3記録](../testing/m3-acceptance.md)、[M4記録](../testing/m4-acceptance.md)。修正head `1539223` の [CI run 37426096876](https://github.com/soramikan/kronello/actions/runs/37426096876) は全5jobs成功。PR #8は2026-10-06にmainへマージ済み。
- **実装・手順はあるが環境確認待ち:** STORE-003。Dropbox管理フォルダと実network FSの残件があるためin_progress。
- **未実装:** 以下のplannedタスク。型・schema・設計案の存在だけでは実装済みとしない。
- **未決:** OQ-02（名称の採否・確保の所有者判断のみ残る。OQ-17 は ADR-0105、OQ-21 は ADR-0106 で解決済み）。決定を要するタスクは依存タスクがdoneでも、採否の確定前に実装へ進めない。
- **保証外・意図した制約:** 後述の資源上限、GUI表示込みFPS、software adapterとhardwareの区別。未実装の機能と同一視しない。

M5作業ツリーではVEC-002・TEXT-002・AUDIO-001・MATTE-001・INSPECT-002・FRAMEBRIDGE-001・REPEAT-001・EXPR-003・GUI-007・SIM-001・EXPR-002・AUDIO-005の固有受け入れ条件と統合checkpointを確認し、12件をdoneにした（[M5進捗記録](../testing/m5-acceptance.md)）。EXPR-002はADR-0105、AUDIO-005はADR-0106の採用決定を経て実装・受け入れた。最終freezeは841 passed / 0 failed / 41 ignoredで、fmt / Clippy / 公開schema / 生成Swift照合・Swift test・native GUI確認も成功。mainへのマージやM5全13件の完了とは扱わない。NAME-001は所有者判断が保留のためin_progressのままである。

先行workspaceの旧Matte期待値とschema不一致は修正・検証済み。別の実行は並行する通常CLIビルドがテスト用CLIを上書きしてジョブ試験を壊したが、同一出力先のビルドを重ねずに全workspaceを成功させた。各時点の失敗・成功ログはM5進捗記録に対応付ける。後続実装とM5最終変更の各OS CI・GUI検証は継続する。

805件の統合checkpointはSIM-001統合前の結果であり、最終sourceでのfreezeは841 passed / 0 failed / 41 ignored（128 suites）を再確認した（[M5進捗記録](../testing/m5-acceptance.md)）。固定CLI/MCPでの中間検証・Metal比較・native GUI確認は[SIM-001記録](../testing/sim-001.md)へ分ける。追加したWindows M5 gateも、workflowに存在することと実際のCI成功を区別する。

## STORE-003 の実環境残件

[検証記録](../testing/store-003.md) に、適応的snapshotの既定不採用（ADR-0052）、2026-10-04のiCloud Drive、2026-10-06のLinux/Windows CIの競合・WAL/DELETE journal強制終了回復を記録した。各OSのstorageテストは35件成功。古い「Linux/Windows未確認」という記述は解消した。

残るのは**実Dropbox管理フォルダと実ネットワークマウント**でのAuto判定、安全モード、同一ホスト内の別プロセス排他、close後の再open・清掃である。通常のローカルCIや似た名前のフォルダでは代替できない。異なるホスト間の同時編集・同期完了・オフライン時の整合性まで保証する試験ではない。

## 当初の未完了タスクと現在の状態

| タスク | 残る範囲・確認先 | 依存上の状態 |
|---|---|---|
| VEC-002 | Trim Path・morph・限定SVG adapterの受け入れを確認。[検証記録](../testing/vec-002.md) | done（M5作業ツリー） |
| REPEAT-001 | source共有、instance ID/seed、個別制御と明示expand。[06](../architecture/06-extensions.md) | done（[受け入れ記録](../testing/repeat-001.md)） |
| TEXT-002 | layout版1は従来の未対応拒否を維持。版2の縦書き・ルビ・文字selectorとreflowの受け入れを確認。[検証記録](../testing/text-002.md) | done（M5作業ツリー） |
| AUDIO-001 | 不変の特徴量・共有audio.analyze・式参照と再送契約の受け入れを確認。[検証記録](../testing/audio-001.md) | done（M5作業ツリー） |
| SIM-001 | 固定刻みsimulation/particle、checkpoint seekと入力変更時の無効化。[検証記録](../testing/sim-001.md) | done（M5作業ツリー） |
| DIM-001 | Scene3D、camera、2D順序と3D depthの分離、補助port。[06](../architecture/06-extensions.md) | 依存完了 |
| INTEROP-001 | OTIO/SVG/外部render交換、保持・変換・欠落レポートと明示bake。[06](../architecture/06-extensions.md) | 依存完了 |
| PLUGIN-001 | WASM runtime、fuel/memory/host call/権限と信頼区分。[06](../architecture/06-extensions.md) | 依存完了 |
| DIST-001 | remote worker、素材・font・engine版の一致、simulation bake境界。[14](../architecture/14-jobs.md) | SIM-001待ち |

## 今回追加した追跡タスク

詳しい依存・受け入れ条件は生成バックログを参照する。既存doneタスクの受け入れ範囲を後から拡大して取り消すのではなく、未保証の範囲を分離する。

| タスク | 配置 | 現状・追加理由 | 根拠 |
|---|---|---|---|
| NAME-001 | M5 / P1 | J-PlatPat・USPTO・EUIPOの限定検索とcrate/domainの登録照会を記録済み。所有者の採用・確保判断と取得は未完了 | [OQ-02](../open-questions.md#oq-02-商標の確認と名前の確保)、[名称調査](../naming.md) |
| EXPR-002 | M5 / P1 | ADR-0105の構文parser/formatter、`property_expression_text_set`、GUI式入力（診断・Undo・detach・IME）を実装・受け入れ済み（[記録](../testing/expr-002.md)） | [ADR-0105](../adr/0105-human-readable-expression-syntax.md)、[ADR-0058](../adr/0058-bounded-canonical-expression-ast.md) |
| EXPR-003 | M5 / P2 | 固定DataAsset参照、動的な過去Property sample、連続補間noiseを受け入れ済み。静的依存・循環拒否・共有予算・版pinと実CLI/MCP・描画を検証した。人間向け構文はEXPR-002の別範囲 | [検証記録](../testing/expr-003.md)、[ADR-0102](../adr/0102-bounded-temporal-expression-assets.md) |
| AUDIO-005 | M5 / P1 | ADR-0106でAAC-LC/Opusを採用し、4 profile・厳密長/bounded error・libopus配布構成を受け入れ済み（[記録](../testing/audio-005.md)） | [ADR-0106](../adr/0106-versioned-compressed-delivery-audio.md)、[ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md) |
| MATTE-001 | M5 / P1 | Project.mattes・共有編集/Undo・CPU/GPU合成・Inspectorを実装。個別試験、GUI直接操作、CLI/MCPと同梱FFIの比較は確認済み。統合checkpointを含め受け入れ済み（[記録](../testing/matte-001.md)） | [snapshot.rs](../../crates/kronello-render/src/snapshot.rs)、[05](../architecture/05-render-gpu.md) |
| INSPECT-002 | M5 / P1 | 単一graph・temporal/tile計画に更新し、M4/Metalでactual countersとの個別比較が成功。統合checkpointを含め受け入れ済み（[記録](../testing/inspect-002.md)） | [inspect.rs](../../crates/kronello-render/src/inspect.rs)、[ADR-0092](../adr/0092-single-graph-gpu-final-output-and-observations.md) |
| FRAMEBRIDGE-001 | M5 / P2 | generic selectorを互換用の型付き拒否として明確化。具体8経路の一覧とBGRA/NV12の実機試験を追加し、統合checkpointを含め受け入れ済み（[記録](../testing/framebridge-001.md)） | [FrameBridge](../../crates/kronello-framebridge/src/lib.rs)、[GPU-003](../testing/gpu-003.md) |
| GUI-007 | M5 / P2 | macOS編集操作を受け入れ済み。共有API同等性、Undo・外部競合・IMEと直接GUIの証拠を[検証記録](../testing/gui-007.md)で分ける | 下表と [10 GUI](../architecture/10-desktop-gui.md) |
| GUI-005 / GUI-006 | M11 / P2 | Windows WinUI 3 / Linux GTK4アプリ、native preview surfaceとOS固有操作は未実装・未検証 | [ADR-0032](../adr/0032-windows-winui-linux-gtk.md)、[10 GUI](../architecture/10-desktop-gui.md) |
| RELEASE-002 | M11 / P1 | macOS Developer ID、notarization、staple、Gatekeeper、quarantineを保持した実ダウンロード | [RELEASE-001](../testing/release-001.md)。開発署名の受け入れと分離 |
| RELEASE-003 / RELEASE-004 | M11 / P2 | Windows/LinuxのGUIを含む製品packageとクリーン環境での配布確認 | [12](../architecture/12-platform-dependencies.md)。CLIビルド成功と分離 |
| GPU-004 / GPU-005 | M11 / P2 | Windows/Linuxのhardware resident media/render経路 | [GPU-003](../testing/gpu-003.md)。software decode/goldenの成功と分離 |
| GPU-006 | M11 / P2 | macOS residentのHDR/10-bit/full-range/HEVC hev1等を形式別に評価・昇格 | [GPU-003](../testing/gpu-003.md)。software HDR出力や8-bit SDR保証と分離 |

### macOS GUI で残る明示的な制限

次表はPR #9監査で検出した操作の実装先を示す。GUI-007は2026-10-06に全3条件を受け入れた。Motion guide/snap、色・固定font・複数style span、Edit操作、Normal / Multiply / Screen、明示reverse samplingを共有APIへ接続し、111 CLI/MCP checks、実worker回帰、host IMEとroot直接GUIで確認した（[検証記録](../testing/gui-007.md)、[blend記録](../testing/gui-007-blend.md)）。Option+dragの直接操作はCUA API制約により同一snap:false経路の試験で補完する。

直接GUI確認で見つけたTEXT-002のルビ重なりは修正済みで、再構築したアプリの表示、親文字とルビの同時アニメーション、折り返し後の対応維持まで確認した（[記録](../testing/text-002.md)）。MATTE-001は反転・luminance・Undo・MCP外部編集のGUI反映を確認し、CLI/MCPの全76,800画素、同梱FFI/CLIの全19,200画素がそれぞれ一致した（[条件と証拠](../testing/matte-001.md)）。これはOS compositor後の画素一致やM5全体の受け入れを意味しない。

GUI-007ではx=320のガイド追加と作品revision不変を直接確認した。一方、文字範囲1..2だけの色変更で `INVALID_EDIT: property key already exists on node` を検出した。範囲別書式の共有APIを修正し、色・サイズ変更、非選択span保持、Undoの個別回帰試験は成功した。修正ビルドでも2文字だけの色・サイズ変更とUndoを直接確認した（[記録](../testing/gui-007.md)）。ビルド成功やAXの操作成功だけで画素・編集結果の受け入れを代替しない。

| GUI-007の受け入れ済み操作 | コードの根拠 |
|---|---|
| EditのEffects追加、手のひら操作、速度/ソース開始/逆再生、クリップ不透明度/合成設定の編集 | [EditPage.swift](../../apps/macos/Sources/Kronello/EditPage.swift) |
| トラックの表示・ミュート操作 | [Track.swift](../../apps/macos/Sources/KronelloDesign/Components/Track.swift)、[EditPage.swift](../../apps/macos/Sources/Kronello/EditPage.swift) |
| Motionのガイドとガイドへのスナップ | [MotionViewer.swift](../../apps/macos/Sources/Kronello/MotionViewer.swift) |
| 色、書体・ウェイト変更、複数Text style spanの編集 | [PropertyFields.swift](../../apps/macos/Sources/Kronello/PropertyFields.swift)、[InspectorPanel.swift](../../apps/macos/Sources/Kronello/InspectorPanel.swift)、[Authoring.swift](../../apps/macos/Sources/KronelloAppModel/Authoring.swift) |

## 保証外と性能制約

| 境界 | 現在の扱い |
|---|---|
| 基本4K native preview | M4 Mac mini 32GB・既定cache・warm p95 ≤33.3msの承認済み基準。実測22.139ms。cold p95 34.166msは別報告 |
| complex lower-third whole-graph native preview | 1080p/4Kとも既存512MiB admissionで型付き拒否。proxy previewとtile化finalを区別する |
| whole-region 4K temporal | 既存budget超過で拒否。1080pで検証した時間積分の精度を任意の解像度での実行保証に広げない |
| GUI表示・動画decode込みFPS | 上記native previewの測定対象外。全作品の4K30fps保証はない |
| 他OSのGPU | Vulkan/DX12のsoftware adapterでのgolden成功。実hardware resident保証はGPU-004/005の別課題 |
| 完全な3D・既存外部プラグインの完全互換 | DIM-001の2.5D、PLUGIN-001の有界WASMの受け入れ範囲に含まれない。現時点で実装・納期を約束しない |

性能の数値・条件・比較対象は [M4受け入れ](../testing/m4-acceptance.md)、[PERF-001](../testing/perf-001.md)、[ADR-0093](../adr/0093-m4-reference-preview-performance-target.md) が正本。現在の制約は黙って劣化させない設計に基づく。全作品FPSの改善など未合意の目標を、今回の文書整理だけで追加の完了条件にはしない。

## 未決事項と記録の保守

- 圧縮音声は**OQ-21**へ再採番した。歴史的な**OQ-19**はイベントと逆操作情報の保持期間（ADR-0030）であり、解決済みの決定を変更しない。旧資料の圧縮音声OQ-19はOQ-21を指す。
- OQ-02の「公開前」は公開repo化により経過済み。OQ-17の「M3」も経過したが、式入力UIの完成を意味しない。新しい期限や採否は所有者の判断を待つ。
- M3の個別検証記録では、初回workerの未コミット・host待ちと、後続の正式受け入れを冒頭で区別した。Golden手順の古い採用待ち、APIの旧操作数・検索未対応・空effects、MCPの旧版限定、Windows loaderの移植待ちも、後続タスクの証拠と対応付けた。
- 過去の測定値・旧CIは日付とrevision付きで残す。最新結果を参照する入口を更新し、過去の未検証記述を現状の未検証と混同しない。
- M5進捗の再照合では、ワークスペース一覧の音声特徴量・縦書き・ルビの旧未実装表記、simulation crateの記載漏れ、Matte等の古い受け入れ待ち表記を修正した。M5受け入れ表のGUI-007とWindowsのsim001試験コマンドも補い、正本の状態・実workflowとの対応を揃えた。
- status・依存・受け入れ条件を変えたらJSONを編集し、`python3 scripts/backlog.py render` と `check` を実行する。本書・roadmapの集計と対応表も同時に見直す。
