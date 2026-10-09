import SwiftUI
import KronelloDesign
import KronelloAppModel

/// COLOR-004 (ADR-0113): collapsible scope read-out for the Edit page. The
/// shared `inspect.scopes` query observes the composited working-space frame
/// on the deterministic CPU reference backend; this panel only draws the
/// returned integer bins — no display transform is applied here.
struct ScopesPanel: View {
    @ObservedObject var model: EditorModel
    /// GUI-012: scope visibility persists per workspace layout (ADR-0033).
    private var expanded: Bool {
        get { model.ui.layout.scopesVisible }
        nonmutating set { model.ui.layout.scopesVisible = newValue }
    }
    @State private var scopes: ScopeBins?
    @State private var failure: ServiceFailure?
    @State private var loading = false
    @State private var requestID = 0
    @Environment(\.krPalette) var p

    /// Decoded `inspect.scopes` value; bins are row-major integer histograms.
    struct ScopeBins {
        var revision: String
        var waveform: (columns: Int, levels: Int, bins: [Int])
        var vectorscope: (size: Int, bins: [Int])
        var histogram: (levels: Int, red: [Int], green: [Int], blue: [Int], luma: [Int])
        var parade: (columns: Int, levels: Int, red: [Int], green: [Int], blue: [Int])
        static func parse(_ value: [String: Any]) -> ScopeBins? {
            func bins(_ object: [String: Any], _ key: String) -> [Int] {
                (object[key] as? [NSNumber])?.map(\.intValue) ?? []
            }
            func dimension(_ object: [String: Any], _ key: String) -> Int {
                (object[key] as? NSNumber)?.intValue ?? 0
            }
            guard let waveform = value["waveform"] as? [String: Any],
                  let vectorscope = value["vectorscope"] as? [String: Any],
                  let histogram = value["histogram"] as? [String: Any],
                  let parade = value["parade"] as? [String: Any] else { return nil }
            return ScopeBins(
                revision: value["revision"] as? String ?? "",
                waveform: (dimension(waveform, "columns"), dimension(waveform, "levels"), bins(waveform, "bins")),
                vectorscope: (dimension(vectorscope, "size"), bins(vectorscope, "bins")),
                histogram: (dimension(histogram, "levels"), bins(histogram, "red"), bins(histogram, "green"), bins(histogram, "blue"), bins(histogram, "luma")),
                parade: (dimension(parade, "columns"), dimension(parade, "levels"), bins(parade, "red"), bins(parade, "green"), bins(parade, "blue")))
        }
    }

    /// Refresh identity: the scopes observe the same composited frame as the
    /// preview, so revision, sequence, and playhead all invalidate the bins.
    var refreshKey: String {
        "\(expanded)|\(model.ui.sequence ?? "")|\(model.revision)|\(model.ui.time.num)/\(model.ui.time.den)"
    }
    var body: some View {
        VStack(spacing: 0) {
            Divider().overlay(p.surface200)
            Button { expanded.toggle() } label: {
                HStack(spacing: KRSpace.space2) {
                    KRIconView(expanded ? .chevronDown : .chevronRight, size: 12).foregroundStyle(p.inkMuted)
                    Text("スコープ").krText(KRType.label).foregroundStyle(p.ink)
                    if loading { KRActivityIndicator() }
                    Spacer()
                    if let scopes { Text("rev \(scopes.revision)").krText(KRType.caption).foregroundStyle(p.inkMuted) }
                }
                .padding(.horizontal, KRSpace.space3).padding(.vertical, KRSpace.space2)
                .contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityLabel("スコープパネルの開閉")
            if expanded {
                if let failure {
                    Text("\(failure.code): \(failure.message)").krText(KRType.caption)
                        .foregroundStyle(p.danger).padding(.horizontal, KRSpace.space3).padding(.bottom, KRSpace.space2)
                }
                if let scopes { charts(scopes).padding(.horizontal, KRSpace.space3).padding(.bottom, KRSpace.space3) }
                else if failure == nil {
                    Text(loading ? "スコープを計算中…" : "フレームを評価してスコープを表示します。")
                        .krText(KRType.caption).foregroundStyle(p.inkMuted)
                        .padding(.horizontal, KRSpace.space3).padding(.bottom, KRSpace.space2)
                }
            }
        }
        .task(id: refreshKey) { await refresh() }
    }
    /// One `inspect.scopes` call per refresh key; stale responses are dropped.
    @MainActor func refresh() async {
        guard expanded, let sequence = model.ui.sequence, !model.sequence.isEmpty else { scopes = nil; failure = nil; return }
        requestID += 1; let id = requestID; loading = true
        let extent = model.extent
        let width = 256, height = max(1, min(256, Int((CGFloat(width) * extent.height / max(1, extent.width)).rounded())))
        do {
            // inspect.scopes is a projectless operation; `model.request`
            // would inject `project` and fail strict request validation.
            let value = try await model.transport.call([
                "operation": "inspect.scopes",
                "input": ["project": model.path,
                    "target": ["kind": "sequence", "sequence": sequence],
                    "region": ["origin": [0.0, 0.0], "extent": [extent.width, extent.height], "pixels": [width, height]],
                    "luts": model.lutInputs],
                "time": model.ui.time.wire])
            guard id == requestID else { return }
            scopes = ScopeBins.parse(value); failure = nil; loading = false
        } catch {
            guard id == requestID else { return }
            failure = model.serviceFailure(error); loading = false
        }
    }
    @ViewBuilder func charts(_ scopes: ScopeBins) -> some View {
        VStack(spacing: KRSpace.space2) {
            HStack(spacing: KRSpace.space2) {
                scope("波形") { ScopeHeatmap(columns: scopes.waveform.columns, levels: scopes.waveform.levels, bins: scopes.waveform.bins, tint: .white) }
                scope("ベクトルスコープ") { ScopeVectorscopeView(side: scopes.vectorscope.size, bins: scopes.vectorscope.bins) }
            }
            HStack(spacing: KRSpace.space2) {
                scope("ヒストグラム") {
                    ScopeHistogramView(levels: scopes.histogram.levels, red: scopes.histogram.red,
                        green: scopes.histogram.green, blue: scopes.histogram.blue, luma: scopes.histogram.luma)
                }
                scope("RGB パレード") {
                    HStack(spacing: 2) {
                        ScopeHeatmap(columns: scopes.parade.columns, levels: scopes.parade.levels, bins: scopes.parade.red, tint: .red)
                        ScopeHeatmap(columns: scopes.parade.columns, levels: scopes.parade.levels, bins: scopes.parade.green, tint: .green)
                        ScopeHeatmap(columns: scopes.parade.columns, levels: scopes.parade.levels, bins: scopes.parade.blue, tint: .blue)
                    }
                }
            }
        }
    }
    func scope<V: View>(_ title: String, @ViewBuilder content: () -> V) -> some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            Text(title).krText(KRType.caption).foregroundStyle(p.inkMuted)
            content().frame(height: 96).frame(maxWidth: .infinity)
                .background(p.surface0, in: RoundedRectangle(cornerRadius: KRRadius.radiusSm))
        }.frame(maxWidth: .infinity)
    }
}

