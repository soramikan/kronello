# ADR-0131: VST3/AU プラグインホスティングと信頼境界

- 状態: 採用
- 日付: 2026-10-08

## 背景

AUDIO-011 は VST3/AU プラグインのホスティングと信頼境界を
要求する。VST3 SDK は GPL/proprietary デュアルライセンス
であり配布物に含められない。またプラグインを UI/service
プロセスへ直接ロードするとクラッシュ・非決定性・版不一致が
作品状態へ漏れる。

## 決定

- プラグインは必ず detached worker プロセスでロードし、
  UI・service・レンダー worker 本体へ `dlopen` しない。
  worker は既存 JOB-002 系のプロセス隔離・heartbeat・
  recovery 規則に従う。クラッシュ・タイムアウト・応答
  不一致は `PLUGIN_FAILED` 系の型付きエラーで返す。
- macOS の AU は AudioToolbox 経由の helper binary で
  ホストする。VST3 は SDK コードを使わず、公開された ABI
  （COM 互換 vtable 契約）を自前実装した loader で扱う。
  サードパーティ SDK のソース・ヘッダを vendoring しない。
- プラグイン bundle は hash-pinned 入力とする。作品へ
  bundle を保存せず、参照と版を記録する。未指定・hash 不
  一致・非対応 ABI は `PLUGIN_MISSING` / `ASSET_HASH_MISMATCH` /
  `UNSUPPORTED_FEATURE` の型付きエラー。
- 検証は in-repo の決定的 test plugin（同じ ABI を実装する
  固定ゲイン等）で host→load→process→unload の roundtrip を
  行う。実製品プラグインとの互換性は本タスクの保証外。
