import AppKit
import SwiftUI
import KronelloAppModel
import KronelloDesign

/// FLOW-002 media browser (ADR-0129): bins and asset availability come from
/// the shared `media.query`; membership edits are shared edit commands; offline
/// assets relink through `asset.relink`; thumbnails are an in-memory cache.
struct MediaPage: View {
    @Environment(\.krPalette) var p
    @ObservedObject var editor: EditorModel
    @StateObject private var model: MediaPageModel
    @State private var binName = ""
    init(model: EditorModel) {
        editor = model
        _model = StateObject(wrappedValue: MediaPageModel(editor: model))
    }
    var body: some View {
        HStack(spacing: 0) {
            binsPanel.frame(width: 232)
            p.line.frame(width: 1)
            browser
        }
        .task { await model.refresh() }
        .onChange(of: editor.refreshToken) { _, _ in Task { await model.refresh() } }
    }
    var binsPanel: some View {
        KRPanel("ビン") {
            VStack(spacing: 0) {
                ScrollView {
                    VStack(spacing: 0) {
                        binRow("すべて", count: model.assets.count, selected: model.binSelection == nil) {
                            model.binSelection = nil
                        }
                        ForEach(model.bins, id: \.selfID) { bin in
                            binRow(bin.string("name"), count: (bin["assets"] as? [String] ?? []).count,
                                   selected: model.binSelection == bin.string("id")) {
                                model.binSelection = bin.string("id")
                            }
                        }
                    }
                }
                Spacer(minLength: 0)
                p.line.frame(height: 1)
                VStack(spacing: KRSpace.space2) {
                    KRTextField(model.binSelection == nil ? "新しいビンの名前" : "ビンの名前", value: $binName)
                    HStack(spacing: KRSpace.space2) {
                        if let bin = model.binSelection {
                            KRButton("名前変更", variant: .secondary) {
                                Task {
                                    await model.renameBin(bin, name: binName)
                                    binName = ""
                                }
                            }.disabled(binName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            KRButton("削除", variant: .plain) {
                                Task { await model.deleteBin(bin) }
                            }
                        } else {
                            KRButton("作成", variant: .secondary) {
                                Task {
                                    await model.createBin(name: binName)
                                    binName = ""
                                }
                            }.disabled(binName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        }
                    }
                }.padding(KRSpace.space3)
            }
        }
    }
    func binRow(_ name: String, count: Int, selected: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: KRSpace.space2) {
                KRIconView(.folder).foregroundStyle(p.inkMuted)
                Text(name).krText(KRType.body).foregroundStyle(p.ink).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text("\(count)").krText(KRType.caption).foregroundStyle(p.inkMuted)
            }.padding(.horizontal, KRSpace.space3).frame(height: KRSize.rowHeight)
            .background(selected ? p.selectionBg : .clear)
            .contentShape(Rectangle())
        }.buttonStyle(.plain)
    }
    var browser: some View {
        KRPanel(header: {
            KRPanelTitle("メディア")
            KRSearchField("メディアを検索", text: $model.search)
        }, actions: {
            if let error = model.failure { KRErrorLine(.init(error.code, error.message)) }
            KRButton(icon: .refreshCw, accessibilityLabel: "メディアを更新") { Task { await model.refresh() } }
                .disabled(model.loading)
        }) {
            VStack(spacing: 0) {
                ScrollView {
                    if model.visibleAssets.isEmpty {
                        KREmptyState(icon: .film,
                            title: model.assets.isEmpty ? "メディアがありません" : "このビンは空です",
                            message: model.assets.isEmpty
                                ? "クリップや .cube LUT を読み込むとここに表示されます。"
                                : "コンテキストメニューからアセットをこのビンに割り当てられます。")
                            .padding(KRSpace.space4)
                    } else {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 176), spacing: KRSpace.space3)], spacing: KRSpace.space3) {
                            ForEach(model.visibleAssets, id: \.assetID) { asset in cell(asset) }
                        }.padding(KRSpace.space3)
                    }
                }
                Spacer(minLength: 0)
                if let selected = model.assetSelection, let entry = model.assets.first(where: { $0.string("asset") == selected }) {
                    detail(entry)
                }
            }
        }
    }
    func cell(_ entry: [String: Any]) -> some View {
        let id = entry.string("asset")
        let offline = model.isOffline(entry)
        return VStack(alignment: .leading, spacing: KRSpace.space1) {
            ZStack {
                RoundedRectangle(cornerRadius: KRRadius.radiusMd).fill(p.surface200)
                if let image = model.image(for: id) {
                    Image(nsImage: NSImage(cgImage: image, size: .zero))
                        .resizable().aspectRatio(contentMode: .fit)
                } else if offline {
                    VStack(spacing: KRSpace.space1) {
                        KRIconView(.triangleAlert).foregroundStyle(p.danger)
                        Text("オフライン").krText(KRType.caption).foregroundStyle(p.danger)
                    }
                } else if let failure = model.thumbnailFailures[id] {
                    VStack(spacing: KRSpace.space1) {
                        KRIconView(mediaKind(of: entry).icon).foregroundStyle(p.inkMuted)
                        Text(failure.code).krText(KRType.ruler).foregroundStyle(p.inkMuted)
                    }
                } else {
                    KRIconView(mediaKind(of: entry).icon).foregroundStyle(p.inkMuted)
                        .onAppear { model.ensureThumbnail(id) }
                }
            }.frame(height: 96).clipped()
            Text(model.name(of: entry)).krText(KRType.body).foregroundStyle(offline ? p.danger : p.ink).lineLimit(1)
            HStack(spacing: KRSpace.space1) {
                Text(model.kind(of: entry)).krText(KRType.caption).foregroundStyle(p.inkMuted)
                Spacer(minLength: 0)
                Text(model.sizeText(of: entry)).krText(KRType.ruler).foregroundStyle(p.inkMuted)
            }
            if let error = model.error(of: entry) {
                Text(error.code).krText(KRType.ruler).foregroundStyle(p.danger).lineLimit(1)
            }
        }
        .padding(KRSpace.space2)
        .background(model.assetSelection == id ? p.selectionBg : .clear)
        .clipShape(RoundedRectangle(cornerRadius: KRRadius.radiusMd))
        .contentShape(Rectangle())
        .onTapGesture { model.assetSelection = id }
        .contextMenu { assetMenu(entry) }
        .accessibilityLabel(model.name(of: entry))
    }
    @ViewBuilder func assetMenu(_ entry: [String: Any]) -> some View {
        let id = entry.string("asset")
        if !model.bins.isEmpty {
            Menu("ビンに割り当て") {
                ForEach(model.bins, id: \.selfID) { bin in
                    let member = model.binsContaining(id).contains(bin.string("id"))
                    Button(member ? "✓ " + bin.string("name") : bin.string("name")) {
                        Task { await model.setMembership(id, in: bin.string("id"), member: !member) }
                    }
                }
            }
        }
        if model.isOffline(entry) {
            Button("再リンク…") { relink(id) }
        }
    }
    func detail(_ entry: [String: Any]) -> some View {
        let id = entry.string("asset")
        let locator = entry.object("detail").object("locator")
        let file = locator.string("relative").isEmpty ? locator.string("absolute") : locator.string("relative")
        return VStack(alignment: .leading, spacing: KRSpace.space2) {
            p.line.frame(height: 1)
            HStack(spacing: KRSpace.space3) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(model.name(of: entry)).krText(KRType.heading)
                    Text(file).krText(KRType.caption).foregroundStyle(p.inkMuted).lineLimit(1).truncationMode(.middle)
                    Text("\(id) · \(model.kind(of: entry)) · \(model.availability(of: entry))").krText(KRType.ruler).foregroundStyle(p.inkMuted)
                }
                Spacer(minLength: 0)
                if let error = model.error(of: entry) {
                    Text(error.message).krText(KRType.caption).foregroundStyle(p.danger).lineLimit(2)
                }
                if model.isOffline(entry) {
                    KRButton("再リンク…", variant: .secondary) { relink(id) }
                }
            }.padding(KRSpace.space3)
        }
    }
    func relink(_ asset: String) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false
        panel.prompt = "このフォルダから再リンク"
        if panel.runModal() == .OK, let url = panel.url {
            Task { await model.relink(asset, searchDirectory: url.path) }
        }
    }
    func mediaKind(of entry: [String: Any]) -> KRMediaKind {
        switch model.kind(of: entry) {
        case "video": return .video
        case "image": return .image
        case "audio": return .audio
        default: return .generator
        }
    }
}
private extension Dictionary where Key == String, Value == Any { var assetID: String { string("asset") } }
