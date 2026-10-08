# バックログ一覧

<!-- このファイルは scripts/backlog.py render が生成する。直接編集しない。正本は backlog.json。 -->

- schema_version: 0.5
- 更新日: 2026-10-08
- タスク数: 141

## 集計

| マイルストーン | planned | in_progress | done | dropped | 計 |
|---|---:|---:|---:|---:|---:|
| M0 | 0 | 0 | 6 | 0 | 6 |
| M1 | 0 | 0 | 14 | 0 | 14 |
| M2 | 0 | 1 | 10 | 0 | 11 |
| M3 | 0 | 0 | 25 | 0 | 25 |
| M4 | 0 | 0 | 12 | 0 | 12 |
| M5 | 0 | 1 | 12 | 0 | 13 |
| M7 | 0 | 0 | 11 | 0 | 11 |
| M8 | 0 | 0 | 16 | 0 | 16 |
| M9 | 12 | 0 | 0 | 0 | 12 |
| M10 | 7 | 0 | 0 | 0 | 7 |
| M11 | 14 | 0 | 0 | 0 | 14 |

## M0

### ARC-001 意味規約と互換性境界をADRに固定

- 優先度: P0 / 領域: architecture / 状態: done
- 依存: なし
- 受け入れ条件:
  - Timeline/Composition/Property/Renderの依存方向を文書化する
  - 単位・alpha・未知機能・スナップショット版の規約をレビューする
  - 作業用色空間（既定は線形Rec.709、HDRは線形Rec.2020）とタグなし色入力のsRGB解釈を規約として固定する

### TIME-001 有理数時間・半開区間・時間マッピング

- 優先度: P0 / 領域: time / 状態: done
- 依存: ARC-001
- 受け入れ条件:
  - 整数overflowを検出しJSONの整数精度を失わない
  - 30000/1001fpsと48kHz音声の境界をproperty-based testで検証する

### PROP-001 型付きPropertyとschema registry

- 優先度: P0 / 領域: model / 状態: done
- 依存: ARC-001
- 受け入れ条件:
  - 型・単位・補間可否・範囲・安定IDを共通schemaで表す
  - Constant/Curve/Expressionの主値源を排他的に保持する

### GPU-001 最短GPU描画とFrameBridge技術スパイク

- 優先度: P0 / 領域: gpu / 状態: done
- 依存: ARC-001
- 受け入れ条件:
  - 2D素材からRGBA16F出力までの色とalphaを検証する
  - CPU往復とGPU内コピーを区別して各候補経路の可否を記録する
  - macOS (Metal / VideoToolbox) を最初の検証対象とする
  - sRGB色入力から線形Rec.709作業空間への変換と、線形Rec.2020作業空間での同じ色の一致を検証する

### QA-001 参照素材とgolden sceneを整備

- 優先度: P0 / 領域: test / 状態: done
- 依存: ARC-001
- 受け入れ条件:
  - 日本語・alpha・異なるfps・VFR・HDRの権利確認済みfixtureを用意する
  - 固定環境の画像比較と意味的比較を別に定義する
  - 素材は生成またはCC0/自作、フォントはOFLに限り、出典とライセンスを台帳に記録する
  - 大きい素材はhash固定の取得スクリプトで取得し、取得失敗時は該当テストを失敗として扱う

### CI-001 ツールチェーン固定とCI

- 優先度: P0 / 領域: infra / 状態: done
- 依存: ARC-001
- 受け入れ条件:
  - rust-toolchain.tomlでstableの特定版に固定し、edition 2024とする
  - rustfmtとclippy(警告をエラー扱い)をCIで必須にする
  - GitHub ActionsでmacOSと、ソフトウェアVulkanのLinuxを実行する
  - GPU画素のgolden比較を固定環境で実行する手順を文書化する


## M1

### STORE-001 文書・イベント・スナップショット保存

- 優先度: P0 / 領域: storage / 状態: done
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - 文書とイベントとrevisionを同一transactionで更新する
  - 完全snapshotからの復元と失敗migrationの非破壊性を確認する
  - 単一SQLiteファイル(.kronello)を正本とし、レンダーキャッシュをプロジェクト外へ分離する
  - 別プロセスからの同時書き込みをrevision照合で直列化し、古いbase_revisionを拒否する
  - イベントごとにsession・変更したキーの集合・逆操作情報を保存する
  - WALで開き、最後のプロセスが閉じると付随ファイルが残らない。異常終了後は次回に回復する
  - 同期フォルダ等では安全モード(非WAL・単一プロセス)で開き、他プロセスにはPROJECT_LOCKEDを返す
  - 公開JSONスキーマによるproject.export/importが往復し、未知フィールドを保持する
  - history.compactが指定revisionより前の履歴を切り捨て、基点の完全snapshotを残す

### COMP-001 Composition/Instance/Group/Nullモデル

- 優先度: P0 / 領域: scene / 状態: done
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - InstancePathで共有定義の複数配置を区別する
  - 所有・変換・参照の循環を個別に診断する

### ANIM-001 基本キーフレームと補間

- 優先度: P0 / 領域: animation / 状態: done
- 依存: TIME-001, PROP-001
- 受け入れ条件:
  - Hold/Linear/Cubicと連続角の複数回転を実装する
  - 同時刻key upsertと任意時刻評価の結果を固定する

### EVAL-001 Property依存グラフと任意時刻評価

- 優先度: P0 / 領域: evaluation / 状態: done
- 依存: COMP-001, ANIM-001
- 受け入れ条件:
  - 順序・逆順・ランダム順の評価で同じ意味的結果を返す
  - 依存循環の経路と原因Propertyを報告する

### VEC-001 Shape/Path/Fill/Stroke IR

- 優先度: P0 / 領域: vector / 状態: done
- 依存: COMP-001
- 受け入れ条件:
  - 角丸矩形・楕円・ベジェ・基本fill/strokeを保持する
  - 拡大と解像度変更で文書をビットマップ化しない

### TEXT-001 日本語組版とクラスタ/グリフ対応

- 優先度: P0 / 領域: text / 状態: done
- 依存: COMP-001, QA-001
- 受け入れ条件:
  - 横書き日本語・基本禁則・フォント固定・欠落検査を実装する
  - 書記素とグリフが一対一でないfixtureで対応を検証する

### GPU-002 線形色合成・mask・isolated group

- 優先度: P0 / 領域: gpu / 状態: done
- 依存: GPU-001, VEC-001, TEXT-001
- 受け入れ条件:
  - Group opacityを子ごとに適用した誤結果をgolden testで区別する
  - mask/alpha edge/色変換を固定環境で検証する

### RENDER-001 Scene IRからRender DAGと画像連番

- 優先度: P0 / 領域: render / 状態: done
- 依存: EVAL-001, GPU-002
- 受け入れ条件:
  - 指定有理数時刻と領域を入力に一枚の画像を描画する
  - 出力の色/alpha規約をmetadataへ記録する

### CACHE-001 値・layout・geometry・rasterの分離cache

- 優先度: P0 / 領域: cache / 状態: done
- 依存: RENDER-001
- 受け入れ条件:
  - 位置変更で組版cacheを再利用する
  - 色・本文・フォント変更が必要な範囲だけを無効化する

### CLI-001 ヘッドレス生成と機械向けCLI

- 優先度: P0 / 領域: cli / 状態: done
- 依存: STORE-001, RENDER-001
- 受け入れ条件:
  - stdin JSON/stdout JSONとstderr logを分離する
  - GUIなしでShapeと日本語Textのアニメーション連番を生成する

### STORE-002 完全snapshotの間引き保存

- 優先度: P1 / 領域: storage / 状態: done
- 依存: STORE-001
- 受け入れ条件:
  - 完全snapshotを初期revision・64 revisionごと・history.compactの基点にだけ保存する
  - 任意revisionを直前の完全snapshotとイベントのpatch(最大63個)の再適用で復元し、全revision保存時と同じ文書を返す
  - 全revisionを保存した既存の.kronelloを開いて同じ復元結果を返し、元ファイルを壊さない
  - 間引き後もrevision照合・逆操作情報・idempotencyの記録とcompactの意味を変えない

### VEC-003 線のjoin/capと線形・放射グラデーション

- 優先度: P1 / 領域: vector / 状態: done
- 依存: VEC-001, GPU-002, RENDER-001
- 受け入れ条件:
  - strokeのjoin(miter/bevel/round、miter limit既定4)とcap(butt/square/round)をGPUとCPU参照で同じ定義で描画する
  - 線形・放射(中心と半径)グラデーションをShapeのfill/strokeのpaintとして保持し、範囲外はpadとする
  - グラデーションの色は作業用線形空間のpremultipliedで補間し、stopの色と位置をアニメーションできる
  - 補間・描画の意味の版をRenderSnapshotのmetadataへ記録し、後続(VEC-004/VEC-005)の機能を含む文書は最終レンダーでUNSUPPORTED_FEATUREとする

### QA-003 Apple Silicon/Metal共通のgolden基準画像

