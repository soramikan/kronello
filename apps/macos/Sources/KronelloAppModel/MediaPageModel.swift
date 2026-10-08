import Foundation
import Combine
import CoreGraphics
import KronelloDesign

/// FLOW-002 media browser state (ADR-0129). Assets, bins and availability come
/// from the shared `media.query` result; bin edits go through the same
/// edit.plan/apply commands as every other transport, relinking calls the
/// shared `asset.relink` operation, and thumbnails stay an in-memory cache —
/// the document never stores GUI preview pixels.
@MainActor public final class MediaPageModel: ObservableObject {
    public let editor: EditorModel
    /// One decoded `asset.thumbnail` result: straight opaque RGBA8 pixels.
    public struct Thumbnail: Equatable, Sendable {
        public let width: Int
        public let height: Int
        public let rgba: Data
        public init(width: Int, height: Int, rgba: Data) {
            self.width = width; self.height = height; self.rgba = rgba
        }
    }
    @Published public private(set) var assets: [[String: Any]] = []
    @Published public private(set) var bins: [[String: Any]] = []
    @Published public private(set) var mediaRevision = ""
    @Published public var binSelection: String?
    @Published public var assetSelection: String?
    @Published public var search = ""
    @Published public private(set) var thumbnails: [String: Thumbnail] = [:]
    /// Permanent per-asset thumbnail failures (typed code) to avoid retry loops.
    @Published public private(set) var thumbnailFailures: [String: ServiceFailure] = [:]
    @Published public private(set) var thumbnailPending: Set<String> = []
    @Published public private(set) var loading = false
    @Published public var failure: ServiceFailure?
    public init(editor: EditorModel) { self.editor = editor }
    /// Browser grid thumbnails are requested at this longest-edge cap.
    public nonisolated static let thumbnailSize = 128
    public func request(_ operation: String, _ fields: [String: Any] = [:]) async throws -> [String: Any] {
        try await editor.request(operation, fields)
    }
    /// Query the shared media list. Probe results are presentation data; bins
    /// additionally ride `editor.document`, so this just re-reads both through
    /// one authoritative response.
    public func refresh() async {
        loading = true; defer { loading = false }
        do {
            let result = try await request("media.query")
            assets = result.objects("assets")
            bins = result.objects("bins")
            mediaRevision = result.string("revision")
            if let selected = binSelection, !bins.contains(where: { $0.string("id") == selected }) { binSelection = nil }
            if let selected = assetSelection, !assets.contains(where: { $0.string("asset") == selected }) { assetSelection = nil }
            failure = nil
        } catch { failure = editor.serviceFailure(error) }
    }
    public var selectedBin: [String: Any] { bins.first { $0.string("id") == binSelection } ?? [:] }
    public var visibleAssets: [[String: Any]] {
        var listed = assets
        if binSelection != nil {
            let members = Set(selectedBin["assets"] as? [String] ?? [])
            listed = listed.filter { members.contains($0.string("asset")) }
        }
        if !search.isEmpty {
            listed = listed.filter { name(of: $0).localizedCaseInsensitiveContains(search) }
        }
        return listed
    }
    /// Display name: the locator file name, falling back to a short id.
    public func name(of entry: [String: Any]) -> String {
        let locator = entry.object("detail").object("locator")
        let file = locator.string("relative").isEmpty ? locator.string("absolute") : locator.string("relative")
        let name = URL(fileURLWithPath: file).lastPathComponent
        return name.isEmpty ? String(entry.string("asset").prefix(8)) : name
    }
    public func kind(of entry: [String: Any]) -> String { entry.object("detail").string("kind") }
    public func availability(of entry: [String: Any]) -> String { entry.string("availability") }
    /// Offline badge: anything the shared cheap locator probe did not confirm.
    public func isOffline(_ entry: [String: Any]) -> Bool { availability(of: entry) != "present_unverified" }
    public func error(of entry: [String: Any]) -> ServiceFailure? {
        let raw = entry.object("error")
        guard !raw.isEmpty else { return nil }
        return .init(code: raw.string("code"), message: raw.string("message"), details: raw.object("details"))
    }
    public func sizeText(of entry: [String: Any]) -> String {
        let bytes = entry.number("size_bytes")
        guard bytes > 0 else { return "—" }
        let units = ["B", "KB", "MB", "GB", "TB"]
        var value = bytes, unit = 0
        while value >= 1024, unit + 1 < units.count { value /= 1024; unit += 1 }
        return String(format: unit == 0 ? "%.0f %@" : "%.1f %@", value, units[unit])
    }
    public func binsContaining(_ asset: String) -> Set<String> {
        Set(bins.filter { ($0["assets"] as? [String] ?? []).contains(asset) }.map { $0.string("id") })
    }
    /// Bin edits are document mutations through the shared apply path; the
    /// apply already reloads the document, then `refresh` re-runs the probe.
    private func edit(_ commands: [[String: Any]], label: String) async {
        _ = await editor.apply(.init(base: editor.revision, commands: commands, label: label))
        await refresh()
    }
    public func createBin(name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        await edit([["bin_create": ["bin": ["id": UUID().uuidString.lowercased(), "name": name, "assets": [String]()]]]],
                   label: "ビンの作成")
    }
    public func renameBin(_ bin: String, name: String) async {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        await edit([["bin_rename": ["bin": bin, "name": name]]], label: "ビンの名前変更")
    }
    public func deleteBin(_ bin: String) async {
        await edit([["bin_delete": ["bin": bin]]], label: "ビンの削除")
    }
    /// Toggle one asset's membership in one bin. `bin_assign` replaces the
    /// member set wholesale, so the command carries the complete next set.
    public func setMembership(_ asset: String, in bin: String, member: Bool) async {
        guard let existing = bins.first(where: { $0.string("id") == bin }) else { return }
        var members = existing["assets"] as? [String] ?? []
        if member {
            guard !members.contains(asset) else { return }
            members.append(asset)
        } else {
            guard members.contains(asset) else { return }
            members.removeAll { $0 == asset }
        }
        await edit([["bin_assign": ["bin": bin, "assets": members]]], label: "ビンへの割り当て")
    }
    /// Offline assets relink through the shared search operation; the result
    /// updates the document via `project.import`, so the editor reloads too.
    public func relink(_ asset: String, searchDirectory: String) async {
        do {
            _ = try await request("asset.relink", ["base_revision": editor.revision, "asset": asset, "search_directory": searchDirectory])
            try await editor.reload()
            await refresh()
        } catch { failure = editor.serviceFailure(error) }
    }
    /// Lazily request one fixed-snapshot thumbnail per asset. Asset content is
    /// pinned by `content_hash`, so a cached result stays valid for the whole
    /// session; permanent failures cache the typed error to stop retry loops.
    public func ensureThumbnail(_ asset: String, maxSize: Int = MediaPageModel.thumbnailSize) {
        guard thumbnails[asset] == nil, thumbnailFailures[asset] == nil, !thumbnailPending.contains(asset) else { return }
        thumbnailPending.insert(asset)
        Task { [weak self] in
            guard let self else { return }
            defer { self.thumbnailPending.remove(asset) }
            do {
                let result = try await self.request("asset.thumbnail", ["asset": asset, "max_size": maxSize])
                let bytes = Data((result["rgba"] as? [NSNumber] ?? []).map { UInt8(clamping: $0.intValue) })
                self.thumbnails[asset] = .init(width: Int(result.number("width")), height: Int(result.number("height")), rgba: bytes)
            } catch {
                self.thumbnailFailures[asset] = self.editor.serviceFailure(error)
            }
        }
    }
    /// Decode cached straight-alpha RGBA8 pixels into an image for the grid.
    /// `CGContext` cannot host non-premultiplied `last` alpha, so the image is
    /// built directly from the provider instead.
    public func image(for asset: String) -> CGImage? {
        guard let thumbnail = thumbnails[asset],
              thumbnail.rgba.count == thumbnail.width * thumbnail.height * 4,
              thumbnail.width > 0, thumbnail.height > 0,
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let provider = CGDataProvider(data: thumbnail.rgba as CFData) else { return nil }
        return CGImage(width: thumbnail.width, height: thumbnail.height,
                       bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: thumbnail.width * 4,
                       space: space,
                       bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: false,
                       intent: .defaultIntent)
    }
}
