#!/usr/bin/env python3
"""Vendor the full Lucide set and generate the SwiftUI icon table.

Reads third_party/lucide/icons/*.svg (24x24 stroke icons) and writes
apps/macos/Sources/KronelloDesign/Generated/Icons.swift. Every outline is
reduced to absolute moves, lines, cubic Béziers, ellipses and rounded
rectangles (arcs and quadratic curves become cubics) and stored as a compact
path string that KRIcon decodes at draw time.

`KRIcon.documented` lists the icons named in docs/design-system/icons.md, in
table order; generation fails if the table names an icon that is not vendored.

Usage:
    gen_lucide_icons.py            # regenerate Icons.swift
    gen_lucide_icons.py --check    # fail when Icons.swift is out of date
    gen_lucide_icons.py --vendor   # replace third_party/lucide with every icon of the pinned lucide-static
"""

from __future__ import annotations

import io
import math
import re
import shutil
import sys
import tarfile
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "third_party" / "lucide" / "icons"
OUT = ROOT / "apps" / "macos" / "Sources" / "KronelloDesign" / "Generated" / "Icons.swift"
ICON_DOC = ROOT / "docs" / "design-system" / "icons.md"
NS = "{http://www.w3.org/2000/svg}"

TOKEN = re.compile(r"[A-Za-z]|-?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?")


def fmt(v: float) -> str:
    s = f"{v:.4f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def op(code: str, *values: float) -> str:
    return " ".join([code, *(fmt(v) for v in values)])


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
    # Last control point of the previous curve, for the S / T reflections.
    last_cubic = last_quad = None
    ops: list[str] = []

    def num():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    def flag():
        # Arc flags are single digits and may be packed with what follows ("011.5").
        nonlocal i
        tok = toks[i]
        if len(tok) > 1:
            toks[i] = tok[1:]
        else:
            i += 1
        return int(tok[0])

    def quad(qx, qy, ex, ey):
        # A quadratic segment as the equivalent cubic.
        c1 = (x + 2 / 3 * (qx - x), y + 2 / 3 * (qy - y))
        c2 = (ex + 2 / 3 * (qx - ex), ey + 2 / 3 * (qy - ey))
        ops.append(op("C", *c1, *c2, ex, ey))

    while i < len(toks):
        if re.fullmatch(r"[A-Za-z]", toks[i]):
            cmd = toks[i]
            i += 1
        rel = cmd.islower()
        c = cmd.upper()
        cubic = quadratic = None
        if c == "Z":
            ops.append("Z")
            x, y = sx, sy
        elif c == "M":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            x, y, sx, sy = nx, ny, nx, ny
            ops.append(op("M", x, y))
            cmd = "l" if rel else "L"
        elif c == "L":
            nx, ny = num(), num()
            if rel:
                nx, ny = x + nx, y + ny
            x, y = nx, ny
            ops.append(op("L", x, y))
        elif c in "HV":
            v = num()
            if c == "H":
                x = x + v if rel else v
            else:
                y = y + v if rel else v
            ops.append(op("L", x, y))
        elif c in "CS":
            if c == "C":
                v = [num() for _ in range(6)]
                if rel:
                    v = [v[0] + x, v[1] + y, v[2] + x, v[3] + y, v[4] + x, v[5] + y]
            else:
                v = [num() for _ in range(4)]
                if rel:
                    v = [v[0] + x, v[1] + y, v[2] + x, v[3] + y]
                rx1, ry1 = (2 * x - last_cubic[0], 2 * y - last_cubic[1]) if last_cubic else (x, y)
                v = [rx1, ry1, *v]
            ops.append(op("C", *v))
            cubic = (v[2], v[3])
            x, y = v[4], v[5]
        elif c in "QT":
            if c == "Q":
                qx, qy, ex, ey = (num() for _ in range(4))
                if rel:
                    qx, qy, ex, ey = qx + x, qy + y, ex + x, ey + y
            else:
                ex, ey = num(), num()
                if rel:
                    ex, ey = ex + x, ey + y
                qx, qy = (2 * x - last_quad[0], 2 * y - last_quad[1]) if last_quad else (x, y)
            quad(qx, qy, ex, ey)
            quadratic = (qx, qy)
            x, y = ex, ey
        elif c == "A":
            rx, ry, rot = num(), num(), num()
            fa, fs = flag(), flag()
            ex, ey = num(), num()
            if rel:
                ex, ey = x + ex, y + ey
            for seg in arc_to_cubics(x, y, rx, ry, rot, int(fa), int(fs), ex, ey):
                ops.append(op("L", seg[1], seg[2]) if seg[0] == "L" else op("C", *seg[1:]))
            x, y = ex, ey
        else:
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
        last_cubic, last_quad = cubic, quadratic
    return ops


