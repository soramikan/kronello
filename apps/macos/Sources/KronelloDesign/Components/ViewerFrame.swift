import SwiftUI

/// Selection bounds in normalized frame coordinates with a preformatted bounds label.
public struct KRViewerSelection: Equatable, Sendable {
    public var rect: CGRect
    public var label: String
    public init(_ rect: CGRect, label: String) { self.rect = rect; self.label = label }
}

/// A checkerboard frame that fits both available dimensions. Content supplies the preview surface.
public struct KRViewerFrame<Content: View>: View {
    @Environment(\.krPalette) private var p
    public let aspectRatio: CGFloat
    public let selection: KRViewerSelection?
    private let content: Content
    public init(aspectRatio: CGFloat = 16 / 9, selection: KRViewerSelection? = nil, @ViewBuilder content: () -> Content) {
        precondition(aspectRatio.isFinite && aspectRatio > 0)
        self.aspectRatio = aspectRatio; self.selection = selection; self.content = content()
    }
    /// Resolves the maximum fitting frame without overflowing width or height.
    public static func fittingSize(container: CGSize, aspectRatio: CGFloat) -> CGSize {
        guard aspectRatio.isFinite && aspectRatio > 0 else { return .zero }
        let width = min(max(0, container.width), max(0, container.height) * aspectRatio)
        return CGSize(width: width, height: width / aspectRatio)
    }
    public var body: some View {
        GeometryReader { proxy in
            let size = Self.fittingSize(container: proxy.size, aspectRatio: aspectRatio)
            ZStack {
                KRCheckerboard()
                content.frame(width: size.width, height: size.height).clipped()
                if let selection { KRViewerBounds(selection: selection, size: size) }
            }.frame(width: size.width, height: size.height)
                .overlay { Rectangle().strokeBorder(p.line, lineWidth: 1) }
                .position(x: proxy.size.width / 2, y: proxy.size.height / 2)
        }
    }
}

extension KRViewerFrame where Content == EmptyView {
    public init(aspectRatio: CGFloat = 16 / 9, selection: KRViewerSelection? = nil) {
        self.init(aspectRatio: aspectRatio, selection: selection) { EmptyView() }
    }
}

private struct KRCheckerboard: View {
    @Environment(\.krPalette) var p
    var body: some View {
        Canvas { context, size in
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(p.surface200))
            // The CSS tile is 16px, with four 8px quadrants.
            for row in 0..<Int(ceil(size.height / 8)) {
                for column in 0..<Int(ceil(size.width / 8)) where (row + column) % 2 == 1 {
                    context.fill(Path(CGRect(x: column * 8, y: row * 8, width: 8, height: 8)), with: .color(p.controlHover))
                }
            }
        }.accessibilityHidden(true)
    }
}

private struct KRViewerBounds: View {
    @Environment(\.krPalette) var p
    let selection: KRViewerSelection
    let size: CGSize
    var body: some View {
        let rect = CGRect(x: selection.rect.minX * size.width, y: selection.rect.minY * size.height,
                          width: selection.rect.width * size.width, height: selection.rect.height * size.height)
        ZStack(alignment: .topLeading) {
            Rectangle().stroke(p.selection, lineWidth: 1).frame(width: rect.width, height: rect.height).offset(x: rect.minX, y: rect.minY)
            ForEach(0..<8, id: \.self) { index in
                let point = points(rect)[index]
                Rectangle().fill(p.surface200).frame(width: 7, height: 7)
                    .overlay { Rectangle().strokeBorder(p.selection, lineWidth: 1) }.position(point)
            }
            Text(selection.label).krText(KRType.ruler).foregroundStyle(p.selection).offset(x: rect.minX, y: rect.maxY + KRSpace.space2)
        }.frame(width: size.width, height: size.height, alignment: .topLeading).allowsHitTesting(false)
    }
    private func points(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.midX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
         CGPoint(x: r.minX, y: r.midY), CGPoint(x: r.maxX, y: r.midY),
         CGPoint(x: r.minX, y: r.maxY), CGPoint(x: r.midX, y: r.maxY), CGPoint(x: r.maxX, y: r.maxY)]
    }
}
