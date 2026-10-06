import SwiftUI
import KronelloAppModel
import KronelloDesign

@MainActor enum TemplateInspectionSheets {
    static var node: TemplateInspectionNode {
        let position: [String: Any] = ["id": "position", "descriptor": ["key": "kronello.transform.position"]]
        let width: [String: Any] = ["id": "wrap", "descriptor": ["key": "kronello.text.wrap_width"]]
        let opacity: [String: Any] = ["id": "opacity", "descriptor": ["key": "kronello.opacity"]]
        let bounds: [String: Any] = ["min": [32.0, 220.0], "max": [80.0, 284.0]]
        return .init(path: ["portrait-instance"], layer: .init(id: "headline", authored: ["name": "Portrait headline", "properties": [position, width, opacity]],
            evaluated: ["text": "日本語の字幕\n背景帯が追従", "properties": ["position": ["kind": "vec2", "value": [32.0, 220.0]],
                "wrap": ["kind": "scalar", "value": 48.0], "opacity": ["kind": "scalar", "value": 1.0]],
                "bounds": ["layout_bounds": bounds, "ink_bounds": bounds, "visual_bounds": bounds]], level: 1))
    }
    static var all: [(String, AnyView)] {
        [("TemplateInstanceInspection", AnyView(HStack(alignment: .top, spacing: KRSpace.space4) {
            panel(stale: false)
            panel(stale: true)
            KRPanel("Inspector") {
                TemplateInspectionContent(nodes: [], selection: .constant(nil), stage: .constant("layout"),
                    failure: .init(code: "FONT_MISSING", message: "固定した font lock の書体を読み込めません。素材の場所を確認してください。"))
            }.frame(width: 304, height: 420)
        })),
         ("TemplateInstanceBounds", AnyView(ZStack {
             KRViewerFrame(aspectRatio: 9 / 16)
             TemplateInspectionBounds(selection: node.selection(stage: "layout", extent: .init(width: 180, height: 320))!)
         }.frame(width: 180, height: 320)))]
    }
    static func panel(stale: Bool) -> some View {
        KRPanel("Inspector") {
            TemplateInspectionContent(nodes: [node], selection: .constant(node.id), stage: .constant("layout"), stale: stale)
        }.frame(width: 304, height: 420)
    }
}
