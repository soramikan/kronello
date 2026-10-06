import SwiftUI

/// Edit page panel dimensions from screens/edit.md, shared with the gallery.
public struct KREditLayout<Project: View, Viewer: View, Inspector: View, Tracks: View>: View {
    private let project: Project
    private let viewer: Viewer
    private let inspector: Inspector
    private let tracks: Tracks
    public init(@ViewBuilder project: () -> Project, @ViewBuilder viewer: () -> Viewer,
                @ViewBuilder inspector: () -> Inspector, @ViewBuilder tracks: () -> Tracks) {
        self.project = project(); self.viewer = viewer(); self.inspector = inspector(); self.tracks = tracks()
    }
    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                project.frame(width: 280)
                viewer.frame(maxWidth: .infinity, maxHeight: .infinity)
                inspector.frame(width: 296)
            }.frame(maxHeight: .infinity)
            tracks.frame(height: 312)
        }
    }
}
