#!/usr/bin/env python3
"""Generate SwiftUI shapes for the vendored Lucide icons.

Reads third_party/lucide/icons/*.svg (24x24 stroke icons) and writes
apps/macos/Sources/KronelloDesign/Generated/Icons.swift. Elliptical arcs are
converted to cubic Béziers here so the Swift side only needs lines, curves,
ellipses and rounded rectangles.

Usage: gen_lucide_icons.py [--check]
"""

from __future__ import annotations

import math
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "third_party" / "lucide" / "icons"
OUT = ROOT / "apps" / "macos" / "Sources" / "KronelloDesign" / "Generated" / "Icons.swift"
NS = "{http://www.w3.org/2000/svg}"

TOKEN = re.compile(r"[A-Za-z]|-?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?")


def fmt(v: float) -> str:
    s = f"{v:.4f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def pt(x: float, y: float) -> str:
    return f"CGPoint(x: {fmt(x)}, y: {fmt(y)})"


def arc_to_cubics(x1, y1, rx, ry, phi_deg, fa, fs, x2, y2):
    """SVG 1.1 F.6.5 endpoint-to-center conversion, split into <=90° cubic segments."""
    if (x1, y1) == (x2, y2):
        return []
    rx, ry = abs(rx), abs(ry)
    if rx == 0 or ry == 0:
        return [("L", x2, y2)]
    phi = math.radians(phi_deg)
    cp, sp = math.cos(phi), math.sin(phi)
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p, y1p = cp * dx + sp * dy, -sp * dx + cp * dy
    lam = (x1p / rx) ** 2 + (y1p / ry) ** 2
    if lam > 1:
        s = math.sqrt(lam)
        rx, ry = rx * s, ry * s
    num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
    den = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    coef = math.sqrt(max(0.0, num / den)) if den else 0.0
    if fa == fs:
        coef = -coef
    cxp, cyp = coef * rx * y1p / ry, -coef * ry * x1p / rx
    cx = cp * cxp - sp * cyp + (x1 + x2) / 2
    cy = sp * cxp + cp * cyp + (y1 + y2) / 2

    def ang(ux, uy, vx, vy):
        a = math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        return a

    t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not fs and dt > 0:
        dt -= 2 * math.pi
    elif fs and dt < 0:
        dt += 2 * math.pi
    n = max(1, math.ceil(abs(dt) / (math.pi / 2) - 1e-9))
    step = dt / n
    k = 4 / 3 * math.tan(step / 4)
    out = []
    t = t1
    for _ in range(n):
        c1, s1 = math.cos(t), math.sin(t)
        c2, s2 = math.cos(t + step), math.sin(t + step)
        p1 = (c1 - k * s1, s1 + k * c1)
        p2 = (c2 + k * s2, s2 - k * c2)
        p3 = (c2, s2)

        def m(p):
            x, y = p[0] * rx, p[1] * ry
            return cp * x - sp * y + cx, sp * x + cp * y + cy

        a, b, c = m(p1), m(p2), m(p3)
        out.append(("C", a[0], a[1], b[0], b[1], c[0], c[1]))
        t += step
    return out


def path_ops(d: str) -> list[str]:
    toks = TOKEN.findall(d)
    i, cmd = 0, None
    x = y = sx = sy = 0.0
    ops: list[str] = []

    def num():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    while i < len(toks):
        if re.fullmatch(r"[A-Za-z]", toks[i]):
            cmd = toks[i]
            i += 1
        rel = cmd.islower()
        c = cmd.upper()
        if c == "Z":
            ops.append("p.closeSubpath()")
            x, y = sx, sy
            continue
        if c == "M":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            x, y, sx, sy = nx, ny, nx, ny
            ops.append(f"p.move(to: {pt(x, y)})")
            cmd = "l" if rel else "L"
        elif c == "L":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            x, y = nx, ny
            ops.append(f"p.addLine(to: {pt(x, y)})")
        elif c == "H":
            nx = num()
            x = x + nx if rel else nx
            ops.append(f"p.addLine(to: {pt(x, y)})")
        elif c == "V":
            ny = num()
            y = y + ny if rel else ny
            ops.append(f"p.addLine(to: {pt(x, y)})")
        elif c == "C":
            v = [num() for _ in range(6)]
            if rel:
                v = [v[0] + x, v[1] + y, v[2] + x, v[3] + y, v[4] + x, v[5] + y]
            ops.append(f"p.addCurve(to: {pt(v[4], v[5])}, control1: {pt(v[0], v[1])}, control2: {pt(v[2], v[3])})")
            x, y = v[4], v[5]
        elif c == "A":
            rx, ry, rot, fa, fs, ex, ey = (num() for _ in range(7))
            if rel:
                ex, ey = x + ex, y + ey
            for seg in arc_to_cubics(x, y, rx, ry, rot, int(fa), int(fs), ex, ey):
                if seg[0] == "L":
                    ops.append(f"p.addLine(to: {pt(seg[1], seg[2])})")
                else:
                    _, a, b, cc, dd, e, f = seg
                    ops.append(f"p.addCurve(to: {pt(e, f)}, control1: {pt(a, b)}, control2: {pt(cc, dd)})")
            x, y = ex, ey
        else:
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
    return ops


