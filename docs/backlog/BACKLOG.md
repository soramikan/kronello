# バックログ一覧

<!-- このファイルは scripts/backlog.py render が生成する。直接編集しない。正本は backlog.json。 -->

- schema_version: 0.5
- 更新日: 2026-10-03
- タスク数: 60

## 集計

| マイルストーン | planned | in_progress | done | dropped | 計 |
|---|---:|---:|---:|---:|---:|
| M0 | 0 | 0 | 6 | 0 | 6 |
| M1 | 0 | 0 | 14 | 0 | 14 |
| M2 | 11 | 0 | 0 | 0 | 11 |
| M3 | 12 | 0 | 0 | 0 | 12 |
| M4 | 8 | 0 | 0 | 0 | 8 |
| M5 | 5 | 0 | 0 | 0 | 5 |
| M6 | 4 | 0 | 0 | 0 | 4 |

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

- 優先度: P0 / 領域: media / 状態: planned
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

- 優先度: P0 / 領域: audio / 状態: planned
- 依存: TIME-001, MEDIA-001
- 受け入れ条件:
  - 素材音声を48kHzへ変換しクリップ音量を適用してBusへミックスする
  - 音声サンプル位置を絶対時刻から計算し、映像との同期をサンプル精度で検証する
  - 書き出しで映像と音声を同じ固定snapshotからmuxする

### NLE-001 CompositionClipとマルチトラック統合

- 優先度: P0 / 領域: timeline / 状態: planned
- 依存: COMP-001, MEDIA-001, RENDER-001
- 受け入れ条件:
  - 同じCompositionを異なる長さ/時刻で複数配置する
  - trim/stretch/instance retimeを別操作としてテストする

### FX-001 基本エフェクト: drop shadow / gaussian blur

- 優先度: P0 / 領域: effects / 状態: planned
- 依存: GPU-002, RENDER-001
- 受け入れ条件:
  - エフェクトが必要な入力領域(ROI halo)を宣言しvisual_boundsへ反映する
  - premultiplied alphaと線形作業空間でのshadow/blurをgolden testで検証する

### SERVICE-001 計画・適用・競合・冪等性

- 優先度: P0 / 領域: service / 状態: planned
- 依存: STORE-001, CLI-001
- 受け入れ条件:
  - base_revision/plan_hash/idempotency_keyを検証する
  - 同じキー同じpayloadは重複適用せず異なるpayloadは拒否する
  - idempotency_keyと適用結果をプロジェクト内に保存し、別プロセスからの再送にも同一結果を返す
  - edit.undoが対象イベントの逆操作を新しいrevisionとして発行し、後続の未取り消しイベントが同じキーに触れていればUNDO_CONFLICTで何も適用せず拒否する
  - 値変更は(オブジェクト,Property)単位、構造変更はオブジェクトと親コンテナ単位で競合を判定する

### TEMPLATE-001 公開入力と保護時間区間の最小template

- 優先度: P0 / 領域: template / 状態: planned
- 依存: COMP-001, SERVICE-001
- 受け入れ条件:
  - 定義とinstance入力を分離し版を固定する
  - 5秒から8秒にしてintro/outroの長さが変わらない
  - テキストlayout_boundsへの単方向参照で背景帯が追従し、max_lines超過をoverflowとして検出する

### API-001 共通schemaとquery/command公開

- 優先度: P0 / 領域: api / 状態: planned
- 依存: SERVICE-001, PROP-001, EVAL-001
- 受け入れ条件:
  - scene query/property sample/capabilitiesを構造化結果で返す
  - APIの任意シェル/外部URL実行を禁止する
  - history.listでイベント・session・変更したキー・取り消し状態を返す

### MCP-001 MCPアダプター

- 優先度: P0 / 領域: mcp / 状態: planned
- 依存: API-001
- 受け入れ条件:
  - 交渉したprotocol versionでschemaとstructuredContentを返す
  - クライアント接続状態に暗黙の対象Projectを保持しない

