import SwiftUI

/// Playback controls and current time; rates and view preferences are UI-only values.
public struct KRTransportBar: View {
    @Environment(\.krPalette) private var p
    @Binding private var frames: Int64
    @Binding private var playing: Bool
    @Binding private var looping: Bool
    @Binding private var zoom: String
    @Binding private var resolution: String
    public let fps: Int
    public let duration: String
    private let onSeek: (Int64) -> Void
    private let onStep: (Int) -> Void
    private let onBoundary: (Bool) -> Void
    public init(frames: Binding<Int64>, fps: Int, duration: String, playing: Binding<Bool>, looping: Binding<Bool>,
                zoom: Binding<String>, resolution: Binding<String>, onSeek: @escaping (Int64) -> Void = { _ in },
                onStep: @escaping (Int) -> Void = { _ in }, onBoundary: @escaping (Bool) -> Void = { _ in }) {
        _frames = frames; self.fps = fps; self.duration = duration; _playing = playing; _looping = looping
        _zoom = zoom; _resolution = resolution; self.onSeek = onSeek; self.onStep = onStep; self.onBoundary = onBoundary
    }
    public var body: some View {
        HStack(spacing: KRSpace.space2) {
            KRTimecodeField(frames: $frames, fps: fps, onSeek: onSeek)
            Spacer(minLength: 0)
            HStack(spacing: 2) {
                KRButton(icon: .skipBack, accessibilityLabel: "先頭へ") { onBoundary(false) }
                KRButton(icon: .stepBack, accessibilityLabel: "1 フレーム戻る") { onStep(-1) }
                KRButton(icon: playing ? .pause : .play, accessibilityLabel: playing ? "一時停止" : "再生") { playing.toggle() }
                KRButton(icon: .stepForward, accessibilityLabel: "1 フレーム進む") { onStep(1) }
                KRButton(icon: .skipForward, accessibilityLabel: "末尾へ") { onBoundary(true) }
                KRButton(icon: .repeat, accessibilityLabel: "ループ再生", pressed: looping) { looping.toggle() }
            }
            Spacer(minLength: 0)
            KRPopupButton("表示倍率", options: [.init("fit", "Fit"), .init("50", "50%"), .init("100", "100%")], selection: $zoom).frame(width: 96)
            KRPopupButton("解像度", options: [.init("full", "Full"), .init("half", "Half"), .init("quarter", "Quarter")], selection: $resolution).frame(width: 96)
            Text(duration).krText(KRType.timecode).foregroundStyle(p.inkMuted)
        }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.trackHeight).background(p.surface100)
            .overlay(alignment: .top) { p.line.frame(height: 1) }
    }
}