- 優先度: P1 / 領域: qa / 状態: done
- 依存: GPU-002, VEC-003
- 受け入れ条件:
  - ADR-0038のgolden固定環境の決定を新しいADRで置き換え、Apple SiliconのMetalを共通の比較環境とする(性能計測の基準機はM4 Mac miniのまま)
  - M1開発機で基準画像を登録し、明示実行のgoldenが許容誤差2^-10で比較・合格する
  - adapter・OS・機種はprovenanceとして記録し、比較の可否判定に使わない

### CLI-002 GPU不在時の型付きエラー

- 優先度: P1 / 領域: cli / 状態: done
- 依存: CLI-001
- 受け入れ条件:
  - テスト専用の仕組みでadapter取得を失敗させ、CLIがADAPTER_UNAVAILABLEを返して非0で終了することを通常テストで確認する
  - GPU不在時にCPU参照backendへ暗黙に切り替えない


## M2

### MEDIA-001 FFmpeg素材I/Oと時刻精度

- 優先度: P0 / 領域: media / 状態: done
- 依存: TIME-001, QA-001, GPU-001
- 受け入れ条件:
  - VFR/B-frame/seek後の対象PTSを確認する
  - 使用中のdecode/encode/transfer経路を報告する
  - LGPL構成のFFmpegを動的リンクし、検出したcodec/hwaccelをcapabilitiesへ報告する
  - 素材を相対パス・絶対パスの順に解決してhashを照合し、ASSET_MISSING/ASSET_HASH_MISMATCHを報告する
  - asset.relinkがhash一致のファイルだけを再リンクし、project.collectが相対パスのフォルダを書き出す
  - ソフトウェアエンコードはAV1とProResを提供し、H.264/HEVCのエンコーダーがない環境ではENCODER_UNAVAILABLEを返す
  - 同梱用FFmpegのビルドスクリプトとnative dependencies manifestを管理する

### AUDIO-000 基本音声: デコード・ミックス・音声付き書き出し

- 優先度: P0 / 領域: audio / 状態: done
- 依存: TIME-001, MEDIA-001
- 受け入れ条件:
  - 素材音声を48kHzへ変換しクリップ音量を適用してBusへミックスする
  - 音声サンプル位置を絶対時刻から計算し、映像との同期をサンプル精度で検証する
  - 書き出しで映像と音声を同じ固定snapshotからmuxする

### NLE-001 CompositionClipとマルチトラック統合

- 優先度: P0 / 領域: timeline / 状態: done
- 依存: COMP-001, MEDIA-001, RENDER-001
- 受け入れ条件:
  - 同じCompositionを異なる長さ/時刻で複数配置する
  - trim/stretch/instance retimeを別操作としてテストする

### FX-001 基本エフェクト: drop shadow / gaussian blur

- 優先度: P0 / 領域: effects / 状態: done
- 依存: GPU-002, RENDER-001
- 受け入れ条件:
  - エフェクトが必要な入力領域(ROI halo)を宣言しvisual_boundsへ反映する
  - premultiplied alphaと線形作業空間でのshadow/blurをgolden testで検証する

### SERVICE-001 計画・適用・競合・冪等性

- 優先度: P0 / 領域: service / 状態: done
- 依存: STORE-001, CLI-001
- 受け入れ条件:
  - base_revision/plan_hash/idempotency_keyを検証する
  - 同じキー同じpayloadは重複適用せず異なるpayloadは拒否する
  - idempotency_keyと適用結果をプロジェクト内に保存し、別プロセスからの再送にも同一結果を返す
  - edit.undoが対象イベントの逆操作を新しいrevisionとして発行し、後続の未取り消しイベントが同じキーに触れていればUNDO_CONFLICTで何も適用せず拒否する
  - 値変更は(オブジェクト,Property)単位、構造変更はオブジェクトと親コンテナ単位で競合を判定する

### TEMPLATE-001 公開入力と保護時間区間の最小template

- 優先度: P0 / 領域: template / 状態: done
- 依存: COMP-001, SERVICE-001
- 受け入れ条件:
  - 定義とinstance入力を分離し版を固定する
  - 5秒から8秒にしてintro/outroの長さが変わらない
  - テキストlayout_boundsへの単方向参照で背景帯が追従し、max_lines超過をoverflowとして検出する

### API-001 共通schemaとquery/command公開

- 優先度: P0 / 領域: api / 状態: done
- 依存: SERVICE-001, PROP-001, EVAL-001
- 受け入れ条件:
  - scene query/property sample/capabilitiesを構造化結果で返す
  - APIの任意シェル/外部URL実行を禁止する
  - history.listでイベント・session・変更したキー・取り消し状態を返す

### MCP-001 MCPアダプター

- 優先度: P0 / 領域: mcp / 状態: done
- 依存: API-001
- 受け入れ条件:
  - 交渉したprotocol versionでschemaとstructuredContentを返す
  - クライアント接続状態に暗黙の対象Projectを保持しない

### JOB-001 固定snapshot書き出しjob

- 優先度: P0 / 領域: jobs / 状態: done
- 依存: SERVICE-001, MEDIA-001, RENDER-001
- 受け入れ条件:
  - render.submitが固定snapshotをプロジェクト外へ保存し、ユーザーごとの状態DBへジョブを記録して、切り離したworkerプロセスを起動する
  - 投入したプロセスやMCP接続が終了してもジョブが継続し、保存されたjob IDで状態取得できる
  - 既定で同時実行は1ジョブとし、残りはqueuedで待機する
  - workerの異常終了をheartbeatの途絶で検出しinterruptedとして報告する
  - ジョブの進行で.kronelloへ書き込まない
  - 一時出力を検証してから確定名へ切り替える
  - 終了から30日を過ぎたジョブディレクトリを次の投入時に掃除し、interruptedは対象外とする。job.pruneで手動掃除できる

### INTEGRATION-001 縦断デモ第1段階: 日本語lower-third (CLI/MCP)

- 優先度: P0 / 領域: integration / 状態: done
- 依存: NLE-001, TEMPLATE-001, MCP-001, JOB-001, AUDIO-000, FX-001
- 受け入れ条件:
  - CLIで作成しMCPで確認して固定snapshotから4K出力する
  - 別instanceの文字/色/長さが干渉しない
  - 5秒から8秒への尺変更と別テキストで、背景帯追従・overflow検出・基本shadowを検証する

### STORE-003 保存の運用検証と適応的snapshot

- 優先度: P2 / 領域: storage / 状態: in_progress
- 依存: STORE-002
- 受け入れ条件:
  - patchの累計が文書サイズを超えたときにも完全snapshotを取る方式を評価し、採否を記録する
  - iCloud Drive・Dropbox等の実際の同期フォルダとネットワークファイルシステムで、安全モードとPROJECT_LOCKEDを手順で確認して記録する
  - Linux/Windowsで複数プロセスの競合と強制終了後の回復を確認する
  - 2026-10-06時点の確認済み範囲（ADR-0052、host iCloud Drive、run 37426096876のLinux/Windows競合・強制終了回復）と未確認範囲（実Dropbox管理フォルダ・実ネットワークFS）をdocs/testing/store-003.mdに分けて記録し、残る実環境検証が完了するまでin_progressを維持する


## M3

### EXPR-001 型付きASTと有界式評価

- 優先度: P1 / 領域: expression / 状態: done
- 依存: EVAL-001, API-001
- 受け入れ条件:
  - 静的依存列挙と命令/メモリ/サンプル予算を実装する
  - 固定seedのnoiseと禁止機能の拒否をテストする
  - ASTを式の正本とし、将来の式言語と一対一に往復できる構造にする
  - Expressionの設定を共有edit.plan/applyと公開schemaへ接続し、property.sampleと最終レンダーで同じ有界評価・依存診断を使う。revision/idempotency/Undoと固定snapshotを検証する

### LAYOUT-001 responsive layoutとbounds段階

- 優先度: P1 / 領域: layout / 状態: done
- 依存: TEXT-001, VEC-001, EVAL-001
- 受け入れ条件:
  - layout/ink/visual boundsを区別する
  - 文字幅と背景幅の循環およびoverflowを診断する
  - ink_boundsを背景帯追従へ供給する純粋なLayoutValueと依存宣言を定義し、layout/ink/visualの選択が組版と描画結果で一致することを検証する

### TEMPLATE-002 長さ・縦横比variant・data入力・版移行

- 優先度: P1 / 領域: template / 状態: done
- 依存: TEMPLATE-001, LAYOUT-001
- 受け入れ条件:
  - 短尺拒否/hold/loop/stretchを明示する
  - 版更新の差分計画とプレビューを生成し勝手に既存作品を更新しない
  - MediaSlot/DataTable入力と縦横比variantを共有schema・公開入力policyに接続し、別instanceの入力・尺・版が干渉しないことを検証する
  - tight-ink背景帯追従をlayout_bounds追従と区別した明示policyとして設計し、短文/空白/複数行/変換後のink_boundsとpaddingを検証する。既存templateのwrap_width基準を暗黙に変更せず、循環/overflowを型付き診断する