/// Integer-bin heatmap for waveform and RGB parade: row 0 is the lowest level
/// (screen y flips), and intensity is normalized per scope family.
private struct ScopeHeatmap: View {
    let columns: Int, levels: Int, bins: [Int]
    var tint: Color = .white
    var body: some View {
        Canvas { context, size in
            guard columns > 0, levels > 0, bins.count == columns * levels else { return }
            let peak = bins.max() ?? 0
            guard peak > 0 else { return }
            let cell = CGSize(width: size.width / CGFloat(columns), height: size.height / CGFloat(levels))
            for (index, count) in bins.enumerated() where count > 0 {
                let column = index % columns, row = index / columns
                let alpha = 0.25 + 0.75 * Double(count) / Double(peak)
                let rect = CGRect(x: CGFloat(column) * cell.width,
                    y: CGFloat(levels - 1 - row) * cell.height,
                    width: max(1, cell.width), height: max(1, cell.height))
                context.fill(Path(rect), with: .color(tint.opacity(alpha)))
            }
        }
    }
}

/// Cb/Cr histogram centered on neutral; `bins[cr * size + cb]`.
private struct ScopeVectorscopeView: View {
    let side: Int, bins: [Int]
    var body: some View {
        Canvas { context, size in
            guard side > 0, bins.count == side * side else { return }
            let peak = bins.max() ?? 0
            guard peak > 0 else { return }
            let cell = CGSize(width: size.width / CGFloat(side), height: size.height / CGFloat(side))
            // Neutral-center crosshair for orientation.
            context.fill(Path(CGRect(x: size.width / 2 - 0.5, y: 0, width: 1, height: size.height)),
                with: .color(.gray.opacity(0.2)))
            context.fill(Path(CGRect(x: 0, y: size.height / 2 - 0.5, width: size.width, height: 1)),
                with: .color(.gray.opacity(0.2)))
            for (index, count) in bins.enumerated() where count > 0 {
                let cb = index % side, cr = index / side
                let alpha = 0.25 + 0.75 * Double(count) / Double(peak)
                let rect = CGRect(x: CGFloat(cb) * cell.width, y: CGFloat(cr) * cell.height,
                    width: max(1, cell.width), height: max(1, cell.height))
                context.fill(Path(rect), with: .color(.mint.opacity(alpha)))
            }
        }
    }
}

/// Overlaid R/G/B area histograms plus a dim luma outline.
private struct ScopeHistogramView: View {
    let levels: Int, red: [Int], green: [Int], blue: [Int], luma: [Int]
    var body: some View {
        Canvas { context, size in
            guard levels > 0 else { return }
            let peaks = [red.max(), green.max(), blue.max(), luma.max()].compactMap { $0 }.max() ?? 0
            guard peaks > 0 else { return }
            let step = size.width / CGFloat(levels)
            for (bins, color) in [(red, Color.red), (green, Color.green), (blue, Color.blue)] where bins.count == levels {
                var path = Path()
                path.move(to: CGPoint(x: 0, y: size.height))
                for (index, count) in bins.enumerated() {
                    path.addLine(to: CGPoint(x: CGFloat(index) * step,
                        y: size.height * (1 - CGFloat(count) / CGFloat(peaks))))
                }
                path.addLine(to: CGPoint(x: size.width, y: size.height))
                context.fill(path, with: .color(color.opacity(0.35)))
            }
            if luma.count == levels {
                var path = Path()
                for (index, count) in luma.enumerated() {
                    let point = CGPoint(x: CGFloat(index) * step, y: size.height * (1 - CGFloat(count) / CGFloat(peaks)))
                    if index == 0 { path.move(to: point) } else { path.addLine(to: point) }
                }
                context.stroke(path, with: .color(.white.opacity(0.5)), style: .init(lineWidth: 1))
            }
        }
    }
}
