import SwiftUI

/// Edit page panel dimensions from screens/edit.md, shared with the gallery.
/// FLOW-001 lets the app pass per-page panel visibility and sizes; the
/// defaults keep the designed geometry for call sites that opt out.
public struct KREditLayout<Project: View, Viewer: View, Inspector: View, Tracks: View>: View {
    private let project: Project
    private let viewer: Viewer
    private let inspector: Inspector
    private let tracks: Tracks
    private let projectPanel: Bool
    private let inspectorPanel: Bool
    private let tracksPanel: Bool
    private let projectWidth: Double
    private let inspectorWidth: Double
    private let tracksHeight: Double
    public init(projectPanel: Bool = true, inspectorPanel: Bool = true, tracksPanel: Bool = true,
                projectWidth: Double = 280, inspectorWidth: Double = 296, tracksHeight: Double = 312,
                @ViewBuilder project: () -> Project, @ViewBuilder viewer: () -> Viewer,
                @ViewBuilder inspector: () -> Inspector, @ViewBuilder tracks: () -> Tracks) {
        self.project = project(); self.viewer = viewer(); self.inspector = inspector(); self.tracks = tracks()
        self.projectPanel = projectPanel; self.inspectorPanel = inspectorPanel; self.tracksPanel = tracksPanel
        self.projectWidth = projectWidth; self.inspectorWidth = inspectorWidth; self.tracksHeight = tracksHeight
    }
    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                if projectPanel { project.frame(width: projectWidth) }
                viewer.frame(maxWidth: .infinity, maxHeight: .infinity)
                if inspectorPanel { inspector.frame(width: inspectorWidth) }
            }.frame(maxHeight: .infinity)
            if tracksPanel { tracks.frame(height: tracksHeight) }
        }
    }
}