### JOB-001 固定snapshot書き出しjob

- 優先度: P0 / 領域: jobs / 状態: planned
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

- 優先度: P0 / 領域: integration / 状態: planned
- 依存: NLE-001, TEMPLATE-001, MCP-001, JOB-001, AUDIO-000, FX-001
- 受け入れ条件:
  - CLIで作成しMCPで確認して固定snapshotから4K出力する
  - 別instanceの文字/色/長さが干渉しない
  - 5秒から8秒への尺変更と別テキストで、背景帯追従・overflow検出・基本shadowを検証する

### STORE-003 保存の運用検証と適応的snapshot

- 優先度: P2 / 領域: storage / 状態: planned
- 依存: STORE-002
- 受け入れ条件:
  - patchの累計が文書サイズを超えたときにも完全snapshotを取る方式を評価し、採否を記録する
  - iCloud Drive・Dropbox等の実際の同期フォルダとネットワークファイルシステムで、安全モードとPROJECT_LOCKEDを手順で確認して記録する
  - Linux/Windowsで複数プロセスの競合と強制終了後の回復を確認する


## M3

### EXPR-001 型付きASTと有界式評価

- 優先度: P1 / 領域: expression / 状態: planned
- 依存: EVAL-001, API-001
- 受け入れ条件:
  - 静的依存列挙と命令/メモリ/サンプル予算を実装する
  - 固定seedのnoiseと禁止機能の拒否をテストする
  - ASTを式の正本とし、将来の式言語と一対一に往復できる構造にする

### LAYOUT-001 responsive layoutとbounds段階

- 優先度: P1 / 領域: layout / 状態: planned
- 依存: TEXT-001, VEC-001, EVAL-001
- 受け入れ条件:
  - layout/ink/visual boundsを区別する
  - 文字幅と背景幅の循環およびoverflowを診断する

### TEMPLATE-002 長さ・縦横比variant・data入力・版移行

- 優先度: P1 / 領域: template / 状態: planned
- 依存: TEMPLATE-001, LAYOUT-001
- 受け入れ条件:
  - 短尺拒否/hold/loop/stretchを明示する
  - 版更新の差分計画とプレビューを生成し勝手に既存作品を更新しない

### GUI-001 macOSネイティブGUI: Canvas・階層・変換操作

- 優先度: P1 / 領域: gui / 状態: planned
- 依存: FFI-001, RENDER-001
- 受け入れ条件:
  - GUI操作が共通command/eventを使う
  - 選択・pan/zoomなどUI状態を作品から分離する
  - CLI/MCPなど外部プロセスによるrevision変化を検知して再読込する
  - GUIのUndoは自セッションの操作だけを取り消し、競合時は理由を表示する
  - UI状態をユーザーごとの状態領域に保存し、.kronelloへ書き込まない

### GUI-002 Dope sheetとCurve editor

- 優先度: P1 / 領域: gui / 状態: planned
- 依存: GUI-001, ANIM-001
- 受け入れ条件:
  - キー移動/接線編集/UndoがCLIで読める同じモデルを変更する
  - 空間パスと時間イージングを区別して表示する

### AUDIO-002 リアルタイム音声再生とA/V同期

- 優先度: P1 / 領域: audio / 状態: planned
- 依存: AUDIO-000, GUI-001
- 受け入れ条件:
  - 音声コールバックをプロジェクト更新・ディスク読み出し・式評価と別の実行系にする
  - プレビュー再生で音声クロックを基準に映像フレームを提示する

### INSPECT-001 非表示原因・依存・レンダー経路のexplain

- 優先度: P1 / 領域: inspection / 状態: planned
- 依存: API-001, LAYOUT-001, CACHE-001
- 受け入れ条件:
  - opacity/active range/parent/mask/asset不足を要因別に返す
  - 過大処理と転送/メモリ/キャッシュを構造化して表示する

