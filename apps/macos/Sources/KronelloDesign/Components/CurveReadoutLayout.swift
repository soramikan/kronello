import SwiftUI

/// The readout stays in graph space, beyond the measured value-axis gutter.
/// Prefer the right of the playhead; flip to its left near the right edge.
public enum KRCurveReadoutLayout {
    public static func frame(graph: CGSize, playheadX: CGFloat, axisWidth: CGFloat, readout: CGSize) -> CGRect {
        let gutter = min(max(KRSpace.space2, axisWidth + KRSpace.space2), max(KRSpace.space2, graph.width - KRSpace.space2))
        let width = min(readout.width, max(0, graph.width - KRSpace.space2 - gutter))
        let right = playheadX + KRSpace.space2
        let candidate = right + width <= graph.width - KRSpace.space2 ? right : playheadX - KRSpace.space2 - width
        let x = min(max(gutter, candidate), max(gutter, graph.width - KRSpace.space2 - width))
        return CGRect(x: x, y: KRSpace.space2, width: width, height: min(readout.height, max(0, graph.height - KRSpace.space2 * 2)))
    }
}
