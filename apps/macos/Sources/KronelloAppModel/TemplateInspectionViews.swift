import SwiftUI
import KronelloDesign

/// Shared with the gallery; all internal values are labels, never editable fields.
public struct TemplateInspectionContent: View {
    @Environment(\.krPalette) private var p
    public let nodes: [TemplateInspectionNode]
    @Binding private var selection: String?
    @Binding private var stage: String
    public let stale: Bool
    public let loading: Bool
    public let failure: ServiceFailure?
    public init(nodes: [TemplateInspectionNode], selection: Binding<String?>, stage: Binding<String>,
                stale: Bool = false, loading: Bool = false, failure: ServiceFailure? = nil) {
        self.nodes = nodes; _selection = selection; _stage = stage
        self.stale = stale; self.loading = loading; self.failure = failure
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space2) {
            Text("テンプレート内部（読み取り専用）").krText(KRType.label).foregroundStyle(p.inkMuted)
                .padding(.horizontal, KRSpace.space3).padding(.top, KRSpace.space3)
            if stale { Text("再生中は停止時に更新").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3) }
            if loading { Text("検査値を読み込み中…").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3) }
            if let failure { KRErrorLine(.init(failure.code, failure.message)).padding(.horizontal, KRSpace.space3) }
            if !nodes.isEmpty {
                KRInspectorSettingRow("Layer") {
                    KRPopupButton("内部レイヤー", options: nodes.map { .init($0.id, $0.title) },
                        selection: Binding(get: { selection ?? "" }, set: { selection = $0 }))
                        .frame(width: KRWindowMetrics.settingWidth)
                }
                if let node = nodes.first(where: { $0.id == selection }) {
                    if let text = node.layer.evaluated["text"] as? String {
                        Text(text).krText(KRType.body).foregroundStyle(p.ink).textSelection(.enabled)
                            .padding(.horizontal, KRSpace.space3).fixedSize(horizontal: false, vertical: true)
                    }
                    ForEach(node.layer.properties, id: \.inspectionID) { property in
                        KRInspectorSettingRow(PropertyPresentation.of(property).label) {
                            Text(node.values(property).joined(separator: " · ")).krText(KRType.timecode)
                                .foregroundStyle(p.inkMuted).textSelection(.enabled)
                        }
                    }
                    KRInspectorSettingRow("Bounds") {
                        KRSegmentedControl([.init("layout", "layout"), .init("ink", "ink"), .init("visual", "visual")], selection: $stage).fixedSize()
                    }
                    if let bounds = node.layer.bounds(stage) {
                        KRInspectorSettingRow("Min") { readout(bounds.minX, bounds.minY) }
                        KRInspectorSettingRow("Max") { readout(bounds.maxX, bounds.maxY) }
                        KRInspectorSettingRow("Size") { readout(bounds.width, bounds.height) }
                    } else {
                        Text("この段階の bounds は空です").krText(KRType.caption).foregroundStyle(p.inkMuted).padding(.horizontal, KRSpace.space3)
                    }
                }
            }
        }
    }
    private func readout(_ x: Double, _ y: Double) -> some View {
        Text(String(format: "%.1f · %.1f px", x, y)).krText(KRType.timecode).foregroundStyle(p.inkMuted).textSelection(.enabled)
    }
}
private extension Dictionary where Key == String, Value == Any {
    var inspectionID: String { string("id") }
}

public struct TemplateInstanceInspectionPanel: View {
    @ObservedObject private var model: EditorModel
    @StateObject private var inspection: TemplateInstanceInspection
    public init(model: EditorModel) {
        self.model = model; _inspection = StateObject(wrappedValue: TemplateInstanceInspection.shared(for: model))
    }
    public var body: some View {
        TemplateInspectionContent(nodes: inspection.nodes, selection: $inspection.selection, stage: $model.ui.bounds,
            stale: inspection.stale, loading: inspection.loading, failure: inspection.failure)
            .task(id: TemplateInstanceInspection.refreshKey(model)) { await inspection.refresh(model) }
    }
}

/// Uses KRViewerSelection geometry/style with no manipulation handles or hit target.
public struct TemplateInstanceInspectionOverlay: View {
    @ObservedObject private var model: EditorModel
    @ObservedObject private var inspection: TemplateInstanceInspection
    public init(model: EditorModel) { self.model = model; inspection = .shared(for: model) }
    public var body: some View {
        if TemplateInstanceInspection.instance(model.selected, document: model.document) != nil, inspection.matchesSelection(model),
           let selection = inspection.selected?.selection(stage: model.ui.bounds, extent: model.extent) {
            TemplateInspectionBounds(selection: selection)
        }
    }
}
public struct TemplateInspectionBounds: View {
    @Environment(\.krPalette) private var p
    public let selection: KRViewerSelection
    public init(selection: KRViewerSelection) { self.selection = selection }
    public var body: some View {
        GeometryReader { proxy in
                let rect = CGRect(x: selection.rect.minX * proxy.size.width, y: selection.rect.minY * proxy.size.height,
                                  width: selection.rect.width * proxy.size.width, height: selection.rect.height * proxy.size.height)
                ZStack(alignment: .topLeading) {
                    Rectangle().stroke(p.selection, lineWidth: 1).frame(width: rect.width, height: rect.height).offset(x: rect.minX, y: rect.minY)
                    Text(selection.label).krText(KRType.ruler).foregroundStyle(p.selection).offset(x: rect.minX, y: rect.maxY + KRSpace.space2)
                }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }.allowsHitTesting(false).accessibilityHidden(true)
    }
}