### FFI-001 kronello-ffi: ネイティブGUI向けCommand/Query境界

- 優先度: P1 / 領域: ffi / 状態: planned
- 依存: API-001
- 受け入れ条件:
  - SwiftからCommand/Query APIを呼び、CLIと同じrevision/eventへ到達する
  - ネイティブ側のCAMetalLayerをwgpu surfaceとして受け取りプレビューを表示する
  - FFI境界にwgpu/SQLite/Tokioの型を露出しない
  - C ABIの関数は少数に保ち、Command/QueryはCLI/MCPと同じJSONで受け渡す
  - Swiftの型を公開JSON Schemaから生成する
  - 高頻度経路でのJSON直列化コストを計測する

### QA-002 GUI/CLI/MCP同等性と日本語IME

- 優先度: P1 / 領域: test / 状態: planned
- 依存: GUI-002, MCP-001, INTEGRATION-001
- 受け入れ条件:
  - 同じ編集操作の結果snapshotが一致する
  - 未確定IME文字列を作品履歴へ大量commitしない

### INTEGRATION-002 縦断デモ第2段階: GUI閲覧と縦型variant

- 優先度: P1 / 領域: integration / 状態: planned
- 依存: INTEGRATION-001, TEMPLATE-002, GUI-001
- 受け入れ条件:
  - 第1段階と同じプロジェクトをmacOS GUIで開き、CLI/MCPと同じ値・layout boundsを表示する
  - 同じtemplate定義を縦型variantで再利用し、再レイアウト結果を検証する

### VEC-004 グラデーションの拡張

- 優先度: P2 / 領域: vector / 状態: planned
- 依存: VEC-003
- 受け入れ条件:
  - repeat/reflectのspreadを実装する
  - 焦点付き放射(焦点位置・焦点半径)と円錐(sweep)グラデーションを実装する
  - グラデーションごとに補間空間(sRGB・straight等)を明示指定でき、補間空間の意味の版を管理する
  - 図形のbounding box基準の座標とgradient transformを扱う
  - テキストのfillにグラデーションを適用し、組版クラスタを壊さない
  - SDR 8bit出力のbanding対策(dither)の要否を判断し、採用する場合は固定seedで決定的にする

### VEC-005 線の拡張: 破線・線の位置・非一様変換

- 優先度: P2 / 領域: vector / 状態: planned
- 依存: VEC-003
- 受け入れ条件:
  - 破線(dash配列・offset)とoffsetのアニメーションを実装する
  - 線の位置(中央・内側・外側)を指定できる
  - 非一様scale/skew下の線幅の意味を定義し、現状のUNSUPPORTED_FEATUREを解消する


## M4

### RENDER-002 時間サンプルと高品質モーションブラー

- 優先度: P1 / 領域: render / 状態: planned
- 依存: RENDER-001, CACHE-001, ANIM-001
- 受け入れ条件:
  - サブ時刻ごとの全体合成を基準に比較する
  - カット境界/ネスト/露光位相/重複サンプルを扱う

### GPU-003 各OSのFrameBridge保証経路

- 優先度: P1 / 領域: gpu / 状態: planned
- 依存: GPU-001, GPU-002, MEDIA-001
- 受け入れ条件:
  - macOS (Metal / VideoToolbox) を最初の保証経路とし、Windows/Linuxは順次昇格する
  - 対応形式ごとにデバイス/所有権/同期/寿命を検証する
  - 非対応経路は明示fallbackまたはrequire_gpu_residentエラー

### COLOR-001 HDR/alpha/高解像度品質

- 優先度: P1 / 領域: color / 状態: planned
- 依存: GPU-002, RENDER-002, QA-001
- 受け入れ条件:
  - HDRの表示変換を最終出力へ勝手に焼き込まない
  - 文字・mask・glowを含む8K offline出力を検証する
  - 基準白203cd/m2(BT.2408)とRec.2100 PQ/HLG出力を検証し、トーンマップは表示と明示的なSDR変換出力に限る