### GUI-001 macOSネイティブGUI: Canvas・階層・変換操作

- 優先度: P1 / 領域: gui / 状態: done
- 依存: FFI-001, RENDER-001
- 受け入れ条件:
  - GUI操作が共通command/eventを使う
  - 選択・pan/zoomなどUI状態を作品から分離する
  - CLI/MCPなど外部プロセスによるrevision変化を検知して再読込する
  - GUIのUndoは自セッションの操作だけを取り消し、競合時は理由を表示する
  - UI状態をユーザーごとの状態領域に保存し、.kronelloへ書き込まない

### GUI-002 Dope sheetとCurve editor

- 優先度: P1 / 領域: gui / 状態: done
- 依存: GUI-001, ANIM-001
- 受け入れ条件:
  - キー移動/接線編集/UndoがCLIで読める同じモデルを変更する
  - 空間パスと時間イージングを区別して表示する

### AUDIO-002 リアルタイム音声再生とA/V同期

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-000, GUI-001
- 受け入れ条件:
  - 音声コールバックをプロジェクト更新・ディスク読み出し・式評価と別の実行系にする
  - プレビュー再生で音声クロックを基準に映像フレームを提示する
  - seek・再生停止/再開・非整数fpsで音声sample格子と映像提示の同期を検証し、offline exportやCPU参照だけを実時間再生の受け入れ証拠にしない

### INSPECT-001 非表示原因・依存・レンダー経路のexplain

- 優先度: P1 / 領域: inspection / 状態: done
- 依存: API-001, LAYOUT-001, CACHE-001
- 受け入れ条件:
  - opacity/active range/parent/mask/asset不足を要因別に返す
  - 過大処理と転送/メモリ/キャッシュを構造化して表示する

### FFI-001 kronello-ffi: ネイティブGUI向けCommand/Query境界

- 優先度: P1 / 領域: ffi / 状態: done
- 依存: API-001
- 受け入れ条件:
  - SwiftからCommand/Query APIを呼び、CLIと同じrevision/eventへ到達する
  - ネイティブ側のCAMetalLayerをwgpu surfaceとして受け取りプレビューを表示する
  - FFI境界にwgpu/SQLite/Tokioの型を露出しない
  - C ABIの関数は少数に保ち、Command/QueryはCLI/MCPと同じJSONで受け渡す
  - Swiftの型を公開JSON Schemaから生成する
  - 高頻度経路でのJSON直列化コストを計測する

### QA-002 GUI/CLI/MCP同等性と日本語IME

- 優先度: P1 / 領域: test / 状態: done
- 依存: GUI-002, MCP-001, INTEGRATION-001
- 受け入れ条件:
  - 同じ編集操作の結果snapshotが一致する
  - 未確定IME文字列を作品履歴へ大量commitしない

### INTEGRATION-002 縦断デモ第2段階: GUI閲覧と縦型variant

- 優先度: P1 / 領域: integration / 状態: done
- 依存: INTEGRATION-001, TEMPLATE-002, GUI-001
- 受け入れ条件:
  - 第1段階と同じプロジェクトをmacOS GUIで開き、CLI/MCPと同じ値・layout boundsを表示する
  - 同じtemplate定義を縦型variantで再利用し、再レイアウト結果を検証する

### GUI-003 macOS GUI: 編集ページ（Sequence のトラック・素材一覧・クリップの Inspector）

- 優先度: P2 / 領域: gui / 状態: done
- 依存: GUI-001, NLE-002
- 受け入れ条件:
  - docs/design-system/screens/edit.md の配置で Project の素材一覧・Sequence の Viewer・クリップの Inspector・トラックを表示し、種類の色・選択・ASSET_MISSING を仕様どおりに示す
  - クリップの配置・トリム・ブレードを共通 command/event で行い、ドラッグ中は候補表示、確定時に 1 コマンドを発行する。Undo・外部変更・競合は GUI-001 と同じ扱いにする
  - Composition クリップからモーションページへ移り、その Composition を開ける

### GUI-004 macOS GUI: テンプレートページと書き出しページ

- 優先度: P2 / 領域: gui / 状態: done
- 依存: GUI-001, TEMPLATE-002, INSPECT-001
- 受け入れ条件:
  - docs/design-system/screens/template.md の配置で variant を並べて layout/ink/visual bounds と TEMPLATE_OVERFLOW 等の診断を表示し、公開入力・版の差分計画・尺のポリシーを共通 API で扱う。既存配置を暗黙に更新しない
  - docs/design-system/screens/export.md の配置で書き出し設定・書き出し前の確認・ジョブ一覧を表示し、型付きエラーがある間は書き出しを開始できない。投入した job は固定 snapshot の独立 worker で実行し、進捗・失敗を JobRow で示す

### VEC-004 グラデーションの拡張

- 優先度: P2 / 領域: vector / 状態: done
- 依存: VEC-003
- 受け入れ条件:
  - repeat/reflectのspreadを実装する
  - 焦点付き放射(焦点位置・焦点半径)と円錐(sweep)グラデーションを実装する
  - グラデーションごとに補間空間(sRGB・straight等)を明示指定でき、補間空間の意味の版を管理する
  - 図形のbounding box基準の座標とgradient transformを扱う
  - テキストのfillにグラデーションを適用し、組版クラスタを壊さない
  - SDR 8bit出力のbanding対策(dither)の要否を判断し、採用する場合は固定seedで決定的にする

### VEC-005 線の拡張: 破線・線の位置・非一様変換

- 優先度: P2 / 領域: vector / 状態: done
- 依存: VEC-003
- 受け入れ条件:
  - 破線(dash配列・offset)とoffsetのアニメーションを実装する
  - 線の位置(中央・内側・外側)を指定できる
  - 非一様scale/skew下の線幅の意味を定義し、現状のUNSUPPORTED_FEATUREを解消する

### NLE-002 動画Asset/Generator ClipとTimeline編集の拡張

- 優先度: P1 / 領域: timeline / 状態: done
- 依存: NLE-001, MEDIA-001, FX-001
- 受け入れ条件:
  - 動画Asset Clipを明示stream・source_in・TimeMapで描画し、CFR/VFR/B-frameの正確なseekと配置境界、異なる配置の独立性を検証する
  - Generator Clipの実行契約と意味版を定義し、固定snapshotから決定的に描画する。未知Generatorを代替画像で継続しない
  - clip effectsを既存Render DAGへ接続し、配置・変換・ROI haloと合成順をCPU参照および明示GPU実行で検証する
  - transition・ripple・リンク連動編集の範囲と競合キーを定義し、共通plan/apply・idempotency・selective Undoと保存再読込で検証する

### AUDIO-003 Sequence音声trackのA/V muxとclip音量

- 優先度: P1 / 領域: audio / 状態: done
- 依存: NLE-001, AUDIO-000, JOB-001
- 受け入れ条件:
  - Sequenceのaudio trackを同じ固定RenderSnapshotから音声配置へcompileし、映像と音声を同期exportおよびrender.submitのProRes/PCM24 MOVへmuxする
  - Sequence音声とoutput.clipsの明示配置の選択・併用規約を定義し、音声の二重加算や暗黙silenceへの置換を防ぐ
  - clip volumeを非負の線形Gainとして共有編集APIに公開し、定数・Curve/Propertyによる音量アニメーションを純粋評価する。revision・idempotency・Undo・固定入力hashを検証する
  - Compositionと入れ子instanceの再帰音声の時間写像・ミックス順を定義し、別配置の独立性、trim、負時刻、NTSC格子でのsample数とA/V duration差1/48000秒未満を検証する
  - 投入後の作品編集・削除が映像と音声を変えず、素材不足・hash不一致・clippingの型付き失敗で成果物を公開しない

### AUDIO-004 リタイム音声・audio effects・Generator音声

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-003, ANIM-001
- 受け入れ条件:
  - retimed audioのTimeMap・補間・pitchの扱いを設計して意味版を固定し、線形/区分線形map、trim/stretch、非整数sample境界で同期とsource範囲を検証する
  - audio effectsとGenerator音声の純粋評価・資源予算・固定入力の契約を定義し、同じsnapshotを任意順のbatchで評価して一致することを検証する
  - Sequenceのcrossfadeに含まれる音声clipの音声crossfade（曲線・意味版・sample境界）を定義して検証し、現行のUNSUPPORTED_FEATUREを対応範囲内で解消する
  - 未対応map・effect・Generator、予算超過や非有限値を型付きエラーにし、暗黙の速度1・無音・clampで続行しない

### MEDIA-002 追加movie job profileと圧縮音声

