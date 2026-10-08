import Foundation

/// GUI-012: actionable job list for the edit-page jobs sheet. All operations
/// are the shared `job.*` queries — the same calls CLI/MCP and the export page
/// use. `job_progress` notifications already feed `jobs`; these helpers add
/// refresh-on-open plus cancel/resume/prune for failed and terminal rows.
extension EditorModel {
    /// `job.*` are projectless operations: `request` injects `project`, which
    /// the strict request schema rejects. Scope the shared queue by
    /// `project_id` on the client instead.
    private func jobRequest(_ operation: String, _ fields: [String: Any] = [:]) async throws -> [String: Any] {
        var request = fields; request["operation"] = operation
        return try await transport.call(request)
    }
    /// Refreshes `jobs` from the shared service, scoped to this project.
    public func refreshJobs() async {
        do { jobs = try await jobRequest("job.list").objects("jobs").filter { $0.string("project_id") == projectID } }
        catch { mapFailure(error) }
    }
    /// Cancel a queued/running job. The service answers with the updated record.
    public func cancelJob(_ id: String) async {
        do { _ = try await jobRequest("job.cancel", ["job": id]); await refreshJobs() }
        catch { mapFailure(error) }
    }
    /// Resume an interrupted/failed job from its recorded checkpoint.
    public func resumeJob(_ id: String) async {
        do { _ = try await jobRequest("job.resume", ["job": id]); await refreshJobs() }
        catch { mapFailure(error) }
    }
    /// Remove terminal jobs from the shared queue store. `job.prune` is a
    /// service-level operation with no per-project filter; the refresh keeps
    /// the sheet scoped to this project.
    public func pruneJobs() async {
        do { _ = try await jobRequest("job.prune"); await refreshJobs() }
        catch { mapFailure(error) }
    }
    /// Status groups the sheet needs: resumable rows plus cancellable rows.
    public static func jobCanCancel(_ job: [String: Any]) -> Bool {
        ["queued", "running"].contains(job.string("status"))
    }
    public static func jobCanResume(_ job: [String: Any]) -> Bool {
        ["interrupted", "failed"].contains(job.string("status"))
    }

    // MARK: - GUI-012 guided missing-asset recovery

    /// The concrete asset id a missing clip references, if its `source_ref`
    /// points at a single asset. Multicam clips resolve through the active
    /// angle so the guided relink fixes the right stream.
    public func clipMissingAsset(_ clip: EditClip) -> String? {
        guard clipMissing(clip) != nil else { return nil }
        let source = clip.authored.object("source_ref")
        if let multicam = clip.multicam {
            return multicamGroup(multicam.group)?.angles.first { $0.id == multicam.angle }?.asset
        }
        guard source.string("kind") == "asset" else { return nil }
        let asset = source.string("asset")
        return asset.isEmpty ? nil : asset
    }
    /// Guided relink for a missing clip asset: the shared `asset.relink`
    /// search resolves the file under the picked directory. The document
    /// updates via `project.import` inside the service, so reloads pick the
    /// recovered path up for every client.
    public func relinkAsset(_ asset: String, searchDirectory: String) async {
        do {
            _ = try await request("asset.relink", ["base_revision": revision, "asset": asset, "search_directory": searchDirectory])
            try await reload()
        } catch { mapFailure(error) }
    }
}
