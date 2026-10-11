import XCTest
@testable import KronelloAppModel

final class TimelineIndexTests: XCTestCase {
    @MainActor func testQueryAndDocumentChangesReplacePresentationIndexes() throws {
        let model = EditorModel(path: "/tmp/timeline-index.kronello", transport: FakeTransport())
        var document = EditorModel.newDocument(name: "Index")
        document["compositions"] = []
        document["assets"] = [["id": "asset", "kind": "video", "locator": ["absolute": "/tmp/original.mov"],
            "streams": [["index": 0, "width": 32, "height": 24, "duration": ["num": "4", "den": "1"]]]]]
        model.adopt(document: document, scene: [:], revision: "1", actor: "test", external: false)
        model.ui.page = "edit"
        let clip: [String: Any] = ["track": "track", "kind": "video", "clip": ["id": "clip",
            "source_ref": ["kind": "asset", "asset": "asset", "stream_index": 0],
            "timeline_range": ["start": ["num": "0", "den": "1"], "end": ["num": "2", "den": "1"]]]]
        var query: [String: Any] = ["sequence": ["frame_rate": ["num": "30", "den": "1"],
            "tracks": [["id": "track", "kind": "video", "state": ["locked": false]]]],
            "clips": [clip], "asset_status": [["asset": "asset", "error": ["code": "ASSET_MISSING"]]]]
        model.sequenceResult = query; model.ui.clipSelection = "clip"
        let selected = try XCTUnwrap(model.selectedClip)
        XCTAssertEqual(model.clips(in: "track").map(\.id), ["clip"])
        XCTAssertEqual(model.sequenceDurationFrames, 60)
        XCTAssertEqual(model.clipName(selected), "original.mov · stream 0")
        XCTAssertEqual(model.clipMissing(selected), "ASSET_MISSING")
        XCTAssertFalse(model.trackLocked("track"))
        document["assets"] = [["id": "asset", "kind": "video", "locator": ["absolute": "/tmp/renamed.mov"],
            "streams": [["index": 0, "width": 32, "height": 24]]]]
        model.adopt(document: document, scene: [:], revision: "2", actor: "external", external: true)
        XCTAssertEqual(model.clipName(selected), "renamed.mov · stream 0")
        query["asset_status"] = []
        query["sequence"] = ["frame_rate": ["num": "24", "den": "1"],
            "tracks": [["id": "track", "kind": "video", "state": ["locked": true]]]]
        model.sequenceResult = query
        XCTAssertEqual(model.sequenceDurationFrames, 48)
        XCTAssertNil(model.clipMissing(selected))
        XCTAssertTrue(model.trackLocked("track"))
        query["clips"] = []; model.sequenceResult = query
        model.validateClipSelection(actor: "external")
        XCTAssertNil(model.selectedClip); XCTAssertNil(model.ui.clipSelection)
        XCTAssertTrue(model.clips(in: "track").isEmpty)
    }

    @MainActor func testProxyPreferenceDecodesOldStateAndNormalizesWireChoice() throws {
        let state = ProjectUIState()
        var json = try XCTUnwrap(JSONSerialization.jsonObject(with: JSONEncoder().encode(state)) as? [String: Any])
        json.removeValue(forKey: "mediaProxies")
        let old = try JSONDecoder().decode(ProjectUIState.self, from: JSONSerialization.data(withJSONObject: json))
        XCTAssertNil(old.mediaProxies)
        let model = EditorModel(path: "/tmp/proxy-state.kronello", transport: FakeTransport())
        model.ui = old; XCTAssertEqual(model.previewProxyMode, "off")
        model.previewProxyMode = "prefer"
        let restored = try JSONDecoder().decode(ProjectUIState.self, from: JSONEncoder().encode(model.ui))
        XCTAssertEqual(restored.mediaProxies, "prefer")
        model.previewProxyMode = "unknown"; XCTAssertEqual(model.previewProxyMode, "off")
        XCTAssertNil(TimelineSourceKey(["kind": "asset", "asset": "asset", "stream_index": 0, "future": true]))
    }
}
