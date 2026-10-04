import SwiftUI

/// A presentation-only job state, including a typed failure when required.
public enum KRJobState: Equatable, Sendable {
    case running(progress: Double, remaining: String)
    case queued
    case done(elapsed: String)
    case failed(KRDiagnostic)
    case cancelled
    var icon: KRIcon {
        switch self { case .running: return .loaderCircle; case .queued: return .clock; case .done: return .circleCheck; case .failed: return .triangleAlert; case .cancelled: return .circleX }
    }
}

/// A job list row with state, fixed input metadata, result, and consumer-supplied actions.
public struct KRJobRow<Actions: View>: View {
    @Environment(\.krPalette) private var p
    public let name: String
    public let detail: String
    public let state: KRJobState
    private let actions: Actions
    public init(_ name: String, detail: String, state: KRJobState, @ViewBuilder actions: () -> Actions) {
        self.name = name; self.detail = detail; self.state = state; self.actions = actions()
    }
    private var failure: KRDiagnostic? { if case .failed(let error) = state { return error }; return nil }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            Group { if case .running = state { KRActivityIndicator(size: 14) } else { KRIconView(state.icon) } }
                .foregroundStyle(failure == nil ? p.inkMuted : p.danger)
            VStack(alignment: .leading, spacing: 0) {
                Text(name).krText(KRType.body).foregroundStyle(p.ink).lineLimit(1)
                if let failure {
                    (Text(failure.code).font(KRMono.caption.font()) + Text(" · " + failure.message))
                        .krText(KRType.caption).foregroundStyle(p.danger).lineLimit(1)
                } else { Text(detail).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1) }
            }.frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: KRSpace.space2) {
                switch state {
                case .running(let progress, let remaining):
                    Text("\(Int(progress * 100))% · \(remaining)"); KRProgressBar(progress)
                case .queued: Text("待機中")
                case .done(let elapsed): Text("完了 · \(elapsed)")
                case .failed: Text("失敗")
                case .cancelled: Text("中止")
                }
            }.krText(KRMono.label).foregroundStyle(p.inkMuted)
            actions
        }.padding(.horizontal, KRSpace.space3).padding(.vertical, KRSpace.space1).frame(minHeight: 40).krBottomLine()
    }
}

extension KRJobRow where Actions == EmptyView {
    public init(_ name: String, detail: String, state: KRJobState) { self.init(name, detail: detail, state: state) { EmptyView() } }
}