- 優先度: P1 / 領域: media / 状態: done
- 依存: MEDIA-001, AUDIO-000, JOB-001
- 受け入れ条件:
  - AV1・H.264・HEVCを同期exportとrender.submitの版付き出力profileとして公開し、固定snapshot・PTS/duration・probe・no-clobber publicationを検証する
  - H.264/HEVCはVideoToolboxのallow_sw=0を維持し、非対応hostではENCODER_UNAVAILABLEを返す。software成功をhardware検証に数えず、codec/転送経路を記録する
  - AAC・ALACとAV1配信用音声のcodec/container契約、priming・padding・終端sampleの扱い、配布・特許方針を採用前に文書化し、採用profileのA/V同期とroundtripを検証する
  - LGPL同梱runtimeと開発用system FFmpegを区別し、任意codec名・FFmpeg引数・暗黙codec fallbackを公開APIに追加しない。HDR profileはCOLOR-001で扱う

### RENDER-003 長尺・大解像度exportのstreaming

- 優先度: P1 / 領域: render / 状態: done
- 依存: JOB-001, AUDIO-000, FX-001
- 受け入れ条件:
  - 音声source/Busと映像payloadを有界batchでdecode・render・encode/muxし、10分/全source 28800000 stereo frames・映像256 MiBの現行上限を超える長尺exportを再現可能な手順で検証する
  - tileから最終成果物へのstreamingにより最終linear/display面の全画面保持を減らし、node別tile allocationとROI/haloの処理予算を定義する。巨大haloは予算超過として診断する
  - 従来の全体exportと画素・音声samples・rational PTS/durationを比較し、負/非ゼロ開始・NTSC・tile境界に欠落や重複を作らない
  - 長尺/4Kのpeak memory・I/Oを実測し、cancel・容量不足・encode失敗でも既存成果物を壊さず一時出力を処理する。強制終了後の回収とresumeはRECOVERY-001で扱う

### MCP-002 MCP HTTP・resources/prompts・進捗とキャンセル

- 優先度: P1 / 領域: mcp / 状態: done
- 依存: MCP-001, JOB-001
- 受け入れ条件:
  - HTTP transportの接続・認証・版交渉・終了の契約を定義し、stdioと同じCommand/Query registry・schema・型付き結果へ到達することを実クライアントで検証する
  - resources・promptsの公開範囲とschemaを定義し、毎回明示したProjectだけを読み、素材文字列を命令として実行しない
  - 実行中要求のキャンセルとprogress通知を追加し、request IDとの対応・競合・接続終了を検証する。永続job.cancelと要求キャンセルの意味を区別し、接続終了だけでdetached jobを停止しない
  - sampling・MCP taskの対応範囲と能力交渉を定義し、採用機能を共有serviceへ接続する。未対応機能を対応済みとして広告しない
  - 入力上限・未知/重複field拒否・stdout protocol分離を維持し、外部MCP client SDKで成功/失敗/通知を検証する

### JOB-002 Windows workerのdetachと各OSの実プロセス検証

- 優先度: P1 / 領域: jobs / 状態: done
- 依存: JOB-001, MCP-001
- 受け入れ条件:
  - WindowsでCLI/MCPの終了から独立するworker起動・stdio/log・回収の契約を実装し、親終了後も固定入力のジョブが継続することを実機またはCIで検証する
  - Windowsの上書き禁止publicationを設計し、同一volume・既存file/空directoryの拒否・cancel/lease競合を実プロセスで検証する
  - Linux/WindowsでFIFO slot・queued/running heartbeat・強制終了後のinterrupted・pruneを検証し、OS/版/コマンド/exitを記録する。未検証OSを保証経路に昇格しない
  - 高負荷時（load average 50 前後）に CLI の job テスト（例: sequence_target_job_preserves_placements_after_trim_and_project_removal）が Queued のまま 60 秒で timeout する事象を再現・原因調査し、状態 DB の lock 競合下でも job が開始されることを検証する
  - テストが失敗・時間切れになっても、起動した detached worker（`kronello worker --job`）を必ず回収し、テスト後に孤立 process を残さないことを検証する（2026-10-05 に最長14時間残った worker を6件確認）

### FX-002 非一様affine下のblur/shadow

- 優先度: P2 / 領域: effects / 状態: done
- 依存: FX-001
- 受け入れ条件:
  - 非一様scale/shear下の正sigmaのGaussian blur/drop shadowの意味を定義して版を管理し、現行のUNSUPPORTED_FEATUREを対応範囲内で解消する
  - 変換後のkernel・offset・解析boundsとROI haloをCPU参照/GPUで比較し、回転・非一様scale・shear・tile境界のcrop一致と有限予算を検証する

### API-002 scene検索・paging・固定履歴cursorとevent stream

- 優先度: P2 / 領域: api / 状態: done
- 依存: API-001, INTEGRATION-001
- 受け入れ条件:
  - scene.queryに範囲・タグ・種類による検索とpagingを追加し、InstancePath/NodeId・所有順・revisionを保ち、既存の明示evaluation modeとの整合性を検証する
  - history.listの複数ページを固定snapshot cursorで取得し、並行編集/Undo/compactで欠落・重複・取り消し状態の混在を起こさない。失効cursorは型付きエラーにする
  - CLIのNDJSON event streamの版・framing・終了/取消契約を定義し、既存の一要求一stdout JSON modeを維持する。診断はstderrへ出す
  - 全拡張を共有registry/公開schema/CLI/MCP経由で検証し、暗黙のcurrent Projectや入口専用状態を作らない

### SERVICE-002 project作成/importの計画・冪等性とModifier編集

- 優先度: P2 / 領域: service / 状態: done
- 依存: SERVICE-001, API-001
- 受け入れ条件:
  - project.create/importの変更計画・再送の冪等性を共通APIで設計し、createの出力予約とimportのrevision照合、同一キー異payloadの拒否、保存後再送を実プロセスで検証する
  - Modifier編集を型付きCommandとして公開し、順序付き配列の変更・競合キー・inverse・selective Undoを検証する。Expression設定/評価はEXPR-001へ接続する
  - 未対応Modifierの保存・編集・評価の境界を文書化し、未対応の最終レンダーを代替値で続行しない。生patch/inverse/changed_keysを入口から受け取らない

### RELEASE-001 同梱FFmpegを含む配布packageの再配置・署名検証

- 優先度: P1 / 領域: release / 状態: done
- 依存: MEDIA-001, AUDIO-000, CLI-001, MCP-001
- 受け入れ条件:
  - macOSの配布packageに同梱するFFmpeg共有library・SVT-AV1・dav1d・license原文・PATENTS・source manifestを固定し、GPL/nonfree・開発用system libraryを含めないことを検証する
  - 元のbuild prefixから独立した場所へpackageを再配置し、全5 FFmpeg libraryの動的リンク・ABI・差し替えとcapabilities、ProRes/PCM24・AV1 roundtripを検証する。単一libavcodecのotool結果でpackage全体を合格にしない
  - macOSの署名・配布検証の手順を固定して再配置後の実binary起動を確認し、platform/revision/package hash/コマンド/exitを記録する。Windows/Linux packageは各platformで別途検証してから保証する


## M4

### RENDER-002 時間サンプルと高品質モーションブラー

- 優先度: P1 / 領域: render / 状態: done
- 依存: RENDER-001, CACHE-001, ANIM-001
- 受け入れ条件:
  - サブ時刻ごとの全体合成を基準に比較する
  - カット境界/ネスト/露光位相/重複サンプルを扱う

### GPU-003 各OSのFrameBridge保証経路

- 優先度: P1 / 領域: gpu / 状態: done
- 依存: GPU-001, GPU-002, MEDIA-001, NLE-002
- 受け入れ条件:
  - macOS (Metal / VideoToolbox) を最初の保証経路とし、Windows/Linuxは順次昇格する
  - 対応形式ごとにデバイス/所有権/同期/寿命を検証する
  - 非対応経路は明示fallbackまたはrequire_gpu_residentエラー
  - hardware decodeとGPU resident media/renderの接続を形式別に検証し、MEDIA-001のsoftware decodeやhardware encode成功をGPU常駐保証の代替にしない。CPU往復・copy・readbackを明示する

### COLOR-001 HDR/alpha/高解像度品質

- 優先度: P1 / 領域: color / 状態: done
- 依存: GPU-002, RENDER-002, QA-001, JOB-001, MEDIA-002
- 受け入れ条件:
  - HDRの表示変換を最終出力へ勝手に焼き込まない
  - 文字・mask・glowを含む8K offline出力を検証する
  - 基準白203cd/m2(BT.2408)とRec.2100 PQ/HLG出力を検証し、トーンマップは表示と明示的なSDR変換出力に限る
  - native source planeのPQ/HLG・10-bit色tagsを保持してworking-spaceへ変換し、SDR/HDRをタグの付替えや暗黙8-bit化で混同しない
  - HDRの同期export/render.submit profileを固定入力・codec/bit depth・色metadata・probeへ接続し、PQ/HLGのroundtripを実機で検証する。未対応encoder/profileでSDRや別codecへ黙ってfallbackしない

### CACHE-002 temporal/region cacheと無効化

- 優先度: P1 / 領域: cache / 状態: done
- 依存: CACHE-001, RENDER-002
- 受け入れ条件:
  - ROI haloと複数時刻依存をkeyに含める
  - ネストやTimeMap変更が古いフレームを再利用しない

