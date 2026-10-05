import SwiftUI

/// Template geometry: left/comparison over a 196pt policy panel; right spans both.
public struct KRTemplatePageLayout<Templates: View, Comparison: View, Policy: View, Inputs: View>: View {
    private let templates: Templates
    private let comparison: Comparison
    private let policy: Policy
    private let inputs: Inputs
    public init(@ViewBuilder templates: () -> Templates, @ViewBuilder comparison: () -> Comparison, @ViewBuilder policy: () -> Policy, @ViewBuilder inputs: () -> Inputs) {
        self.templates = templates(); self.comparison = comparison(); self.policy = policy(); self.inputs = inputs()
    }
    public var body: some View {
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                HStack(spacing: 0) { templates.frame(width: 264); comparison.frame(maxWidth: .infinity) }.frame(maxHeight: .infinity)
                policy.frame(height: 196)
            }
            inputs.frame(width: 320)
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
/// Export's single primary action. The label expands inside the shared style so
/// its fill, hit area and owned focus ring span the settings column together.
public struct KRExportStartButton: View {
    private let action: () -> Void
    public init(action: @escaping () -> Void) { self.action = action }
    public var body: some View {
        Button(action:action) { Text("書き出す").frame(maxWidth:.infinity) }
            .buttonStyle(KRButtonStyle(.primary)).krControlFocusRing(cornerRadius:KRRadius.radiusMd)
    }
}
/// Export geometry: settings span both rows; jobs span preview/check at 232pt.
public struct KRExportPageLayout<Settings: View, Preview: View, Check: View, Jobs: View>: View {
    private let settings: Settings
    private let preview: Preview
    private let check: Check
    private let jobs: Jobs
    public init(@ViewBuilder settings: () -> Settings, @ViewBuilder preview: () -> Preview, @ViewBuilder check: () -> Check, @ViewBuilder jobs: () -> Jobs) {
        self.settings = settings(); self.preview = preview(); self.check = check(); self.jobs = jobs()
    }
    public var body: some View {
        HStack(spacing: 0) {
            settings.frame(width: 320)
            VStack(spacing: 0) {
                HStack(spacing: 0) { preview.frame(maxWidth: .infinity); check.frame(width: 304) }.frame(maxHeight: .infinity)
                jobs.frame(height: 232)
            }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Shared query AABBs, not invented glyph outlines or a rendered pixel claim.
public struct KRTemplateVariantView: View {
    @Environment(\.krPalette) private var p
    public let name: String
    public let extent: CGSize
    public let bounds: [CGRect]
    public let stage: String
    public let diagnostic: KRDiagnostic?
    public init(_ name: String, extent: CGSize, bounds: [CGRect], stage: String, diagnostic: KRDiagnostic? = nil) {
        self.name = name; self.extent = extent; self.bounds = bounds; self.stage = stage; self.diagnostic = diagnostic
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space2) {
            HStack { KRIconView(.layoutTemplate); Text(name).krText(KRType.heading) }
            Text("\(String(format:"%.0f",extent.width))×\(String(format:"%.0f",extent.height)) · \(stage)").krText(KRMono.caption).foregroundStyle(p.inkMuted)
            if let diagnostic { KRErrorLine(diagnostic) }
            else { HStack(spacing: KRSpace.space1) { KRIconView(.circleCheck); Text("収まっています").krText(KRType.caption) }.foregroundStyle(p.inkMuted) }
            KRViewerFrame(aspectRatio: max(1, extent.width) / max(1, extent.height)) {
                Canvas { context, size in
                    for bounds in bounds {
                        let rect = CGRect(x: bounds.minX / max(1, extent.width) * size.width, y: bounds.minY / max(1, extent.height) * size.height,
                            width: bounds.width / max(1, extent.width) * size.width, height: bounds.height / max(1, extent.height) * size.height)
                        context.stroke(Path(rect), with: .color(diagnostic == nil ? p.selection : p.danger), style: StrokeStyle(lineWidth: 1, dash: diagnostic == nil ? [] : [4,3]))
                        context.draw(Text("\(String(format:"%.0f",bounds.width)) × \(String(format:"%.0f",bounds.height))").font(KRMono.caption.font()).foregroundColor(diagnostic == nil ? p.selection : p.danger), at: CGPoint(x: rect.midX, y: rect.maxY + 8))
                    }
                }
            }.frame(minHeight: 140)
            Text(bounds.isEmpty && diagnostic != nil ? "この variant の bounds は取得できません。入力または制約を直して再確認してください。" : "共有評価の bounds。画素・文字輪郭の描画結果ではありません。")
                .krText(KRType.caption).foregroundStyle(p.inkMuted).fixedSize(horizontal: false, vertical: true)
        }.padding(KRSpace.space3).background(p.surface0)
    }
}

public struct KRDurationPolicyBands: View {
    @Environment(\.krPalette) private var p
    public let authoring: String
    public let placement: String
    public let intro: String
    public let outro: String
    public let mode: String
    public let authoringSeconds: Double
    public let placementSeconds: Double
    public let introSeconds: Double
    public let outroSeconds: Double
    public init(authoring: String, placement: String, intro: String, outro: String, mode: String,
                authoringSeconds: Double, placementSeconds: Double, introSeconds: Double, outroSeconds: Double) {
        self.authoring = authoring; self.placement = placement; self.intro = intro; self.outro = outro; self.mode = mode
        self.authoringSeconds = authoringSeconds; self.placementSeconds = placementSeconds; self.introSeconds = introSeconds; self.outroSeconds = outroSeconds
    }
    // Floating values are used only for drawing the ruler, never authored time.
    private var scale: Double { max(0.001, max(authoringSeconds, placementSeconds)) }
    public var body: some View {
        VStack(alignment: .leading, spacing: KRSpace.space1) {
            HStack(spacing: KRSpace.space2) {
                Text("秒").krText(KRMono.caption).foregroundStyle(p.inkMuted).frame(width: 180, alignment: .leading)
                Canvas { context, size in
                    for index in 0...4 {
                        let x = CGFloat(index) / 4 * size.width
                        context.stroke(Path { path in path.move(to: CGPoint(x:x,y:11)); path.addLine(to:CGPoint(x:x,y:16)) },with:.color(p.lineStrong),lineWidth:1)
                        context.draw(Text(String(format:"%.2g",Double(index) / 4 * scale)).font(KRMono.caption.font()).foregroundColor(p.inkMuted),at:CGPoint(x:x,y:4),anchor:index == 0 ? .leading : index == 4 ? .trailing : .center)
                    }
                }.frame(height:16)
            }
            band("Authoring · " + authoring, duration:authoringSeconds)
            band("配置 · " + placement, duration:placementSeconds)
            Text("intro · " + intro + "（保護）  /  中間 · " + mode + "  /  outro · " + outro + "（保護）")
                .krText(KRMono.caption).foregroundStyle(p.inkMuted)
        }.padding(.horizontal, KRSpace.space3)
    }
    private func band(_ label: String, duration: Double) -> some View {
        HStack(spacing: KRSpace.space2) {
            Text(label).krText(KRMono.caption).frame(width: 180, alignment: .leading)
            GeometryReader { geometry in
                let total = max(0,duration), head = min(max(0,introSeconds),total)
                let tail = min(max(0,outroSeconds),max(0,total-head)), middle = max(0,total-head-tail)
                let unit = geometry.size.width / scale
                HStack(spacing:0) {
                    segment("",protected:true).frame(width:head * unit)
                    segment(mode,protected:false).frame(width:middle * unit)
                    segment("",protected:true).frame(width:tail * unit)
                }.frame(width:total * unit,height:24).accessibilityLabel(label + " · intro " + intro + " · " + mode + " · outro " + outro)
            }
        }.frame(height:24)
    }
    private func segment(_ label: String, protected: Bool) -> some View {
        Group { if protected { KRIconView(.lock,size:12) } else { Text(label).krText(KRMono.caption) } }
            .frame(maxWidth:.infinity,maxHeight:.infinity).background(protected ? p.clip : p.surface200)
            .overlay { Rectangle().strokeBorder(p.lineStrong,lineWidth:1) }.clipped()
    }
}