def element_ops(el: ET.Element) -> list[str]:
    tag = el.tag.replace(NS, "")
    a = el.attrib
    g = lambda k, dflt=0.0: float(a.get(k, dflt))
    if tag == "path":
        return path_ops(a["d"])
    if tag == "circle":
        cx, cy, r = g("cx"), g("cy"), g("r")
        return [f"p.addEllipse(in: CGRect(x: {fmt(cx - r)}, y: {fmt(cy - r)}, width: {fmt(2 * r)}, height: {fmt(2 * r)}))"]
    if tag == "line":
        return [f"p.move(to: {pt(g('x1'), g('y1'))})", f"p.addLine(to: {pt(g('x2'), g('y2'))})"]
    if tag == "rect":
        x, y, w, h = g("x"), g("y"), g("width"), g("height")
        rx = g("rx", a.get("ry", 0))
        ry = g("ry", a.get("rx", 0))
        rect = f"CGRect(x: {fmt(x)}, y: {fmt(y)}, width: {fmt(w)}, height: {fmt(h)})"
        if rx or ry:
            return [f"p.addRoundedRect(in: {rect}, cornerSize: CGSize(width: {fmt(rx)}, height: {fmt(ry)}))"]
        return [f"p.addRect({rect})"]
    raise ValueError(f"unsupported element {tag}")


def case_name(stem: str) -> str:
    parts = stem.split("-")
    name = parts[0] + "".join(p[:1].upper() + p[1:] for p in parts[1:])
    return f"`{name}`" if name in {"repeat", "type", "import", "case", "default"} else name


def generate() -> str:
    files = sorted(ICONS.glob("*.svg"))
    if not files:
        raise SystemExit(f"no icons in {ICONS}")
    version = "unknown"
    cases, bodies = [], []
    for f in files:
        text = f.read_text()
        m = re.search(r"lucide-static v([\d.]+)", text)
        if m:
            version = m.group(1)
        root = ET.fromstring(re.sub(r"<!--.*?-->", "", text, flags=re.S))
        ops: list[str] = []
        for el in root:
            ops.extend(element_ops(el))
        name = case_name(f.stem)
        cases.append(f'    case {name} = "{f.stem}"')
        body = "\n".join(f"            {o}" for o in ops)
        bodies.append(f"        case .{name.strip('`')}:\n{body}")
    return f'''// Generated by scripts/gen_lucide_icons.py from third_party/lucide (lucide-static {version}, ISC). Do not edit.

import CoreGraphics
import SwiftUI

/// The Lucide icons Kronello uses (docs/design-system/icons.md).
public enum KRIcon: String, CaseIterable, Sendable {{
{chr(10).join(cases)}
}}

extension KRIcon {{
    /// The icon's outline in Lucide's 24×24 coordinate space.
    public var path: Path {{
        var p = Path()
        switch self {{
{chr(10).join(bodies)}
        }}
        return p
    }}
}}
'''


def main() -> int:
    out = generate()
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text() != out:
            print(f"{OUT.relative_to(ROOT)} is out of date; run scripts/gen_lucide_icons.py", file=sys.stderr)
            return 1
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(out)
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
