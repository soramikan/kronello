import SwiftUI

/// A job summary supplied by the app; progress is a fraction, not time.
public struct KRJobSummary: Equatable, Sendable {
    public var label: String
    public var progress: Double
    public var remainingCount: Int
    public init(_ label: String, progress: Double, remainingCount: Int = 0) { self.label = label; self.progress = progress; self.remainingCount = remainingCount }
}

/// The persistent window status line for revisions, external changes, errors, and jobs.
public struct KRStatusBar: View {
    @Environment(\.krPalette) private var p
    public let saved: String
    public let revision: String
    public let externalChange: String?
    public let error: KRDiagnostic?
    public let job: KRJobSummary?
    private let onError: () -> Void
    private let onJobs: () -> Void
    public init(saved: String = "保存済み", revision: String, externalChange: String? = nil, error: KRDiagnostic? = nil,
                job: KRJobSummary? = nil, onError: @escaping () -> Void = {}, onJobs: @escaping () -> Void = {}) {
        self.saved = saved; self.revision = revision; self.externalChange = externalChange; self.error = error; self.job = job
        self.onError = onError; self.onJobs = onJobs
    }
    public var body: some View {
        HStack(spacing: KRSpace.space4) {
            HStack(spacing: KRSpace.space1) { KRIconView(.circleCheck, size: 12); Text(saved); Text(revision).krText(KRMono.label) }
            if let externalChange { HStack(spacing: KRSpace.space1) { KRIconView(.refreshCw, size: 12); Text(externalChange) } }
            if let error { Button(action: onError) { KRErrorLine(error) }.buttonStyle(.plain).krControlFocusRing() }
            Spacer(minLength: 0)
            Button(action: onJobs) {
                HStack(spacing: KRSpace.space2) {
                    if let job {
                        KRActivityIndicator()
                        Text(job.label).foregroundStyle(p.ink)
                        KRProgressBar(job.progress)
                        if job.remainingCount > 0 { Text("+\(job.remainingCount)") }
                    } else { KRIconView(.circleCheck, size: 12); Text("ジョブなし") }
                }.padding(.horizontal, KRSpace.space1).padding(.vertical, 2)
            }.buttonStyle(KRStatusActionStyle()).krControlFocusRing()
        }.krText(KRType.label, weight: 400).foregroundStyle(p.inkMuted).lineLimit(1)
            .padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight).background(p.surface100)
            .overlay(alignment: .top) { p.line.frame(height: 1) }
    }
}

private struct KRStatusActionStyle: ButtonStyle {
    @Environment(\.krPalette) var p
    @State private var hover = false
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.background(hover || configuration.isPressed ? p.controlHover : .clear,
                                       in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
            .onHover { hover = $0 }
    }
}
