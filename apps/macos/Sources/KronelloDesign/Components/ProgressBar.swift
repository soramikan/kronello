import SwiftUI

/// A neutral progress meter; accent and selection colors are reserved for their semantic roles.
public struct KRProgressBar: View {
    @Environment(\.krPalette) private var p
    public let progress: Double
    public init(_ progress: Double) { self.progress = progress }
    public var body: some View {
        GeometryReader { proxy in
            Capsule().fill(p.lineStrong)
                .overlay(alignment: .leading) { Capsule().fill(p.ink).frame(width: proxy.size.width * min(1, max(0, progress))) }
        }.frame(width: 80, height: 4).accessibilityLabel("進捗").accessibilityValue("\(Int(min(1, max(0, progress)) * 100))%")
    }
}

/// The only animated icon in Kronello; respects the system's reduced-motion setting.
public struct KRActivityIndicator: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.krFreezeActivity) private var frozen
    public let size: CGFloat
    public init(size: CGFloat = 12) { self.size = size }
    public var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 30, paused: reduceMotion || frozen)) { context in
            KRIconView(.loaderCircle, size: size)
                .rotationEffect(.degrees(reduceMotion || frozen ? 0 : context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.2) / 1.2 * 360))
        }.accessibilityLabel("実行中")
    }
}

private struct KRFreezeActivityKey: EnvironmentKey { static let defaultValue = false }
extension EnvironmentValues {
    /// Freezes activity indicators for deterministic snapshots. Reduced motion is always respected.
    public var krFreezeActivity: Bool {
        get { self[KRFreezeActivityKey.self] }
        set { self[KRFreezeActivityKey.self] = newValue }
    }
}