### PERF-001 参照シーンbenchmarkと資源予算

- 優先度: P1 / 領域: performance / 状態: done
- 依存: GPU-003, COLOR-001, CACHE-002, RENDER-003
- 受け入れ条件:
  - warm/cold・proxy/full・preview/finalを分けてp50/p95を出す
  - decode面/atlas/accumulation/encoderを含めたpeak memoryを記録する
  - GPUのlinear/display二重描画の統合とrenderer APIの転送統計集約を評価し、最適化前後の画素一致・実転送/待機・node別tile allocationのpeak memoryを測定する
  - MEDIA-001のstream startからのdecode forwardを正確なseekテストを維持して効率化し、CFR/VFR/B-frameのforward/backward/repeated要求で性能を比較する
  - ADR-0052の合成debug測定と区別して実作品/release buildの履歴復元頻度・遅延・SQLite/WAL物理I/Oを測り、opt-in snapshot・数/容量予算・root patch重複削減の再検討要否を記録する。採用変更が必要なら新ADRで決める
  - ADR-0093の基準機・既定cache・固定基本4K作品で、画素が変わる21要求のwarm native preview p95が33.3ms以下となる。coldは別報告し、動画decode・GUI presentation込みのFPS保証とは区別する

### RECOVERY-001 GPU lost/容量不足/worker停止の復旧

- 優先度: P1 / 領域: reliability / 状態: done
- 依存: JOB-001, GPU-003, STORE-001
- 受け入れ条件:
  - 失敗でProjectや確定済み成果物が壊れない
  - 出力fileへの無条件appendを再開方法に使わない
  - interruptedのジョブをjob.resumeで再開でき、完了済み区間の扱いを検証する
  - SIGKILLが残したdestination側temporary outputを所有jobと固定入力hashで照合して安全に回収し、確定済み成果物や他jobの出力を削除しない
  - atomic NOREPLACE rename後・状態DB commit前の停止を故障注入し、成果物のhash/metadata・snapshot identityとinterrupted記録を照合する。無検証でsucceededへ補正しない
  - 再開時に固定入力の構造/意味版・必要機能・素材/font lockを再検証し、最新Projectへ置換しない。取消・lease失効・容量不足と再開の競合を検証する

### QA-004 Vulkan/Windowsのgolden基準と許容誤差の校正

- 優先度: P1 / 領域: qa / 状態: done
- 依存: QA-003, GPU-003
- 受け入れ条件:
  - Linux(Vulkan)とWindowsの比較環境ごとに基準画像を持ち、明示実行で比較する
  - M4 Mac miniを含む複数のApple Silicon世代で共通基準との差を測り、許容誤差2^-10の妥当性を記録する

### CACHE-003 GPU資源とディスクのraster cache

- 優先度: P2 / 領域: cache / 状態: done
- 依存: CACHE-001, CACHE-002
- 受け入れ条件:
  - GPU textureのcacheを容量予算つきで持ち、意味的keyをCPU側のcacheと共有する
  - プロジェクト外のcache領域へraster結果を永続化し、削除しても描画結果が変わらない
  - GPU texture/surface poolの所有権・寿命・予算とtile/halo再利用を検証し、非対応資源をCPU cacheへ黙って置換しない
  - 厳密raster cacheのbackend/GPU/driver fingerprintと意味版をkeyへ固定し、異なる実行環境の結果を誤再利用しない

### COMP-002 Composition の Media ノード（画像・映像）と MediaSlot の描画

- 優先度: P1 / 領域: render / 状態: done
- 依存: TEMPLATE-002, NLE-002
- 受け入れ条件:
  - Composition の Media ノードを素材参照・source time・色契約つきで描画し、CPU 参照と明示 GPU 実行で検証する
  - TEMPLATE-002 の MediaSlot 入力を最終レンダーへ接続し、素材不足・hash 不一致は型付き失敗にする

### MCP-003 MCP 2026-07-28 版（server/discover・要求ごとの版指定）への対応

- 優先度: P2 / 領域: api / 状態: done
- 依存: MCP-002
- 受け入れ条件:
  - 2026-07-28 版の server/discover と要求ごとの版指定を、2025-11-25 版の initialize と併存させて実装し、同じ Command/Query registry・schema・型付き結果へ到達することを外部 client SDK で検証する
  - 未対応の版や機能を対応済みとして広告しない

### FFI-002 GUI セッション中の安全モード排他の保持

- 優先度: P2 / 領域: gui / 状態: done
- 依存: GUI-001
- 受け入れ条件:
  - 安全モードで開いたプロジェクトについて、GUI を開いている間は他プロセス（CLI / MCP）が PROJECT_LOCKED になるよう、FFI / service の store の寿命を GUI セッションに合わせる
  - GUI を閉じたとき・異常終了したときに lock が解放され、再 open できることを実プロセスで検証する

### MEDIA-003 Windows の FFmpeg 実行時読み込みと CLI/MCP の Windows ビルド

- 優先度: P1 / 領域: media / 状態: done
- 依存: MEDIA-001, JOB-002, RELEASE-001
- 受け入れ条件:
  - kronello-media の native 読み込み（dlfcn / pkg-config 前提）を Windows の LGPL FFmpeg 共有 library に対応させ、CLI / MCP を Windows でビルドする
  - Windows で ProRes / PCM24 と AV1 の書き出し・roundtrip、capabilities を実行し、OS / 版 / コマンド / exit を記録する
  - JOB-002 で jobs / platform 層だけを検証した Windows の detached worker を、実際の `kronello worker --job` で親終了後も継続することを CI で検証する


## M5

### VEC-002 Trim path・morph・SVG対応表

- 優先度: P1 / 領域: vector / 状態: done
- 依存: VEC-001, ANIM-001, QA-001
- 受け入れ条件:
  - morphは点/segment対応を検証し不整合を拒否する
  - SVGのunsupported機能と外部参照を報告する

### REPEAT-001 Repeaterとinstanceごとの制御

- 優先度: P1 / 領域: motion / 状態: done
- 依存: COMP-001, EXPR-001, VEC-002
- 受け入れ条件:
  - Source共有とinstance ID/seedを保持する
  - 個別編集のexpandを明示し元templateを変更しない

### TEXT-002 文字単位selector・ルビ・縦書き

- 優先度: P1 / 領域: text / 状態: done
- 依存: TEXT-001, ANIM-001, LAYOUT-001
- 受け入れ条件:
  - クラスタを壊さない文字演出と親文字/ルビの結合を実装する
  - 再組版時に古いglyph indexへ誤適用しない

### AUDIO-001 音声特徴量DataAssetと連動

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-000, MEDIA-001, EXPR-001, TIME-001
- 受け入れ条件:
  - 分析版/窓/ホップ/時間写像と入力hashを固定する
  - 同じ出力フレームごとに音声全体を再分析しない

### SIM-001 固定刻み・checkpoint・particle基盤

- 優先度: P2 / 領域: simulation / 状態: done
- 依存: EVAL-001, CACHE-002, REPEAT-001
- 受け入れ条件:
  - 順次再生とcheckpointからのseekを固定環境で比較する
  - 入力変更で影響するcheckpointを無効化する

### NAME-001 名称の商標照会・名前確保状況の記録

- 優先度: P1 / 領域: release / 状態: in_progress
- 依存: なし
- 受け入れ条件:
  - OQ-02に従いJ-PlatPat・USPTO・EUIPOの照会結果、対象区分・地域・照会日・未判断の点を記録し、検索結果だけで法的利用可能性を断定しない
  - crates.ioのkronelloと主要crate、ドメインについて所有・利用可否・取得要否を日付付きで確認する。取得・登録等は所有者の判断を記録してから実行する
  - 公開前という旧目安を過ぎた未解決事項として追跡し、採否の決定記録をOQ-02から参照する

### EXPR-002 人間向け式構文とASTの往復・入力UI

- 優先度: P1 / 領域: expression / 状態: done
- 依存: EXPR-001, GUI-001
- 受け入れ条件:
  - 実装着手前にOQ-17の演算子・リテラル・Property参照・診断の構文を決定記録に固定する。ASTを正本としJavaScript互換にしない方針を維持する
  - parser/formatterの往復、型・単位・依存・予算の診断を検証し、既存AST評価と同じ結果を得る
  - GUIの式入力を共有編集APIへ接続し、構文エラー・IME確定・revision競合・Undoを確認する

### AUDIO-005 圧縮音声AAC・Opusの採否と採用profileの検証

- 優先度: P1 / 領域: audio / 状態: done
- 依存: MEDIA-002, AUDIO-003, RELEASE-001
- 受け入れ条件:
  - OQ-21（旧重複番号OQ-19）の採否を先に決定し、AAC-LC・Opusと対応containerごとの保証範囲を明記する。未採用形式は型付き拒否を維持する
  - 採用する場合はLGPL配布構成・encoder品質・priming/padding・pre-skip/codec delay/discard padding・終端sampleとA/V同期を実測し、source manifestとprofileを固定する
  - 採用形式は共有APIの固定snapshot書き出し・roundtripで検証する。採用を見送る場合は理由と代替profileを決定記録に残し、実装済みと扱わない