### CACHE-002 temporal/region cacheと無効化

- 優先度: P1 / 領域: cache / 状態: planned
- 依存: CACHE-001, RENDER-002
- 受け入れ条件:
  - ROI haloと複数時刻依存をkeyに含める
  - ネストやTimeMap変更が古いフレームを再利用しない

### PERF-001 参照シーンbenchmarkと資源予算

- 優先度: P1 / 領域: performance / 状態: planned
- 依存: GPU-003, COLOR-001, CACHE-002
- 受け入れ条件:
  - warm/cold・proxy/full・preview/finalを分けてp50/p95を出す
  - decode面/atlas/accumulation/encoderを含めたpeak memoryを記録する

### RECOVERY-001 GPU lost/容量不足/worker停止の復旧

- 優先度: P1 / 領域: reliability / 状態: planned
- 依存: JOB-001, GPU-003, STORE-001
- 受け入れ条件:
  - 失敗でProjectや確定済み成果物が壊れない
  - 出力fileへの無条件appendを再開方法に使わない
  - interruptedのジョブをjob.resumeで再開でき、完了済み区間の扱いを検証する

### QA-004 Vulkan/Windowsのgolden基準と許容誤差の校正

- 優先度: P1 / 領域: qa / 状態: planned
- 依存: QA-003, GPU-003
- 受け入れ条件:
  - Linux(Vulkan)とWindowsの比較環境ごとに基準画像を持ち、明示実行で比較する
  - M4 Mac miniを含む複数のApple Silicon世代で共通基準との差を測り、許容誤差2^-10の妥当性を記録する

### CACHE-003 GPU資源とディスクのraster cache

- 優先度: P2 / 領域: cache / 状態: planned
- 依存: CACHE-001, CACHE-002
- 受け入れ条件:
  - GPU textureのcacheを容量予算つきで持ち、意味的keyをCPU側のcacheと共有する
  - プロジェクト外のcache領域へraster結果を永続化し、削除しても描画結果が変わらない


## M5

### VEC-002 Trim path・morph・SVG対応表

- 優先度: P1 / 領域: vector / 状態: planned
- 依存: VEC-001, ANIM-001, QA-001
- 受け入れ条件:
  - morphは点/segment対応を検証し不整合を拒否する
  - SVGのunsupported機能と外部参照を報告する

### REPEAT-001 Repeaterとinstanceごとの制御

- 優先度: P1 / 領域: motion / 状態: planned
- 依存: COMP-001, EXPR-001, VEC-002
- 受け入れ条件:
  - Source共有とinstance ID/seedを保持する
  - 個別編集のexpandを明示し元templateを変更しない

### TEXT-002 文字単位selector・ルビ・縦書き

- 優先度: P1 / 領域: text / 状態: planned
- 依存: TEXT-001, ANIM-001, LAYOUT-001
- 受け入れ条件:
  - クラスタを壊さない文字演出と親文字/ルビの結合を実装する
  - 再組版時に古いglyph indexへ誤適用しない

### AUDIO-001 音声特徴量DataAssetと連動

- 優先度: P1 / 領域: audio / 状態: planned
- 依存: AUDIO-000, MEDIA-001, EXPR-001, TIME-001
- 受け入れ条件:
  - 分析版/窓/ホップ/時間写像と入力hashを固定する
  - 同じ出力フレームごとに音声全体を再分析しない

### SIM-001 固定刻み・checkpoint・particle基盤

- 優先度: P2 / 領域: simulation / 状態: planned
- 依存: EVAL-001, CACHE-002, REPEAT-001
- 受け入れ条件:
  - 順次再生とcheckpointからのseekを固定環境で比較する
  - 入力変更で影響するcheckpointを無効化する


## M6

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
