import Foundation
import KronelloDesign
import Darwin

public struct WorkspaceState: Codable, Equatable, Sendable {
    public var leftWidth: Double = 248
    public var rightWidth: Double = 304
    public var bottomHeight: Double = 344
    public var tools = KRToolStripPlacement()
    public var valuesVisible = true
    /// GUI-012: edit-page mixer strip visibility, per workspace layout.
    public var mixerVisible = false
    /// GUI-012: monitor scopes panel visibility, per workspace layout.
    public var scopesVisible = false
    public init() {}
}

public struct CanvasGuide: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var axis: String
    public var position: Double
    public init(axis: String, position: Double) { self.id = UUID().uuidString; self.axis = axis; self.position = position }
}
public struct CanvasSettings: Codable, Equatable, Sendable {
    public var snap = true
    public var guidesVisible = true
    public var guides: [CanvasGuide] = []
    public init() {}
}
public struct EditViewSettings: Codable, Equatable, Sendable {
    public var panX = 0.0
    public var panY = 0.0
    /// GUI-012: persisted timeline zoom factor (px per frame relative scale).
    public var editScale = 1.0
    /// GUI-012: persisted snapping flag for edit gestures.
    public var editSnap = true
    /// GUI-012: last source-monitor destination track override.
    public var sourceTrack: String?
    public init() {}
}
public struct LockedFontSource: Codable, Equatable, Sendable {
    public var family: String
    public var postscriptName: String
    public var sha256: String
    public var faceIndex: Int
    public var path: String
    public init(identity: [String: Any], path: String) {
        family = identity.string("family"); postscriptName = identity.string("postscript_name"); sha256 = identity.string("sha256"); faceIndex = Int(identity.number("face_index")); self.path = path
    }
    public var identity: [String: Any] { ["family": family, "postscript_name": postscriptName, "sha256": sha256, "face_index": faceIndex] }
}

public struct ProjectUIState: Codable, Equatable, Sendable {
    public var canvas: CanvasSettings?
    public var editView: EditViewSettings?
    public var fontSources: [LockedFontSource]?
    public var page = "motion"
    public var workspace = "standard"
    public var layouts: [String: WorkspaceState] = ["standard": .init()]
    public var composition: String?
    public var sequence: String?
    public var clipSelection: String?
    public var selection: String?
    /// Motion-page layer locks only (NLE-005 moved track locks into the
    /// document's TrackState; use `trackLocked`/`setTrackLocked` for lanes).
    public var locked: Set<String> = []
    public var collapsed: Set<String> = []
    public var zoom = "fit"
    public var panX: Double = 0
    public var panY: Double = 0
    public var tool = "select"
    public var bounds = "layout"
    public var resolution = "full"
    public var looping = false
    public var time = RationalTime(num: 0, den: 1)
    public init() {}
    public var layout: WorkspaceState {
        get { layouts[workspace] ?? .init() }
        set { layouts[workspace] = newValue }
    }
}

/// UI time is an integer fraction. Display geometry may use Double; persisted time never does.
public struct RationalTime: Codable, Equatable, Hashable, Sendable {
    public let num: String
    public let den: String
    public init(num: Int64, den: Int64) {
        let denominator = max(1, den)
        var a = num.magnitude, b = denominator.magnitude
        while b != 0 { let r = a % b; a = b; b = r }
        let divisor = Int64(max(1, a))
        self.num = String(num / divisor); self.den = String(denominator / divisor)
    }
    public var wire: [String: Any] { ["num": num, "den": den] }
    public func frames(rateNum: Int64, rateDen: Int64) -> Int64 {
        guard let n = Int64(num), let d = Int64(den), d > 0, rateDen > 0 else { return 0 }
        let product = n.multipliedReportingOverflow(by: rateNum)
        let divisor = d.multipliedReportingOverflow(by: rateDen)
        guard !product.overflow, !divisor.overflow, divisor.partialValue > 0 else { return 0 }
        return product.partialValue / divisor.partialValue
    }
}

public struct RecentProject: Codable, Equatable, Identifiable, Sendable {
    public var id: String { path }
    public let path: String
    public let name: String
    public let opened: Date
    public init(path: String, name: String, opened: Date = Date()) { self.path = path; self.name = name; self.opened = opened }
}

public struct AppPreferences: Codable, Sendable {
    public var light = false
    public var showWelcome = true
    public var recent: [RecentProject] = []
    /// GUI-012: playback defaults applied when a project session opens.
    /// Scrub and monitor mute are session flags; looping only seeds projects
    /// that have no persisted UI state yet.
    public var playbackScrub = true
    public var playbackMuted = false
    public var playbackLooping = false
    /// GUI-012: raster scratch/cache root, exported to the worker as
    /// `KRONELLO_RASTER_CACHE_ROOT` before a session opens. Empty keeps the
    /// platform default (`~/Library/Caches/kronello/raster` on macOS).
    public var scratchDirectory = ""
    public init() {}
}

/// This actor has no project path, store connection, or document-writing API.
public actor UIStateStore {
    public let root: URL
    public init(root: URL? = nil, environment: [String: String] = ProcessInfo.processInfo.environment) {
        self.root = root ?? environment["KRONELLO_STATE_ROOT"].map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Kronello", isDirectory: true)
    }
    public func stateURL(projectID: String) throws -> URL {
        guard let id = UUID(uuidString: projectID) else { throw StateError.invalidProjectID }
        return root.appendingPathComponent("ui-state", isDirectory: true).appendingPathComponent(id.uuidString.lowercased() + ".json")
    }
    public func load(projectID: String) throws -> ProjectUIState {
        try read(ProjectUIState.self, at: stateURL(projectID: projectID)) ?? .init()
    }
    /// Whether a persisted state file exists; playback defaults only apply
    /// when the project has never written UI state (GUI-012).
    public func hasState(projectID: String) throws -> Bool {
        FileManager.default.fileExists(atPath: try stateURL(projectID: projectID).path)
    }
    public func save(_ state: ProjectUIState, projectID: String) throws { try write(state, at: stateURL(projectID: projectID)) }
    public func preferences() throws -> AppPreferences { try read(AppPreferences.self, at: root.appendingPathComponent("preferences.json")) ?? .init() }
    public func savePreferences(_ value: AppPreferences) throws { try write(value, at: root.appendingPathComponent("preferences.json")) }
    private func read<T: Decodable>(_ type: T.Type, at url: URL) throws -> T? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        return try JSONDecoder().decode(type, from: Data(contentsOf: url))
    }
    private func write<T: Encodable>(_ value: T, at url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        // Keep the temporary file beside its destination, including in a restricted sandbox.
        let temporary = url.deletingLastPathComponent().appendingPathComponent(UUID().uuidString + ".tmp")
        defer { try? FileManager.default.removeItem(at: temporary) }
        try JSONEncoder().encode(value).write(to: temporary, options: .withoutOverwriting)
        guard rename(temporary.path, url.path) == 0 else { throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO) }
    }
    public enum StateError: Error { case invalidProjectID }
}