### MATTE-001 Matte関係の作品モデル・保存・共有編集

- 優先度: P1 / 領域: render / 状態: done
- 依存: RENDER-001, SERVICE-002, GUI-001
- 受け入れ条件:
  - transientなRenderSnapshot.MatteBindingと区別して文書matte関係・ID・版・参照循環・欠落時の意味を設計記録に固定する
  - 共有edit.plan/apply、公開schema、保存再読込、revision・Undoへ接続し、alpha/luminance・invert・外部編集を検証する
  - GUIから同じmatte関係を編集し、CLI/MCPと同じ固定snapshotの合成結果を得る。未知機能を黙って無視しない

### FRAMEBRIDGE-001 generic VideoToolbox経路の意味と診断の整理

- 優先度: P2 / 領域: gpu / 状態: done
- 依存: GPU-003
- 受け入れ条件:
  - PathKind::VideoToolboxの未実装診断と具体的なBgra8/Nv12 decode経路を区別し、generic経路の維持・廃止・具体化の判断を記録する
  - 経路一覧・require_gpu_resident・診断・テストを整合させ、汎用decode/encodeや非対応形式が保証済みに見えないことを確認する

### GUI-007 macOS編集UIの未接続操作を共有APIへ接続

- 優先度: P2 / 領域: gui / 状態: done
- 依存: GUI-003, GUI-004, SERVICE-002
- 受け入れ条件:
  - EditのEffects追加、速度・ソース開始・逆再生・不透明度/合成設定、トラック表示/ミュートについて対応する共有モデル/APIの有無を先に整理し、必要なコア契約を固定する
  - Motionのガイド/スナップ、色・書体/ウェイト・複数Text style spanの編集、およびEditの手のひら操作を実装する。GUI専用の作品状態を作らず、表示だけの状態はUI stateに分離する
  - 実装済み操作だけを有効にし、共有APIとの同等性・Undo・外部変更・競合・IMEと実GUI操作を検証する。既存GUI-001〜004の完了範囲と区別して記録する

### INSPECT-002 render.explainの見積もりを単一graph実行へ整合

- 優先度: P1 / 領域: inspection / 状態: done
- 依存: INSPECT-001, PERF-001
- 受け入れ条件:
  - ADR-0092の単一graph・status readback一回に合わせ、render.explainのDUPLICATE_LINEAR_DISPLAY_RENDER診断と旧二重実行の転送見積もりを修正する
  - 通常GPU・resident・CPU reference・temporal/tileごとの見積もりと実測の境界を示し、固定入力で実行countersとの整合をテストする
  - explainは読み取り専用の計画でありGPUを実行しないことを維持し、native previewの画像readbackなしとfinal readbackを区別する

### EXPR-003 DataAsset参照・過去時刻sample・連続noiseの式拡張

- 優先度: P2 / 領域: expression / 状態: done
- 依存: EXPR-001, AUDIO-001
- 受け入れ条件:
  - AUDIO-001の音声特徴量参照を再利用しつつ、残るDataAsset参照要件、動的な過去Property sample、連続補間noiseの型・時間写像・静的依存と循環拒否・意味版を設計記録に固定する
  - 不変の入力hashと固定seedを使い、外部I/Oや時計を評価へ持ち込まず、命令・メモリ・sample予算を実際の追加処理に適用する
  - 任意時刻・逆順・再試行の決定性、時間境界・資産欠落・循環・予算超過を共有APIと最終renderで検証する。人間向け構文のEXPR-002とは独立にAST契約を検証する


## M7

### NLE-003 精密編集操作: slip/slide/rolling edit・ripple delete・ギャップ処理

- 優先度: P0 / 領域: timeline / 状態: done
- 依存: NLE-002
- 受け入れ条件:
  - slip・slide・rolling edit・ripple delete を sequence モデルと共有 Command/Query API に実装する
  - linked clip（映像・音声）間で編集結果の一貫性を保つ
  - 挿入と上書きの区別・ギャップの生成と除去を型付き操作として扱う
  - CLI/MCP での同一操作の意味的一致をテストで確認する

### NLE-004 シーケンス/クリップマーカー・In/Out・ワークエリア書き出し範囲

- 優先度: P0 / 領域: timeline / 状態: done
- 依存: NLE-002
- 受け入れ条件:
  - sequence/clip マーカーの追加・削除・移動・色・コメントをモデル化する
  - In/Out 点とワークエリアをモデル化し、export の範囲指定として job に渡せる
  - Undo・共有編集・保存でマーカーと In/Out が保持されることを確認する

### SUB-001 字幕モデル: caption track・字幕 clip kind・スタイルと版管理

- 優先度: P0 / 領域: captions / 状態: done
- 依存: NLE-002, TEXT-002
- 受け入れ条件:
  - caption track と字幕 clip kind を sequence モデルと schema に追加する
  - 文字装飾（フォント・色・縁取り・背景・位置）を評価可能な Property として表す
  - 未知の版・未対応機能は型付き拒否とし、版管理の規約に従う

### SUB-002 字幕 I/O: SRT/VTT/ITT 入出力と焼き付け・サイドカー書き出し

- 優先度: P0 / 領域: captions / 状態: done
- 依存: SUB-001, MEDIA-002
- 受け入れ条件:
  - SRT/VTT/ITT のインポート・エクスポートを計画/適用の編集として実装する
  - 字幕クリップの描画（焼き付け）と sidecar 書き出しを export profile に追加する
  - 字幕の時間精度と文字化け・改行処理を fixture で検証する

### COLOR-002 色補正エフェクト基盤: exposure/levels/curves/HSL の versioned effect

- 優先度: P0 / 領域: color / 状態: done
- 依存: COLOR-001, FX-001
- 受け入れ条件:
  - exposure・levels・curves・HSL 補正を versioned effect としてモデルと schema に追加する
  - 線形作業色空間での演算と HDR/SDR での適用範囲を規約として定める
  - CPU/GPU 経路の意味的一致と golden 比較で確認する

### FX-003 ブレンドモード全種とトランジション拡張（wipe/slide/dip 等）

- 優先度: P1 / 領域: effects / 状態: done
- 依存: GPU-002, FX-001
- 受け入れ条件:
  - 標準的なブレンドモード一式を BlendMode に追加し CPU/GPU 両経路で合成する
  - wipe・slide・dip 等のトランジションを TransitionKind に追加する
  - トランジション区間と音声クロスフェードの関係を規約化する

### GUI-008 macOS タイムライン編集 UI: trim ツール・マーカー・In/Out・スナップ

- 優先度: P0 / 領域: gui / 状態: done
- 依存: NLE-003, NLE-004, GUI-007
- 受け入れ条件:
  - slip/slide/rolling の trim ツールと ripple delete を GUI 操作から共有 API に接続する
  - マーカー/In/Out の表示・設定・ジャンプを実装する
  - クリップ端・再生ヘッド・マーカーへのスナップを実装し操作を検証する

### GUI-009 macOS 字幕編集 UI: caption lane と編集パネル

- 優先度: P1 / 領域: gui / 状態: done
- 依存: SUB-001, GUI-003
- 受け入れ条件:
  - caption lane の表示と字幕クリップの編集パネルを実装する
  - スタイル編集がプレビューに即時反映され Undo で戻ることを確認する

### GUI-010 macOS カラー Inspector: 色補正パラメータ編集

- 優先度: P1 / 領域: gui / 状態: done
- 依存: COLOR-002, GUI-003
- 受け入れ条件:
  - 色補正 effect の Inspector 編集 UI を実装する
  - パラメータ変更のプレビュー即時反映と Undo を確認する

### AUDIO-006 クリップ波形の生成・キャッシュとタイムライン描画

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-001, GUI-003
- 受け入れ条件:
  - audio.analyze を用いたクリップ波形の生成とキャッシュを実装する
  - タイムラインへの波形描画とズーム追従を確認する

### INTEGRATION-003 縦断デモ第3段階: カット編集+字幕+簡易グレーディング作品

- 優先度: P1 / 領域: integration / 状態: done
- 依存: NLE-003, SUB-002, COLOR-002, GUI-008
- 受け入れ条件:
  - カット編集・字幕・色補正を含む作品を GUI と CLI の両経路で生成する
  - 書き出し結果の映像・音声・字幕を受け入れ基準として検証する


## M8

### NLE-005 トラックロック・クリップ有効/無効・トラックターゲティング

- 優先度: P1 / 領域: timeline / 状態: done
- 依存: NLE-003
- 受け入れ条件:
  - track lock・clip enable/disable・track targeting をモデルと共有 API に追加する
  - GUI のトラック制御操作を接続し Undo を確認する

### NLE-006 可変リタイム（スピードランプ）と freeze frame/hold

