import SwiftUI
import KronelloDesign
import KronelloAppModel

/// FX-004 (ADR-0114) clip mask editor: one mask list row per authored mask,
/// mode/parameter controls bound to `clip_masks_set`, and a vertex editor for
/// the constant Bezier path. Animated mask parameters stay read-only.
struct ClipMaskInspector: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    @State private var draftBases: [String: String] = [:]
    @Environment(\.krPalette) var p
    static let modes: [(String, String)] = [
        ("add", "Add"), ("subtract", "Subtract"), ("intersect", "Intersect"), ("difference", "Difference"),
    ]
    /// (mask field, label, unit, range, step)
    static let scalarRows: [(String, String, String, ClosedRange<Double>, Double)] = [
        ("feather", "フェザー", "px", 0...4096, 0.1),
        ("expansion", "拡張", "px", -4096...4096, 0.1),
        ("opacity", "不透明度", "%", 0...100, 1),
    ]
    var disabled: Bool { model.ui.locked.contains(clip.track) || model.busy || model.pendingCandidate != nil }
    var body: some View {
        let masks = EditorModel.clipMasks(clip)
        Group {
            ForEach(Array(masks.enumerated()), id: \.offset) { index, mask in
                VStack(alignment: .leading, spacing: 0) {
                    KRInspectorSettingRow("Mask \(index + 1)") {
                        KRButton(icon: .trash2, accessibilityLabel: "マスク \(index + 1) を削除") {
                            model.removeClipMask(clip, index: index)
                        }
                    }
                    KRInspectorSettingRow("モード") {
                        KRPopupButton("モード", options: Self.modes.map { KRPopupOption($0.0, $0.1) },
                            selection: Binding(get: { mask.string("mode") }, set: { model.setClipMaskField(clip, index: index, field: "mode", value: $0) }))
                            .frame(width: KRWindowMetrics.settingWidth)
                    }
                    ForEach(Array(Self.scalarRows.enumerated()), id: \.offset) { _, row in
                        let (field, label, unit, range, step) = row
                        let raw = EditorModel.maskScalar(clip, mask: mask, field: field)
                        let shown = field == "opacity" ? (raw ?? 0) * 100 : (raw ?? 0)
                        KRInspectorSettingRow(label) {
                            KRNumberField(value: .constant(shown), unit: unit, step: step, range: range, precision: 2,
                                accessibilityLabel: "マスク \(index + 1) の\(label)",
                                onEditingStart: { draftBases["\(index):\(field)"] = model.revision },
                                onCommit: { _, value in
                                    let committed = field == "opacity" ? value / 100 : value
                                    model.setClipMaskScalar(clip, index: index, field: field, value: committed,
                                        base: draftBases.removeValue(forKey: "\(index):\(field)"))
                                }).frame(width: KRWindowMetrics.settingWidth)
                        }.disabled(raw == nil)
                    }
                    KRInspectorSettingRow("反転") {
                        KRCheckbox("", isOn: Binding(get: { mask["invert"] as? Bool ?? false },
                            set: { model.setClipMaskField(clip, index: index, field: "invert", value: $0) }))
                            .accessibilityLabel("マスク \(index + 1) の反転")
                    }
                    KRInspectorSettingRow("閉合") {
                        KRCheckbox("", isOn: Binding(get: { mask["closed"] as? Bool ?? true },
                            set: { model.setClipMaskField(clip, index: index, field: "closed", value: $0) }))
                            .accessibilityLabel("マスク \(index + 1) の閉合")
                    }
                    maskVertices(index: index, mask: mask)
                    if !EditorModel.maskParameterIsConstant(clip, mask: mask, field: "path")
                        || Self.scalarRows.contains(where: { !EditorModel.maskParameterIsConstant(clip, mask: mask, field: $0.0) }) {
                        Text("アニメーション付きパラメータは読み取り専用です").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                    }
                }
            }
            KRInspectorSettingRow("追加") {
                KRButton("マスクを追加", variant: .secondary) { model.addClipMask(clip) }
                    .frame(width: KRWindowMetrics.settingWidth)
            }
            if masks.isEmpty {
                Text("マスクはまだありません").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
            }
        }.disabled(disabled)
    }
    /// Anchor vertices of the constant path with X/Y fields and per-vertex
    /// insert/remove. Curve handles drag on the viewer overlay instead.
    @ViewBuilder func maskVertices(index: Int, mask: [String: Any]) -> some View {
        if let vertices = EditorModel.maskVertices(clip, mask: mask) {
            ForEach(Array(vertices.enumerated()), id: \.offset) { vertex, v in
                KRInspectorSettingRow("頂点 \(vertex + 1)") {
                    HStack(spacing: KRSpace.space1) {
                        KRInspectorAxis("X") {
                            vertexField(index: index, vertex: vertex, point: v.anchor, axis: 0)
                        }
                        KRInspectorAxis("Y") {
                            vertexField(index: index, vertex: vertex, point: v.anchor, axis: 1)
                        }
                        KRButton(icon: .plus, accessibilityLabel: "頂点 \(vertex + 1) の後に追加", size: 20, iconSize: 10) {
                            model.insertClipMaskVertex(clip, index: index, afterVertex: vertex)
                        }.disabled(vertices.count >= 1024)
                        KRButton(icon: .trash2, accessibilityLabel: "頂点 \(vertex + 1) を削除", size: 20, iconSize: 10) {
                            model.removeClipMaskVertex(clip, index: index, vertexIndex: vertex)
                        }.disabled(vertices.count <= 3)
                    }.frame(width: KRWindowMetrics.settingWidth)
                }
            }
        }
    }
    func vertexField(index: Int, vertex: Int, point: CGPoint, axis: Int) -> some View {
        let key = "v\(index):\(vertex):\(axis)"
        return KRNumberField(value: .constant(axis == 0 ? point.x : point.y), step: 1, precision: 1,
            accessibilityLabel: "頂点 \(vertex + 1) \(axis == 0 ? "X" : "Y")",
            onEditingStart: { draftBases[key] = model.revision },
            onCommit: { _, value in
                var moved = point
                if axis == 0 { moved.x = value } else { moved.y = value }
                model.setClipMaskAnchor(clip, index: index, vertexIndex: vertex, point: moved,
                    base: draftBases.removeValue(forKey: key))
            })
    }
}

