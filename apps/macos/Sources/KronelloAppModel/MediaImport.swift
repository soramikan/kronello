import Foundation

extension EditorModel {
    /// Each file is one shared revision-checked edit and one undo step.
    /// Stop on failure so the existing error UI retains the actionable error.
    public func importMedia(urls: [URL]) async {
        guard !busy, !safeMode, pendingCandidate == nil, !urls.isEmpty else { return }
        busy = true
        defer { busy = false }
        for url in urls {
            do {
                guard url.isFileURL else { throw ServiceFailure(code: "INVALID_REQUEST", message: "ローカルファイルを選択してください。") }
                let event = try await request("media.import", [
                    "base_revision": revision, "path": url.path,
                    "asset": UUID().uuidString.lowercased(), "session_id": sessionID,
                    "idempotency_key": UUID().uuidString.lowercased(),
                ])
                recordIssuedEvent(event, label: "メディアの読み込み")
                try await reload()
            } catch { mapFailure(error); return }
        }
    }
}