- 優先度: P1 / 領域: timeline / 状態: done
- 依存: NLE-003
- 受け入れ条件:
  - TimeMap の曲線化によるスピードランプと freeze frame/hold を実装する
  - 音声の AudioRetimePolicy との整合と型付き拒否を検証する

### COLOR-003 LUT (.cube) の取り込み・検証・適用

- 優先度: P1 / 領域: color / 状態: done
- 依存: COLOR-002
- 受け入れ条件:
  - .cube 3D LUT の取り込み・検証・作品内参照を実装する
  - HDR/SDR 変換パイプラインでの適用位置を規約として定める

### COLOR-004 スコープ: waveform/vectorscope/histogram/RGB parade と GUI パネル

- 優先度: P1 / 領域: color / 状態: done
- 依存: COLOR-002, GUI-010
- 受け入れ条件:
  - waveform・vectorscope・histogram・RGB parade の生成を実装する
  - macOS GUI のスコープパネルを実装し表示を検証する

### FX-004 ベジェマスク: クリップ適用・フェザー・拡張・時間変化

- 優先度: P1 / 領域: effects / 状態: done
- 依存: VEC-001, MATTE-001
- 受け入れ条件:
  - ベジェマスクのクリップ適用・フェザー・拡張・時間変化をモデル化する
  - GUI でのベジェ編集を共有 API に接続する

### FX-005 キーイング: chroma key/luma key とスピル抑制

- 優先度: P1 / 領域: effects / 状態: done
- 依存: COLOR-002
- 受け入れ条件:
  - chroma key・luma key を versioned effect として実装する
  - スピル抑制・エッジ調整の基本パラメータを含める

### FX-006 標準エフェクト拡張: glow/sharpen/vignette/warp 等

- 優先度: P1 / 領域: effects / 状態: done
- 依存: FX-003
- 受け入れ条件:
  - glow・sharpen・vignette・warp 等の標準エフェクトを versioned effect として追加する
  - CPU/GPU 経路の意味的一致を確認する

### FX-007 アジャストメントクリップ: 下位レイヤーへのエフェクト一括適用

- 優先度: P2 / 領域: timeline / 状態: done
- 依存: FX-004
- 受け入れ条件:
  - adjustment clip kind をモデル化し下位レイヤーへのエフェクト適用を実装する
  - 範囲・入れ子・トラック制約を型付きで扱う

### AUDIO-007 EQ/フィルタ系音声エフェクト: パラメトリックEQ・HPF/LPF

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-003
- 受け入れ条件:
  - パラメトリックEQ・HPF/LPF 等のフィルタ系音声エフェクトを実装する
  - リアルタイム再生と書き出しの一致を確認する

### AUDIO-008 ダイナミクスと LUFS 計測・ノーマライズ

- 優先度: P1 / 領域: audio / 状態: done
- 依存: AUDIO-007
- 受け入れ条件:
  - compressor/limiter を音声エフェクトとして実装する
  - LUFS 計測とターゲットノーマライズを実装し検証する

### AUDIO-009 音声 UI: VU メーター・オーディオスクラブ・ミキサーパネル

- 優先度: P2 / 領域: audio / 状態: done
- 依存: AUDIO-006, GUI-008
- 受け入れ条件:
  - VU メーター・オーディオスクラブ・ミキサーパネルを macOS GUI に実装する

### TRACK-001 モーショントラッキング: 点/平面トラッキングと結果の DataAsset 化

- 優先度: P2 / 領域: tracking / 状態: done
- 依存: COMP-002
- 受け入れ条件:
  - 点/平面モーショントラッキングの解析を実装する
  - 結果を Property/DataAsset として参照可能にし式・エフェクトへ接続する

### VEC-006 テキスト拡張: テキストオンパスと文字単位アニメーター

- 優先度: P1 / 領域: text / 状態: done
- 依存: TEXT-002
- 受け入れ条件:
  - テキストオンパスの描画と layout を実装する
  - 文字単位アニメーター（範囲・ランダム・追従）を実装する

### PROXY-001 プロキシワークフロー: プロキシ生成 job・編集時切替・relink

- 優先度: P1 / 領域: media / 状態: done
- 依存: MEDIA-002, JOB-001
- 受け入れ条件:
  - プロキシ生成 job と編集時のプロキシ参照切替を実装する
  - 書き出しでは元素材を用いることを検証する

### FLOW-001 ワークフロー UI: ショートカット設定・レイアウト・Undo 履歴パネル

- 優先度: P2 / 領域: gui / 状態: done
- 依存: GUI-008
- 受け入れ条件:
  - キーボードショートカットのカスタマイズを実装する
  - ワークスペースレイアウトの保存と Undo 履歴パネルを実装する

### INTEGRATION-005 縦断デモ（M8 仕上げ検証）: リタイム+マスク+LUT+音声処理を含む作品

- 優先度: P1 / 領域: integration / 状態: done
- 依存: NLE-006, FX-004, FX-007, COLOR-003, AUDIO-008, PROXY-001
- 受け入れ条件:
  - 可変リタイム・freeze・マスク・アジャストメントクリップ・LUT・音声エフェクト・プロキシ切替を含む作品を CLI と MCP の両経路で生成する
  - 書き出し結果の映像・音声・型付き拒否を受け入れ基準として検証する


## M9

### TRACK-002 手ブレ補正（スタビライズ）

- 優先度: P2 / 領域: tracking / 状態: planned
- 依存: TRACK-001
- 受け入れ条件:
  - トラッキング結果を用いたスタビライズを実装する
  - クロップ・補間・境界処理のパラメータを含める

### TRACK-003 オプティカルフロー補間: 滑らかなスローモーション

- 優先度: P2 / 領域: tracking / 状態: planned
- 依存: TRACK-001, NLE-006
- 受け入れ条件:
  - オプティカルフローによる中間フレーム補間を実装する
  - リタイムとの組み合わせと失敗時の型付き拒否を検証する

### AUDIO-010 ピッチ保持リタイムとマルチチャンネル（5.1ch 等）

- 優先度: P2 / 領域: audio / 状態: planned
- 依存: AUDIO-008
- 受け入れ条件:
  - ピッチを保持する音声リタイムを実装する
  - 2ch 超のマルチチャンネル入出力とダウンミックスを実装する

### GUI-011 ソース/プログラム二画面モニタ

- 優先度: P2 / 領域: gui / 状態: planned
- 依存: GUI-008
- 受け入れ条件:
  - ソースモニタとプログラムモニタの二画面編集 UI を実装する
  - インサート/オーバーライト編集操作を接続する

### AI-001 音声文字起こし: ローカル推論と字幕トラック連携

- 優先度: P2 / 領域: ai / 状態: planned
- 依存: SUB-001, AUDIO-001
- 受け入れ条件:
  - ローカル推論による音声文字起こしを実装する
  - 結果を字幕トラックへ反映し信頼度・単語時間を保持する

### AI-002 シーン検出: カット境界の自動検出

- 優先度: P2 / 領域: ai / 状態: planned
- 依存: MEDIA-001
- 受け入れ条件:
  - 映像のシーン境界を自動検出する job を実装する
  - 検出結果をマーカー/クリップ分割へ適用できる

### AI-003 スマートリフレーム: 被写体追従の自動縦横比変換

- 優先度: P2 / 領域: ai / 状態: planned
- 依存: TRACK-001, LAYOUT-001
- 受け入れ条件:
  - 被写体追従による自動リフレームを実装する
  - responsive layout と組み合わせて検証する

### NLE-007 マルチカム編集: 同期・切替・マルチカムクリップ

- 優先度: P2 / 領域: timeline / 状態: planned
- 依存: NLE-005
- 受け入れ条件:
  - マルチカムクリップの同期・角度切替編集をモデルと共有 API に実装する
  - GUI のマルチカム操作を接続する

### FLOW-002 メディア管理 UI: ビン/ブラウザ/オフラインメディア

- 優先度: P2 / 領域: gui / 状態: planned
- 依存: FLOW-001
- 受け入れ条件:
  - ビン・メディアブラウザ・サムネイル一覧を実装する
  - オフラインメディアの表示と relink 操作を接続する

### FLOW-003 書き出し運用: ウォッチフォルダ・プリセット・バッチキュー

- 優先度: P2 / 領域: media / 状態: planned
- 依存: MEDIA-002, JOB-002
- 受け入れ条件:
  - 書き出しプリセット・バッチキュー・ウォッチフォルダを実装する

### AUDIO-011 VST3/AU プラグインホスティング

- 優先度: P2 / 領域: audio / 状態: planned
- 依存: AUDIO-009
- 受け入れ条件:
  - VST3/AU プラグインのホスティングと信頼境界を実装する

### INTEGRATION-004 縦断デモ第4段階: AI 補助を含む編集ワークフロー

- 優先度: P2 / 領域: integration / 状態: planned
- 依存: AI-001, GUI-011
- 受け入れ条件:
  - 文字起こしから字幕・編集までの一連のワークフローを検証する


## M10

### MEDIA-004 配信拡張: チャプター・マルチ出力・追加コーデック profile

