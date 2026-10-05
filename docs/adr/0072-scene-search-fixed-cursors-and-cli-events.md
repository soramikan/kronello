# ADR-0072: Scene 検索・固定 revision cursor・CLI event framing

状態: 採用
日付: 2026-10-05
対象: API-002

## 背景

Scene の runtime identity と所有順序を保持して検索・ページ取得する必要がある。
従来の history.list の next_since_revision は次回の最新状態を読むため、ページ間に
Undo が入ると undone が別の revision の状態になる。CLI の一要求一 JSON を維持しながら、
進捗と協調取消を機械が識別できる framing も必要である。

## 決定

### Scene と tags

SceneNode.tags は sorted set の任意フィールド。既定は空、空なら保存時に省略する。
公開 Project schema version は1を維持し、旧ファイルの省略を受け入れる。
各 tag は非空、NFC 正規形、制御文字なし、先頭末尾の空白なし、UTF-8 64 bytes 以下、
一 node 当たり32個以下。正本を暗黙に正規化しない。不適合は既存 validation error family
（node_tags_set は INVALID_REQUEST、Project 外枠は INVALID_MUTATION）で拒否する。
tags はデータであり、NodeId / InstancePath の代わりに identity として使わない。

共有 EditCommand に node_tags_set {composition,node,tags} を追加し、集合全体を置換する。
edit.plan / apply / undo の hash、revision、receipt、選択 Undo を使う。
node 自身の構造キーで競合を検出する。raw patch や入口専用状態は公開しない。

scene.query の search は {tags,kinds,range?}。tags は入力を NFC に正規化した後の
case-sensitive な all-of、kinds は閉じた SceneKind enum の any-of（空なら全種類）。
range は正規化した有理数の非空 [start,end) と、各 node の mandatory active_range の
半開区間 overlap。Instance 内でもその node が属する Composition の authored local time
で比較する。active_range の省略や保存モデルの変更は追加しない。
評価時刻・retime を適用した可視性検索ではない。可視性の説明は INSPECT-001 の node.explain を使う。

検索は全 scene の既存 containment pre-order に対する選択で、node collection の保存順や
表示名による並べ替えをしない。roots、parents、children は元の完全な関係を保持し、
検索結果 / page 外の key も参照しうる。expand_instances と明示 evaluation を維持する。
評価は同じ固定 snapshot の全 scene に対して従来の経路で行い、検索によって失敗を隠さない。
limit は省略時に従来の全件、指定時1..=1000。全展開の100000 node 上限は維持する。

### 固定 cursor

両 query の cursor / next_cursor は opaque string。クライアントは解釈・編集せず、そのまま
再送する。内部 version、operation、Project UUID、snapshot revision、正規化した query parameters、
最後の stable key を束ねた stateless continuation とし、checksum で破損を検出する。
これは認証 credential ではなく、暗号化・偽造防止・アクセス制御を提供しない。
別 process / CLI 起動 / MCP 接続でも、同じ Project と同じ parameters で再開できる。
サーバー cache、暗黙の current Project、配列 index identity、時計による失効は導入しない。

scene は最後の {instance_path,node} から owner order を継続し、snapshot_at で元 revision を復元する。
binding は composition / expand_instances / evaluation（time と font locators）/ search / limit。
history は revision / Event UUID 順で、binding は since_revision / limit / session_id。
cursor 再送時も since_revision は初回と同じ値を送る。next_since_revision は互換用に維持するが、
これだけを送る取得は新しい最新 snapshot を開始し、固定取得を保証しない。

history は一つの SQLite read transaction から文書 identity / revision と履歴を取得し、
cursor revision 以下の全イベントから undone を計算してから session filter / page を適用する。
初回の最古の Event UUID も cursor に固定し、compaction で prefix が失われたら期限切れにする。
新しい edit / Undo / Redo を混ぜず、欠落を補って黙って続けない。

不正・破損・未対応 version は INVALID_CURSOR、Project / operation / parameters の変更は
CURSOR_MISMATCH、snapshot または必要な history prefix の削除は CURSOR_EXPIRED。
期限切れなら初回要求から取得し直す。履歴の既定全保持・明示 compact、選択 Undo の意味は
ADR-0026 / ADR-0030 を維持する。

### CLI NDJSON version 1

--events ndjson を明示した一要求だけに適用する。通常 CLI は従来どおり Response JSON 一つと LF。
stream は UTF-8、各行が完全な JSON object、区切り LF、各 record を flush する。
最初は {record:header,version:1,sequence:0}。
次に0個以上の {record:progress,sequence,completed,total}。
最後は一つの {record:terminal,sequence,outcome,response}。
sequence は record ごとに1増加、outcome は end / cancelled / error、response は既存共有 Response。
公開 API schema に CliEvent / CliEventOutcome を含めるが、作品操作を registry に追加しない。
progress は ExecutionControl が報告した frame 数であり、永続 job の subscription ではない。
diagnostic は stderr のみ。end は exit 0、cancelled / error / output failure は非0。

SIGINT は stdin 読み取り待ちも終了させ、dispatch 前、sequence の frame 境界と manifest 公開前で
協調取消する。単一 frame / native codec / SQLite transaction は途中で強制停止しない。
commit 済み編集、checkpoint 後に確定した出力、投入済み detached job を取消で rollback しない。
完了が先に確定した race は end を返す。SIGTERM / SIGKILL の graceful terminal は保証しない。
stdout write failure は OUTPUT_IO_ERROR を stderr に出し、取消 flag を立て、非0で終了する。
閉じた stdout には terminal を届けられないため、receiver は terminal のない EOF を不完全な stream と扱う。
stream replay / resume / live watch は提供しない。固定 query pages は query cursor で再取得する。

## 影響と検証

registry は既存36操作のまま。全入口が同じ Request / Result / EditCommand と公開 schema を使う。
追加の model field のため、Rust SceneNode literal に空 tags を追加し、両 schema と Swift binding を再生成する。
評価 backend・renderer・store table は変更しない。
[API-002 検証](../testing/api-002.md) に条件ごとの実行証拠と host 未実行範囲を記録する。
