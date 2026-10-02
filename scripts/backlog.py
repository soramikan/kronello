#!/usr/bin/env python3
"""Validate docs/backlog/backlog.json and render docs/backlog/BACKLOG.md.

Usage:
    python3 scripts/backlog.py check    # validate only
    python3 scripts/backlog.py render   # validate, then regenerate BACKLOG.md
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs" / "backlog" / "backlog.json"
OUTPUT = ROOT / "docs" / "backlog" / "BACKLOG.md"

MILESTONES = ["M0", "M1", "M2", "M3", "M4", "M5", "M6"]
PRIORITIES = ["P0", "P1", "P2"]
STATUSES = ["planned", "in_progress", "done", "dropped"]
REQUIRED = ["id", "milestone", "priority", "area", "title", "status", "dependencies", "acceptance_criteria"]


def validate(data):
    errors = []
    items = data["items"]
    by_id = {}
    for item in items:
        missing = [k for k in REQUIRED if k not in item]
        if missing:
            errors.append(f"{item.get('id', '?')}: missing fields {missing}")
            continue
        if item["id"] in by_id:
            errors.append(f"{item['id']}: duplicate id")
        by_id[item["id"]] = item
        if item["milestone"] not in MILESTONES:
            errors.append(f"{item['id']}: unknown milestone {item['milestone']}")
        if item["priority"] not in PRIORITIES:
            errors.append(f"{item['id']}: unknown priority {item['priority']}")
        if item["status"] not in STATUSES:
            errors.append(f"{item['id']}: unknown status {item['status']}")
        if not item["acceptance_criteria"]:
            errors.append(f"{item['id']}: no acceptance criteria")

    if data.get("task_count") != len(items):
        errors.append(f"task_count is {data.get('task_count')} but there are {len(items)} items")

    for item in by_id.values():
        for dep in item["dependencies"]:
            if dep not in by_id:
                errors.append(f"{item['id']}: unknown dependency {dep}")
            elif MILESTONES.index(by_id[dep]["milestone"]) > MILESTONES.index(item["milestone"]):
                errors.append(f"{item['id']} ({item['milestone']}) depends on later {dep} ({by_id[dep]['milestone']})")
            elif item["status"] == "done" and by_id[dep]["status"] != "done":
                errors.append(f"{item['id']} is done but dependency {dep} is {by_id[dep]['status']}")

    # Cycle detection (depth-first, three-colour).
    state = {}

    def visit(node, path):
        if state.get(node) == "done" or node not in by_id:
            return
        if state.get(node) == "visiting":
            errors.append("dependency cycle: " + " -> ".join(path + [node]))
            return
        state[node] = "visiting"
        for dep in by_id[node]["dependencies"]:
            visit(dep, path + [node])
        state[node] = "done"

    for node in by_id:
        visit(node, [])
    return errors


def render(data):
    items = data["items"]
    lines = [
        "# バックログ一覧",
        "",
        "<!-- このファイルは scripts/backlog.py render が生成する。直接編集しない。正本は backlog.json。 -->",
        "",
        f"- schema_version: {data['schema_version']}",
        f"- 更新日: {data.get('updated', data['created'])}",
        f"- タスク数: {len(items)}",
        "",
        "## 集計",
        "",
        "| マイルストーン | " + " | ".join(STATUSES) + " | 計 |",
        "|---|" + "---:|" * (len(STATUSES) + 1),
    ]
    for ms in MILESTONES:
        group = [i for i in items if i["milestone"] == ms]
        counts = [sum(1 for i in group if i["status"] == s) for s in STATUSES]
        lines.append(f"| {ms} | " + " | ".join(str(c) for c in counts) + f" | {len(group)} |")

    for ms in MILESTONES:
        group = [i for i in items if i["milestone"] == ms]
        if not group:
            continue
        lines += ["", f"## {ms}", ""]
        for item in group:
            deps = ", ".join(item["dependencies"]) or "なし"
            lines += [
                f"### {item['id']} {item['title']}",
                "",
                f"- 優先度: {item['priority']} / 領域: {item['area']} / 状態: {item['status']}",
                f"- 依存: {deps}",
                "- 受け入れ条件:",
            ]
            lines += [f"  - {ac}" for ac in item["acceptance_criteria"]]
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else "check"
    if command not in ("check", "render"):
        print(__doc__, file=sys.stderr)
        return 2
    data = json.loads(SOURCE.read_text(encoding="utf-8"))
    errors = validate(data)
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    if command == "render":
        OUTPUT.write_text(render(data), encoding="utf-8")
        print(f"wrote {OUTPUT.relative_to(ROOT)}")
    else:
        print(f"ok: {len(data['items'])} tasks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