- 優先度: P2 / 領域: media / 状態: planned
- 依存: AUDIO-005
- 受け入れ条件:
  - チャプターマーカー・マルチ出力・DNxHD/GIF/MP3 等の profile を追加する

### IO-001 外部モニタ出力: SDI/NDI 等のリファレンス出力

- 優先度: P2 / 領域: io / 状態: planned
- 依存: GPU-003
- 受け入れ条件:
  - 外部モニタ出力経路を実装し色精度を検証する

### COLLAB-001 共同編集: 複数ユーザーの同時編集

- 優先度: P2 / 領域: service / 状態: planned
- 依存: STORE-003, SERVICE-001
- 受け入れ条件:
  - 複数ユーザーの同時編集と競合解決のモデルを実装する

### FX-008 残エフェクト群の拡充

- 優先度: P2 / 領域: effects / 状態: planned
- 依存: FX-006
- 受け入れ条件:
  - 未実装の標準エフェクトを棚卸しし versioned effect として追加する

### FLOW-004 キャプチャ/インジェスト: 録画・デッキ取り込み

- 優先度: P2 / 領域: media / 状態: planned
- 依存: MEDIA-001
- 受け入れ条件:
  - 録画・デッキ取り込みの経路を実装する

### MEDIA-005 カメラフォーマット: RAW 系（BRAW/R3D/ProRes RAW）

- 優先度: P2 / 領域: media / 状態: planned
- 依存: MEDIA-001
- 受け入れ条件:
  - カメラ RAW 系フォーマットのデコードと色管理を実装する

### GUI-012 ワークスペース完成: 残 UI の統合と UX ポリッシュ

- 優先度: P2 / 領域: gui / 状態: planned
- 依存: GUI-011, FLOW-002
- 受け入れ条件:
  - 残る UI 要素を統合しワークスペースとして完成させる


## M11

### DIM-001 2.5D Scene3D境界とカメラ

- 優先度: P2 / 領域: 3d / 状態: planned
- 依存: COLOR-001, COMP-001, RENDER-002
- 受け入れ条件:
  - 2D描画順と3D depthを別仕様として持つ
  - Color/Maskと必要な補助ポートを型検証する

### INTEROP-001 OTIO/SVG/外部レンダーの交換アダプター

- 優先度: P2 / 領域: interop / 状態: planned
- 依存: NLE-001, VEC-002, JOB-001
- 受け入れ条件:
  - 保持/変換/欠落した機能をレポートする
  - 完全互換でない表現のbakeを明示承認にする

### PLUGIN-001 有界WASM拡張と信頼区分

- 優先度: P2 / 領域: plugins / 状態: planned
- 依存: EXPR-001, RENDER-001, API-001
- 受け入れ条件:
  - CPU fuel/host call/メモリ/外部権限を別々に制限する
  - WASM制限がGPU処理時間を保証しないことを診断設計へ反映する

### DIST-001 レンダーファームとsimulation bake境界

- 優先度: P2 / 領域: distributed / 状態: planned
- 依存: SIM-001, JOB-001, RECOVERY-001
- 受け入れ条件:
  - 素材/フォント/engine versionsをworker間で照合する
  - 未ベイクstateful区間を任意frameへ自由分割しない

### GUI-005 WindowsネイティブGUIとプレビュー面の接続

- 優先度: P2 / 領域: gui / 状態: planned
- 依存: FFI-002, GUI-004
- 受け入れ条件:
  - ADR-0032のWinUI 3で共有C ABIとCommand/Queryを使い、対象OSのsurface受け渡し・所有権・同期・resize/closeを実環境で検証する
  - Edit/Motion/Template/Exportの共有デザインとUI state分離を実装し、revision通知・競合・Undo・安全モード排他をCLI/MCPと照合する
  - 日本語IME・アクセシビリティ・キーボード操作・再生停止・例外時の終了を実GUIで確認し、対象OS/adapter/版と保証範囲を記録する

### GUI-006 LinuxネイティブGUIとプレビュー面の接続

- 優先度: P2 / 領域: gui / 状態: planned
- 依存: FFI-002, GUI-004
- 受け入れ条件:
  - ADR-0032のGTK4で共有C ABIとCommand/Queryを使い、対象OSのsurface受け渡し・所有権・同期・resize/closeを実環境で検証する
  - Edit/Motion/Template/Exportの共有デザインとUI state分離を実装し、revision通知・競合・Undo・安全モード排他をCLI/MCPと照合する
  - 日本語IME・アクセシビリティ・キーボード操作・再生停止・例外時の終了を実GUIで確認し、対象OS/adapter/版と保証範囲を記録する

### RELEASE-002 macOS製品署名・notarization・実ダウンロード検証

- 優先度: P1 / 領域: release / 状態: planned
- 依存: RELEASE-001, GUI-004, NAME-001
- 受け入れ条件:
  - 開発用ad-hoc署名と区別してDeveloper IDの署名・hardened runtime・entitlements・notarizationを製品package全体で検証する
  - 配布形式を確定し、対象に適したstaple・Gatekeeper・quarantineを保持した実ダウンロードから初回起動・FFmpeg/helper起動まで確認する
  - LGPL再リンク可能性・license/source manifest・package hash・署名者・検証環境・コマンドとexitを記録し、資格情報を含めない

### RELEASE-003 Windows製品packageとクリーン環境での配布検証

- 優先度: P2 / 領域: release / 状態: planned
- 依存: RELEASE-001, NAME-001, GUI-005
- 受け入れ条件:
  - Windowsの配布形式・署名/検証方法を定め、GUI・CLI/MCP・workerとLGPL runtimeの依存・license/source manifestを固定する
  - 開発環境の絶対path/system libraryに依存せず、再配置・実ダウンロード・インストール/起動/削除をクリーン環境で検証する
  - 対応OS/architecture・runtime版・package hash・検証コマンド/exitを記録し、CI内のビルド成功だけで配布可能と判定しない

### RELEASE-004 Linux製品packageとクリーン環境での配布検証

- 優先度: P2 / 領域: release / 状態: planned
- 依存: RELEASE-001, NAME-001, GUI-006
- 受け入れ条件:
  - Linuxの配布形式・署名/検証方法を定め、GUI・CLI/MCP・workerとLGPL runtimeの依存・license/source manifestを固定する
  - 開発環境の絶対path/system libraryに依存せず、再配置・実ダウンロード・インストール/起動/削除をクリーン環境で検証する
  - 対応OS/architecture・runtime版・package hash・検証コマンド/exitを記録し、CI内のビルド成功だけで配布可能と判定しない

### GPU-004 WindowsのGPU常駐media/render経路の昇格

- 優先度: P2 / 領域: gpu / 状態: planned
- 依存: GPU-003, MEDIA-003
- 受け入れ条件:
  - Windowsで保証するadapter/API/codec/pixel formatを設計記録に固定し、hardware decodeからrenderまでのdevice・同期・所有権・寿命を実機で確認する
  - CPU upload/readback・GPU copy・transfer countersを測定し、software golden成功をhardware常駐保証の代替にしない
  - 固定入力の画素/PTS・seek・再利用/解放・異常系を比較し、非対応環境は明示software選択またはrequire_gpu_residentの型付き拒否を維持する

### GPU-005 LinuxのGPU常駐media/render経路の昇格

- 優先度: P2 / 領域: gpu / 状態: planned
- 依存: GPU-003, MEDIA-001
- 受け入れ条件:
  - Linuxで保証するadapter/API/codec/pixel formatを設計記録に固定し、hardware decodeからrenderまでのdevice・同期・所有権・寿命を実機で確認する
  - CPU upload/readback・GPU copy・transfer countersを測定し、software golden成功をhardware常駐保証の代替にしない
  - 固定入力の画素/PTS・seek・再利用/解放・異常系を比較し、非対応環境は明示software選択またはrequire_gpu_residentの型付き拒否を維持する

### GPU-006 macOS常駐デコードの追加形式・色精度の保証評価

- 優先度: P2 / 領域: gpu / 状態: planned
- 依存: GPU-003, COLOR-001
- 受け入れ条件:
  - GPU-003で未保証のHDR/10-bit/full-range/HEVC hev1について形式別の採否と必要なmetadata/変換を設計記録に固定する
  - 採用する形式はVideoToolboxからMetalの所有権・同期・PTS・色精度と転送量を実機で測定し、native software経路と比較する
  - 未採用または未検証の形式は型付き拒否を維持し、8-bit SDRの既存保証やCOLOR-001のsoftware HDRと混同しない

### INTEROP-002 上級交換: AAF/XML/EDL 等のプロジェクト交換

- 優先度: P2 / 領域: interop / 状態: planned
- 依存: INTEROP-001
- 受け入れ条件:
  - AAF/XML/EDL 等の交換フォーマットの入出力を実装する

### DIM-002 完全 3D の検討: スコープ評価と段階的導入計画

- 優先度: P2 / 領域: 3d / 状態: planned
- 依存: DIM-001
- 受け入れ条件:
  - 完全 3D のスコープ評価と段階的導入計画を ADR として定める
