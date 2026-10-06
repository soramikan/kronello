# 現在の実装範囲と残件

確認日: 2026-10-06。基点はPR #8のマージcommit `0c0822c639c165596b6244fe9ffbfec690cfb72a`。参照会話「M3残タスク完了」の調査を手がかりに、現行コード・採用ADR・バックログ・実行ログを照合した記録である。会話内の推測を受け入れ証拠や設計決定として扱わない。

正本は [backlog.json](../backlog/backlog.json)、その生成一覧は [BACKLOG.md](../backlog/BACKLOG.md)。本書は保証範囲と追跡先の索引であり、今後のstatus変更では正本を更新する。M0・M1・M2のP0・M3・M4の完了は、それぞれの受け入れ範囲の完了を意味し、全設計機能・全OS GUI・一般向け配布の完了を意味しない。

## 確認時点の集計

今回、既存77件のstatusを維持し、未割当だった16件をplannedとして追加した。合計93件、done 67 / in_progress 1 / planned 25。M5は13件、M6は12件で、いずれも未着手。M5/M6の配置・優先度は開発計画であり、納期の約束や未決事項の採用判断ではない。

- **実装済み・確認済み:** M3全25件、M4全12件。[M3記録](../testing/m3-acceptance.md)、[M4記録](../testing/m4-acceptance.md)。修正head `1539223` の [CI run 37426096876](https://github.com/soramikan/kronello/actions/runs/37426096876) は全5jobs成功。PR #8は2026-10-06にmainへマージ済み。
- **実装・手順はあるが環境確認待ち:** STORE-003。Dropbox管理フォルダと実network FSの残件があるためin_progress。
- **未実装:** 以下のplannedタスク。型・schema・設計案の存在だけでは実装済みとしない。
- **未決:** OQ-02 / OQ-17 / OQ-21。決定を要するタスクは依存タスクがdoneでも、採否の確定前に実装へ進めない。
- **保証外・意図した制約:** 後述の資源上限、GUI表示込みFPS、software adapterとhardwareの区別。未実装の機能と同一視しない。

## STORE-003 の実環境残件

[検証記録](../testing/store-003.md) に、適応的snapshotの既定不採用（ADR-0052）、2026-10-04のiCloud Drive、2026-10-06のLinux/Windows CIの競合・WAL/DELETE journal強制終了回復を記録した。各OSのstorageテストは35件成功。古い「Linux/Windows未確認」という記述は解消した。

残るのは**実Dropbox管理フォルダと実ネットワークマウント**でのAuto判定、安全モード、同一ホスト内の別プロセス排他、close後の再open・清掃である。通常のローカルCIや似た名前のフォルダでは代替できない。異なるホスト間の同時編集・同期完了・オフライン時の整合性まで保証する試験ではない。

## 既存の未着手タスク

| タスク | 未実装の範囲・確認先 | 依存上の状態 |
|---|---|---|
| VEC-002 | Trim Path、path morphの対応点検証、SVG対応表・外部参照の診断。[04](../architecture/04-vector-text-layout.md) | 依存完了 |
| REPEAT-001 | source共有、instance ID/seed、個別制御と明示expand。[06](../architecture/06-extensions.md) | VEC-002待ち |
| TEXT-002 | 文字selector、縦書き、ルビ、cluster・再組版時の対応。[text実装](../../crates/kronello-text/src/lib.rs) はvertical/rubyを明示拒否 | 依存完了 |
| AUDIO-001 | 音声特徴量DataAsset、分析版・入力hash・窓/hop・時間写像、再解析を避けるcache。[06](../architecture/06-extensions.md) | 依存完了 |
| SIM-001 | 固定刻みsimulation/particle、checkpoint seekと入力変更時の無効化。[06](../architecture/06-extensions.md) | REPEAT-001待ち |
| DIM-001 | Scene3D、camera、2D順序と3D depthの分離、補助port。[06](../architecture/06-extensions.md) | 依存完了 |
| INTEROP-001 | OTIO/SVG/外部render交換、保持・変換・欠落レポートと明示bake。[06](../architecture/06-extensions.md) | VEC-002待ち |
| PLUGIN-001 | WASM runtime、fuel/memory/host call/権限と信頼区分。[06](../architecture/06-extensions.md) | 依存完了 |
| DIST-001 | remote worker、素材・font・engine版の一致、simulation bake境界。[14](../architecture/14-jobs.md) | SIM-001待ち |

## 今回追加した追跡タスク

詳しい依存・受け入れ条件は生成バックログを参照する。既存doneタスクの受け入れ範囲を後から拡大して取り消すのではなく、未保証の範囲を分離する。

| タスク | 配置 | 現状・追加理由 | 根拠 |
|---|---|---|---|
| NAME-001 | M5 / P1 | 商標照会とcrate/domainの確保状況が未確認。公開前の判断目安を経過した | [OQ-02](../open-questions.md#oq-02-商標の確認と名前の確保)、[名称調査](../naming.md) |
| EXPR-002 | M5 / P1 | ASTと有界評価は実装済み。人間向け構文、parser/formatter、GUI式入力は別範囲 | [OQ-17](../open-questions.md#oq-17-式言語の構文)、[ADR-0058](../adr/0058-bounded-canonical-expression-ast.md) |
| EXPR-003 | M5 / P2 | DataAsset参照、動的な過去Property sample、連続補間noiseはEXPR-001の対応外。AST評価の拡張として構文と分離 | [03 式](../architecture/03-property-animation.md#式)、[EXPR-001](../testing/expr-001.md) |
| AUDIO-005 | M5 / P1 | AAC/Opusの採否・配布構成・遅延/終端sampleを検証する。ALAC対応をAAC対応とは扱わない | [OQ-21](../open-questions.md#oq-21-圧縮音声-aac-と配信向け音声の採用)、[ADR-0068](../adr/0068-versioned-delivery-movie-profiles.md) |
| MATTE-001 | M5 / P1 | alpha/luminance matteのrender入力はあるが、作品として保存・編集するmatte関係はない | [snapshot.rs](../../crates/kronello-render/src/snapshot.rs)、[05](../architecture/05-render-gpu.md) |
| INSPECT-002 | M5 / P1 | render.explainの転送見積もりと二重実行noticeが単一graph最適化前のまま。計画診断と実行countersの整合が必要 | [inspect.rs](../../crates/kronello-render/src/inspect.rs)、[ADR-0092](../adr/0092-single-graph-gpu-final-output-and-observations.md) |
| FRAMEBRIDGE-001 | M5 / P2 | generic `PathKind::VideoToolbox`は未実装診断。具体的なBgra8/Nv12 decodeは別に実装済み。genericの意味・廃止等を整理する | [FrameBridge](../../crates/kronello-framebridge/src/lib.rs)、[GPU-003](../testing/gpu-003.md) |
| GUI-007 | M5 / P2 | macOSで明示的に無効化・表示専用になっている編集操作を追跡する | 下表と [10 GUI](../architecture/10-desktop-gui.md) |
| GUI-005 / GUI-006 | M6 / P2 | Windows WinUI 3 / Linux GTK4アプリ、native preview surfaceとOS固有操作は未実装・未検証 | [ADR-0032](../adr/0032-windows-winui-linux-gtk.md)、[10 GUI](../architecture/10-desktop-gui.md) |
| RELEASE-002 | M6 / P1 | macOS Developer ID、notarization、staple、Gatekeeper、quarantineを保持した実ダウンロード | [RELEASE-001](../testing/release-001.md)。開発署名の受け入れと分離 |
| RELEASE-003 / RELEASE-004 | M6 / P2 | Windows/LinuxのGUIを含む製品packageとクリーン環境での配布確認 | [12](../architecture/12-platform-dependencies.md)。CLIビルド成功と分離 |
| GPU-004 / GPU-005 | M6 / P2 | Windows/Linuxのhardware resident media/render経路 | [GPU-003](../testing/gpu-003.md)。software decode/goldenの成功と分離 |
| GPU-006 | M6 / P2 | macOS residentのHDR/10-bit/full-range/HEVC hev1等を形式別に評価・昇格 | [GPU-003](../testing/gpu-003.md)。software HDR出力や8-bit SDR保証と分離 |

### macOS GUI で残る明示的な制限

これはコード上の現状調査であり、今回新しいGUI操作試験は実施していない。実装済みページの受け入れ記録とは区別する。GUI-007着手時に共有APIの対応有無を点検し、必要ならコア契約から実装する。

| 制限 | コードの根拠 |
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
- 過去の測定値・旧CIは日付とrevision付きで残す。最新結果を参照する入口を更新し、過去の未検証記述を現状の未検証と混同しない。
- status・依存・受け入れ条件を変えたらJSONを編集し、`python3 scripts/backlog.py render` と `check` を実行する。本書・roadmapの集計と対応表も同時に見直す。
