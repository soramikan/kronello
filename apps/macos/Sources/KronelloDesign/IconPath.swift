import SwiftUI

/// Decodes the compact outlines written by scripts/gen_lucide_icons.py: absolute
/// `M x y`, `L x y`, `C x1 y1 x2 y2 x y`, `Z`, `E x y w h` (ellipse) and
/// `R x y w h rx ry` (rounded rectangle), separated by spaces.
enum KRIconPath {
    static func decode(_ encoded: String) -> Path {
        var p = Path()
        var command: Substring = ""
        var values: [CGFloat] = []
        func flush() {
            switch command {
            case "M" where values.count == 2: p.move(to: CGPoint(x: values[0], y: values[1]))
            case "L" where values.count == 2: p.addLine(to: CGPoint(x: values[0], y: values[1]))
            case "C" where values.count == 6:
                p.addCurve(to: CGPoint(x: values[4], y: values[5]),
                           control1: CGPoint(x: values[0], y: values[1]), control2: CGPoint(x: values[2], y: values[3]))
            case "Z": p.closeSubpath()
            case "E" where values.count == 4: p.addEllipse(in: CGRect(x: values[0], y: values[1], width: values[2], height: values[3]))
            case "R" where values.count == 6:
                let rect = CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
                if values[4] == 0 && values[5] == 0 { p.addRect(rect) }
                else { p.addRoundedRect(in: rect, cornerSize: CGSize(width: values[4], height: values[5])) }
            default: break
            }
            values.removeAll(keepingCapacity: true)
        }
        for token in encoded.split(separator: " ") {
            if let first = token.first, first.isLetter {
                flush(); command = token
            } else if let value = Double(token) {
                values.append(CGFloat(value))
            }
        }
        flush()
        return p
    }
}
