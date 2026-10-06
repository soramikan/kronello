import SwiftUI
import KronelloDesign
import KronelloAppModel

/// Expression text editor for an Expression-sourced Property (ADR-0105). The
/// canonical AST stays truth: the field shows `expression.format` output,
/// commits only complete text through `property_expression_text_set` on Return
/// or focus loss, and renders shared syntax diagnostics inline.
struct ExpressionField: View {
    @Environment(\.krPalette) var p
    @ObservedObject var model: EditorModel
    let layer: Layer
    let property: [String: Any]
    @State private var text = ""
    @State private var draftBase: String?

    var expressionID: String { property.object("source")["value"] as? String ?? "" }

    /// The last fetch/commit failure: shared syntax diagnostics when present,
    /// otherwise the typed code and message.
    var error: KRDiagnostic? {
        guard let base = model.expressionError(layer, property) else { return nil }
        let diagnostics = model.expressionDiagnostics(layer, property)
        guard !diagnostics.isEmpty else { return base }
        return .init(base.code, diagnostics.map { diagnostic in
            diagnostic.lineMessage + (diagnostic.expected.isEmpty ? "" : " — 期待: " + diagnostic.expected.joined(separator: ", "))
        }.joined(separator: "\n"))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            HStack(spacing: KRSpace.space2) {
                Text("Expression").krText(KRType.caption).foregroundStyle(p.inkMuted)
                Spacer(minLength: 0)
                KRButton("式を解除", variant: .plain) { model.detachExpression(layer, property: property) }
                    .help("現在の評価値を定数に戻して式を解除します")
            }
            KRTextArea("", value: $text, placeholder: "式を入力", lines: 3, style: KRType.timecode,
                help: "Return またはフォーカス移動で適用します", error: error,
                onEditingStart: { draftBase = model.revision },
                onCommit: {
                    model.commitExpressionText(layer, property: property, text: $0, base: draftBase)
                    draftBase = nil
                })
                .accessibilityLabel("式 " + PropertyPresentation.of(property).label)
        }
        // Refetch canonical text on appear and whenever the stored expression changes.
        .task(id: layer.id + "/" + expressionID + "@" + model.revision) {
            guard let formatted = await model.expressionText(layer, property: property), formatted != text else { return }
            text = formatted
        }
    }
}