/// FX-004 viewer overlay: the selected clip's mask outlines and draggable
/// Bezier handles in clip-local design_px, mapped by extent fit like the
/// preview. Handle drags commit one `clip_masks_set` edit on gesture end.
struct MaskOverlay: View {
    @ObservedObject var model: EditorModel
    let clip: EditClip
    @Environment(\.krPalette) var p
    /// Live vertex positions during a drag; reset when the gesture ends.
    @State private var dragged: (mask: Int, vertices: [EditorModel.MaskVertex])?
    var body: some View {
        GeometryReader { geo in
            let scale = CGSize(width: geo.size.width / max(1, model.extent.width),
                               height: geo.size.height / max(1, model.extent.height))
            let masks = EditorModel.clipMasks(clip)
            ForEach(Array(masks.enumerated()), id: \.offset) { index, mask in
                if let vertices = EditorModel.maskVertices(clip, mask: mask) {
                    let shown = dragged?.mask == index ? dragged!.vertices : vertices
                    maskPath(shown, closed: mask["closed"] as? Bool ?? true, scale: scale)
                        .stroke(p.selection, lineWidth: 1.5)
                    ForEach(Array(shown.enumerated()), id: \.offset) { vertex, v in
                        if let incoming = v.controlIn {
                            handle(in: incoming, scale: scale)
                                .gesture(handleDrag(mask: index, vertex: vertex, anchor: v.anchor,
                                    point: incoming, scale: scale, incoming: true))
                        }
                        if let outgoing = v.controlOut {
                            handle(in: outgoing, scale: scale)
                                .gesture(handleDrag(mask: index, vertex: vertex, anchor: v.anchor,
                                    point: outgoing, scale: scale, incoming: false))
                        }
                        anchor(at: v.anchor, scale: scale)
                            .gesture(DragGesture(minimumDistance: 0)
                                .onChanged { g in
                                    dragged = (index, translated(vertices, vertex: vertex,
                                        dx: g.translation.width / scale.width,
                                        dy: g.translation.height / scale.height))
                                }
                                .onEnded { g in
                                    dragged = nil
                                    model.setClipMaskAnchor(clip, index: index, vertexIndex: vertex,
                                        point: CGPoint(x: v.anchor.x + g.translation.width / scale.width,
                                                       y: v.anchor.y + g.translation.height / scale.height))
                                })
                    }
                }
            }
        }
        .allowsHitTesting(model.editTool == "select")
    }
    /// Moves the anchor plus its adjacent handles, previewing the drag.
    func translated(_ vertices: [EditorModel.MaskVertex], vertex: Int, dx: CGFloat, dy: CGFloat) -> [EditorModel.MaskVertex] {
        var copy = vertices
        func moved(_ p: CGPoint?) -> CGPoint? { p.map { CGPoint(x: $0.x + dx, y: $0.y + dy) } }
        copy[vertex].anchor = moved(copy[vertex].anchor)!
        copy[vertex].controlIn = moved(copy[vertex].controlIn)
        copy[vertex].controlOut = moved(copy[vertex].controlOut)
        return copy
    }
    func maskPath(_ vertices: [EditorModel.MaskVertex], closed: Bool, scale: CGSize) -> Path {
        func mapped(_ p: CGPoint) -> CGPoint { CGPoint(x: p.x * scale.width, y: p.y * scale.height) }
        var path = Path()
        guard let first = vertices.first else { return path }
        path.move(to: mapped(first.anchor))
        for index in 1..<vertices.count {
            let previous = vertices[index - 1], vertex = vertices[index]
            switch (previous.controlOut, vertex.controlIn) {
            case (nil, nil): path.addLine(to: mapped(vertex.anchor))
            case let (c?, nil):
                path.addQuadCurve(to: mapped(vertex.anchor), control: mapped(c))
            case let (nil, c?):
                path.addQuadCurve(to: mapped(vertex.anchor), control: mapped(c))
            case let (c1, c2):
                let a = c1 ?? previous.anchor, b = c2 ?? vertex.anchor
                if a == b { path.addQuadCurve(to: mapped(vertex.anchor), control: mapped(a)) }
                else { path.addCurve(to: mapped(vertex.anchor), control1: mapped(a), control2: mapped(b)) }
            }
        }
        if closed { path.closeSubpath() }
        return path
    }
    func anchor(at point: CGPoint, scale: CGSize) -> some View {
        Circle().fill(p.selection).frame(width: 9, height: 9)
            .overlay(Circle().stroke(Color.black.opacity(0.5), lineWidth: 1))
            .position(x: point.x * scale.width, y: point.y * scale.height)
            .contentShape(Rectangle().size(width: 20, height: 20))
    }
    func handle(in point: CGPoint, scale: CGSize) -> some View {
        Rectangle().fill(p.inkMuted).frame(width: 7, height: 7)
            .position(x: point.x * scale.width, y: point.y * scale.height)
            .contentShape(Rectangle().size(width: 16, height: 16))
    }
    /// Drags a Bezier handle; the preview mirrors the live handle position
    /// through the shared vertex list.
    func handleDrag(mask: Int, vertex: Int, anchor: CGPoint, point: CGPoint, scale: CGSize, incoming: Bool) -> some Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { g in
                var copy = EditorModel.maskVertices(clip, mask: EditorModel.clipMasks(clip)[mask]) ?? []
                guard copy.indices.contains(vertex) else { return }
                let moved = CGPoint(x: point.x + g.translation.width / scale.width,
                                    y: point.y + g.translation.height / scale.height)
                if incoming { copy[vertex].controlIn = moved } else { copy[vertex].controlOut = moved }
                dragged = (mask, copy)
            }
            .onEnded { g in
                dragged = nil
                model.setClipMaskHandle(clip, index: mask, vertexIndex: vertex, incoming: incoming,
                    point: CGPoint(x: point.x + g.translation.width / scale.width,
                                   y: point.y + g.translation.height / scale.height))
            }
    }
}
