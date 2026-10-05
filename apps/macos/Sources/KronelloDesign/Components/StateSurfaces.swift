import SwiftUI
import AppKit

public struct KRStateBand: View {
    @Environment(\.krPalette) private var p
    public let message: String
    public let details: (() -> Void)?
    public init(_ message: String, details: (() -> Void)? = nil) { self.message = message; self.details = details }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            KRIconView(.info); Text(message).krText(KRType.body); Spacer(minLength: 0)
            if let details { KRButton("詳細…", variant: .plain, action: details) }
        }
            .foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3).frame(height: KRWindowMetrics.safeBand)
            .background(p.surface200).overlay(alignment: .bottom) { p.line.frame(height: 1) }
    }
}

public struct KRConflictBanner: View {
    @Environment(\.krPalette) private var p
    public let diagnostic: KRDiagnostic
    public let discard: () -> Void
    public let reapply: () -> Void
    public init(_ diagnostic: KRDiagnostic, discard: @escaping () -> Void, reapply: @escaping () -> Void) {
        self.diagnostic = diagnostic; self.discard = discard; self.reapply = reapply
    }
    public var body: some View {
        HStack(spacing: KRSpace.space3) {
            KRErrorLine(diagnostic)
            Spacer(minLength: 0)
            KRButton("破棄", variant: .plain, action: discard)
            KRButton("もう一度適用", variant: .secondary, action: reapply)
        }.padding(KRSpace.space3).background(p.surface200).krShadow(p.shadowPopover, cornerRadius: 0)
    }
}

public struct KRViewerError: View {
    @Environment(\.krPalette) private var p
    public let diagnostic: KRDiagnostic
    public let copy: () -> Void
    public let retry: () -> Void
    public init(_ diagnostic: KRDiagnostic, copy: @escaping () -> Void, retry: @escaping () -> Void) {
        self.diagnostic = diagnostic; self.copy = copy; self.retry = retry
    }
    public var body: some View {
        VStack(spacing: KRSpace.space3) {
            KRIconView(.triangleAlert, size: 24).foregroundStyle(p.danger)
            KRErrorLine(diagnostic)
            HStack(spacing: KRSpace.space2) {
                KRButton("詳細をコピー", variant: .secondary, action: copy)
                KRButton("再試行", variant: .secondary, action: retry)
            }
        }.padding(KRSpace.space4).frame(maxWidth: .infinity, maxHeight: .infinity).background(p.surface0)
            .overlay { Rectangle().strokeBorder(p.danger, style: StrokeStyle(lineWidth: 1, dash: [4, 4])) }
    }
}

/// Reusable interactive bounds. Geometry and command transactions belong to the view model.
public struct KRManipulationOverlay: View {
    @Environment(\.krPalette) private var p
    public let selection: KRViewerSelection
    public let onPreview: (CGSize, Int?, Bool) -> Void
    public let onCommit: (CGSize, Int?, Bool) -> Void
    public init(_ selection: KRViewerSelection, onPreview: @escaping (CGSize, Int?, Bool) -> Void,
                onCommit: @escaping (CGSize, Int?, Bool) -> Void) {
        self.selection = selection; self.onPreview = onPreview; self.onCommit = onCommit
    }
    public var body: some View {
        GeometryReader { proxy in
            let r = CGRect(x: selection.rect.minX * proxy.size.width, y: selection.rect.minY * proxy.size.height,
                           width: selection.rect.width * proxy.size.width, height: selection.rect.height * proxy.size.height)
            ZStack(alignment: .topLeading) {
                Rectangle().fill(p.selectionBg.opacity(0.01)).overlay { Rectangle().stroke(p.selection, lineWidth: 1) }
                    .frame(width: max(1, r.width), height: max(1, r.height)).offset(x: r.minX, y: r.minY)
                    .gesture(gesture(nil, size: proxy.size)).accessibilityLabel("選択したレイヤーを移動")
                ForEach(0..<8) { index in
                    let point = points(r)[index]
                    Rectangle().fill(p.surface200).frame(width: KRWindowMetrics.handle, height: KRWindowMetrics.handle)
                        .overlay { Rectangle().strokeBorder(p.selection, lineWidth: 1) }
                        .contentShape(Rectangle().inset(by: -KRSpace.space1))
                        .position(point).gesture(gesture(index, size: proxy.size))
                        .accessibilityLabel("変形ハンドル \(index + 1)。Option で回転")
                }
                Text(selection.label).krText(KRType.ruler).foregroundStyle(p.selection)
                    .offset(x: r.minX, y: r.maxY + KRSpace.space2).allowsHitTesting(false)
            }
        }
    }
    private func gesture(_ handle: Int?, size: CGSize) -> some Gesture {
        DragGesture().onChanged { value in
            onPreview(CGSize(width: value.translation.width / max(1, size.width), height: value.translation.height / max(1, size.height)), handle, NSEvent.modifierFlags.contains(.option))
        }.onEnded { value in
            onCommit(CGSize(width: value.translation.width / max(1, size.width), height: value.translation.height / max(1, size.height)), handle, NSEvent.modifierFlags.contains(.option))
        }
    }
    private func points(_ r: CGRect) -> [CGPoint] {
        [CGPoint(x: r.minX, y: r.minY), CGPoint(x: r.midX, y: r.minY), CGPoint(x: r.maxX, y: r.minY),
         CGPoint(x: r.minX, y: r.midY), CGPoint(x: r.maxX, y: r.midY), CGPoint(x: r.minX, y: r.maxY),
         CGPoint(x: r.midX, y: r.maxY), CGPoint(x: r.maxX, y: r.maxY)]
    }
}
