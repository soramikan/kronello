import AppKit
import SwiftUI

/// Integral non-drop-frame timecode parsing and formatting; seconds are never stored as floats.
public enum KRTimecode {
    public enum ParseError: Error, Equatable { case invalidRate, invalidSyntax, outOfRange, overflow }
    /// Omitted leading fields are zero: `1:12` means one second and twelve frames.
    public static func parse(_ text: String, fps: Int) throws -> Int64 {
        guard fps > 0 else { throw ParseError.invalidRate }
        let pieces = text.trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ":", omittingEmptySubsequences: false)
        guard (1...4).contains(pieces.count), pieces.allSatisfy({ !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }) else { throw ParseError.invalidSyntax }
        let parsed = try pieces.map { part -> Int64 in guard let value = Int64(part) else { throw ParseError.overflow }; return value }
        let fields = Array(repeating: Int64(0), count: 4 - parsed.count) + parsed
        guard fields[1] < 60 && fields[2] < 60 && fields[3] < Int64(fps) else { throw ParseError.outOfRange }
        var total = fields[0]
        for (factor, field) in [(Int64(60), fields[1]), (Int64(60), fields[2]), (Int64(fps), fields[3])] {
            let multiply = total.multipliedReportingOverflow(by: factor)
            let add = multiply.partialValue.addingReportingOverflow(field)
            guard !multiply.overflow && !add.overflow else { throw ParseError.overflow }
            total = add.partialValue
        }
        return total
    }
    public static func format(frames: Int64, fps: Int) -> String {
        precondition(frames >= 0 && fps > 0)
        let rate = Int64(fps), seconds = frames / rate
        return String(format: "%02lld:%02lld:%02lld:%02lld", seconds / 3600, seconds / 60 % 60, seconds % 60, frames % rate)
    }
}

/// A current-time input with abbreviated parsing, Return seek, and Escape cancellation.
public struct KRTimecodeField: View {
    @Environment(\.krStaticRendering) private var staticRendering
    @Environment(\.krPalette) private var p
    @Environment(\.isEnabled) private var enabled
    @Binding private var frames: Int64
    public let fps: Int
    private let onSeek: (Int64) -> Void
    private let appearance: KRControlAppearance
    private let forcedInvalid: Bool
    @State private var text: String
    @State private var invalid = false
    @FocusState private var focused: Bool
    public init(frames: Binding<Int64>, fps: Int, invalid: Bool = false, appearance: KRControlAppearance = .resting,
                onSeek: @escaping (Int64) -> Void = { _ in }) {
        _frames = frames; self.fps = fps; self.onSeek = onSeek; self.appearance = appearance; forcedInvalid = invalid
        _text = State(initialValue: KRTimecode.format(frames: frames.wrappedValue, fps: fps))
    }
    public var body: some View {
        Group {
            if staticRendering { Text(text).frame(maxWidth: .infinity) }
            else { TextField("現在時刻", text: $text).textFieldStyle(.plain).focused($focused) }
        }.krText(KRType.timecode).multilineTextAlignment(.center)
            .foregroundStyle(focused || appearance == .focused ? p.ink : p.accentInk).tint(p.selection)
            .padding(.horizontal, KRSpace.space2).frame(width: 104, height: KRSize.controlHeight)
            .background(p.surface200, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
            .overlay { RoundedRectangle(cornerRadius: KRRadius.radiusSm).strokeBorder(invalid || forcedInvalid ? p.danger : p.lineStrong, lineWidth: 1) }
            .krFocusRing(focused || appearance == .focused).opacity(enabled ? 1 : 0.45)
            .onSubmit {
                do { let next = try KRTimecode.parse(text, fps: fps); frames = next; invalid = false; text = KRTimecode.format(frames: next, fps: fps); onSeek(next) }
                catch { invalid = true }
            }
            .onKeyPress(.escape) { text = KRTimecode.format(frames: frames, fps: fps); invalid = false; focused = false; return .handled }
            .onChange(of: frames) { _, new in if !focused { text = KRTimecode.format(frames: new, fps: fps) } }
            .onChange(of: focused) { _, new in
                if new { DispatchQueue.main.async { (NSApp.keyWindow?.firstResponder as? NSTextView)?.selectAll(nil) } }
            }
            .accessibilityLabel("現在時刻").accessibilityValue(text)
    }
}
