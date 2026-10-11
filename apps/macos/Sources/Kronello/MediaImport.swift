import AppKit
import SwiftUI
import UniformTypeIdentifiers
import KronelloAppModel
import KronelloDesign

struct MediaImportButton: View {
    @ObservedObject var model: EditorModel
    var body: some View {
        KRButton("メディアを読み込む…", icon: .fileInput, variant: .secondary) {
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [.movie, .audio, .image]
            panel.canChooseFiles = true
            panel.canChooseDirectories = false
            panel.allowsMultipleSelection = true
            panel.prompt = "メディアを読み込む"
            if panel.runModal() == .OK {
                let urls = panel.urls
                Task { await model.importMedia(urls: urls) }
            }
        }.disabled(model.busy || model.pendingCandidate != nil || model.safeMode)
    }
}

extension View {
    func mediaFileDrop(model: EditorModel) -> some View {
        dropDestination(for: URL.self) { urls, _ in
            guard !model.busy, model.pendingCandidate == nil, !model.safeMode,
                  !urls.isEmpty, urls.allSatisfy(\.isFileURL) else { return false }
            Task { await model.importMedia(urls: urls) }
            return true
        }
    }
}
