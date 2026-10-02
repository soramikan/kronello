# ADR-0001: GUI / CLI / MCP は同じ Command / Query API を使う

- 状態: 採用（v0.2 仕様から継承。実装による検証は未了）
- 日付: 2026-10-01

## 背景

人間が GUI で行う編集と、スクリプトや AI エージェントが CLI / MCP で行う編集が別の経路を通ると、入口ごとに作品状態や編集の意味がずれる。

## 決定

- すべての入口は同じ Command API / Query API を通る。
- GUI 専用の作品状態を作らない。選択や pan / zoom などの UI 状態は作品から分離する。
- cli / mcp / ffi は service の薄いアダプターとし、編集の意味を持たない。

## 影響

- 同じ編集操作は入口によらず同じ revision / event に到達する（QA-002 で検証）。
- GUI の操作も型付きコマンドとして表現できなければならず、GUI 実装の自由度は下がる。

## 関連

- [08 API・CLI・MCP](../architecture/08-api-cli-mcp.md)
- [10 デスクトップ GUI](../architecture/10-desktop-gui.md)