def points(text: str) -> list[float]:
    return [float(v) for v in TOKEN.findall(text)]


def element_ops(el: ET.Element) -> list[str]:
    tag = el.tag.replace(NS, "")
    a = el.attrib
    g = lambda k, dflt=0.0: float(a.get(k, dflt))
    if tag == "path":
        return path_ops(a["d"])
    if tag == "circle":
        cx, cy, r = g("cx"), g("cy"), g("r")
        return [op("E", cx - r, cy - r, 2 * r, 2 * r)]
    if tag == "ellipse":
        cx, cy, rx, ry = g("cx"), g("cy"), g("rx"), g("ry")
        return [op("E", cx - rx, cy - ry, 2 * rx, 2 * ry)]
    if tag == "line":
        return [op("M", g("x1"), g("y1")), op("L", g("x2"), g("y2"))]
    if tag in ("polyline", "polygon"):
        v = points(a["points"])
        ops = [op("M", v[0], v[1])] + [op("L", v[k], v[k + 1]) for k in range(2, len(v) - 1, 2)]
        return ops + (["Z"] if tag == "polygon" else [])
    if tag == "rect":
        x, y, w, h = g("x"), g("y"), g("width"), g("height")
        rx = g("rx", a.get("ry", 0))
        ry = g("ry", a.get("rx", 0))
        return [op("R", x, y, w, h, rx, ry)]
    raise ValueError(f"unsupported element {tag}")


def case_name(stem: str) -> str:
    parts = stem.split("-")
    name = parts[0]
    for part in parts[1:]:
        # Keep digit groups apart: arrow-down-1-0 → arrowDown1_0, not arrow-down-10's arrowDown10.
        name += ("_" if name[-1].isdigit() and part[:1].isdigit() else "") + part[:1].upper() + part[1:]
    return f"`{name}`" if name in {"repeat", "type", "import", "case", "default", "subscript"} else name


def documented_icons(available: set[str]) -> list[str]:
    """Icon names from the usage table of icons.md, in first-use order."""
    names: list[str] = []
    for line in ICON_DOC.read_text().splitlines():
        if not line.startswith("|") or line.startswith("|---"):
            continue
        cells = line.strip("|").split("|")
        if len(cells) < 2:
            continue
        for name in re.findall(r"`([a-z0-9-]+)`", cells[1]):
            if name.startswith("kind-") or name in ("danger",):
                continue
            if name not in available:
                raise SystemExit(f"icons.md names {name!r}, which is not in {ICONS.relative_to(ROOT)}")
            if name not in names:
                names.append(name)
    return names


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
        bodies.append(f'        case .{name.strip("`")}: "{" ".join(ops)}"')
    documented = documented_icons({f.stem for f in files})
    listed = ", ".join("." + case_name(n).strip("`") for n in documented)
    return f'''// Generated by scripts/gen_lucide_icons.py from third_party/lucide (lucide-static {version}, ISC). Do not edit.

import CoreGraphics
import SwiftUI

/// Every Lucide icon. Pick one for a new use from this set and record the use in
/// docs/design-system/icons.md; `documented` lists the icons that table names.
public enum KRIcon: String, CaseIterable, Sendable {{
{chr(10).join(cases)}
}}

extension KRIcon {{
    /// The icons named in docs/design-system/icons.md, in table order.
    public static let documented: [KRIcon] = [{listed}]

    /// The icon's outline in Lucide's 24×24 coordinate space.
    public var path: Path {{ KRIconPath.decode(encodedPath) }}

    /// Absolute `M` / `L` / `C` / `Z` commands plus `E` (ellipse in x y w h) and
    /// `R` (rounded rect in x y w h rx ry), space separated.
    var encodedPath: String {{
        switch self {{
{chr(10).join(bodies)}
        }}
    }}
}}
'''


def cmd_vendor() -> int:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from design_preview import LUCIDE, download  # pinned URL and SHA-256 shared with the preview

    data = download(LUCIDE["url"], LUCIDE["sha256"], f"lucide-static-{LUCIDE['version']}.tgz")
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        members = [m for m in archive.getmembers()
                   if m.isfile() and re.fullmatch(r"package/icons/[a-z0-9-]+\.svg", m.name)]
        if not members:
            raise SystemExit("no icons in the lucide-static archive")
        if ICONS.exists():
            shutil.rmtree(ICONS)
        ICONS.mkdir(parents=True)
        for member in members:
            (ICONS / Path(member.name).name).write_bytes(archive.extractfile(member).read())
        (ICONS.parent / "LICENSE").write_bytes(archive.extractfile("package/LICENSE").read())
    print(f"vendored {len(members)} icons from lucide-static {LUCIDE['version']}")
    return 0


def main() -> int:
    if "--vendor" in sys.argv:
        cmd_vendor()
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
