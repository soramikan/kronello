import SwiftUI

/// A peak/RMS level bar on a fixed -60...0 dBFS scale. Feed it the
/// evaluator's per-block stereo readings; silence collapses to zero.
public struct KRMeterBar: View {
    @Environment(\.krPalette) private var p
    public let peak: Double
    public let rms: Double
    public let height: CGFloat
    public init(peak: Double, rms: Double, height: CGFloat = 6) {
        self.peak = peak; self.rms = rms; self.height = height
    }
    /// Linear amplitude → normalized fraction for display. Anything at or
    /// below the floor and non-finite values read as silence.
    public static func level(_ amplitude: Double, floorDb: Double = -60) -> Double {
        guard amplitude.isFinite, amplitude > 0 else { return 0 }
        return min(1, max(0, (20 * log10(amplitude) - floorDb) / -floorDb))
    }
    public var body: some View {
        GeometryReader { proxy in
            let width = proxy.size.width
            ZStack(alignment: .leading) {
                Capsule().fill(p.surface0).frame(height: height)
                Capsule().fill(peak > 1 ? p.danger : p.kindAudio)
                    .frame(width: max(0, width * KRMeterBar.level(rms)), height: height)
                Rectangle().fill(peak > 1 ? p.danger : p.ink)
                    .frame(width: 2, height: height)
                    .offset(x: max(0, width * KRMeterBar.level(peak) - 1))
            }
        }.frame(height: height)
            .accessibilityElement().accessibilityLabel("オーディオメーター")
            .accessibilityValue("\(Int((20 * log10(max(rms, 1e-6))).rounded())) dBFS RMS")
    }
}
