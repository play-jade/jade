import Foundation
import JadeCoreBindings

/// A draft store whose writes fail on demand, as a full disk would make them.
final class FlakyStore: ReplayDraftStore {
    var failSaves = false
    var failLoads = false

    override func load(_ draftId: String) -> DraftRead {
        failLoads ? .unavailable(reason: "The disk did not answer.") : super.load(draftId)
    }

    override func save(_ draft: ReplayDraft) -> String? {
        failSaves ? "The disk is full." : super.save(draft)
    }
}

/// Replay session and local draft behaviour through the UniFFI Swift binding and the sample archive.
enum ReplaySessionProof {
    static func expect(_ condition: Bool, _ message: @autoclosure () -> String) {
        if !condition { fatalError("Replay proof: \(message())") }
    }

    static func run(repoRoot: URL) throws {
        let scratch = FileManager.default.temporaryDirectory
            .appendingPathComponent("jade-replay-proof-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: scratch) }

        // The sample archive answers through the archive contract, labelled as a synthetic fixture.
        let page = try JSONSerialization.jsonObject(
            with: Data(try ffiSampleArchiveGamesJson(offset: 0, limit: 10).utf8)) as! [String: Any]
        let release = page["release"] as! [String: Any]
        expect(release["is_synthetic_fixture"] as? Bool == true, "sample release must be labelled synthetic")
        let gameId = (page["games"] as! [[String: Any]])[0]["game_id"] as! String
        let detail = try JSONSerialization.jsonObject(
            with: Data(try ffiSampleArchiveGameDetailJson(gameId: gameId).utf8)) as! [String: Any]
        let record = detail["record"] as! [String: Any]
        let recorded = record["moves_uci"] as! [String]
        let rootFen = record["initial_fen"] as! String
        let source = ReplaySource(kind: "archive", id: gameId, revision: release["release_id"] as! String)
        expect(recorded.count >= 3, "sample game needs at least three plies")

        func session(_ store: ReplayDraftStore?, source: ReplaySource = source) -> ReplaySession {
            ReplaySession(source: source, rootFen: rootFen, rulesProfile: "program-2023.11-v1",
                          recordedMovesUci: recorded, store: store)
        }
        let store = ReplayDraftStore(directory: scratch)

        // Following the record moves along it; another legal move starts a variation and is kept.
        let s = session(store)
        expect(s.tryMove(recorded[0]) == .followed, "the recorded move follows the record")
        expect(s.cursor == 1 && !s.isInVariation, "following moves the cursor only")
        let legal = try s.positionSession().summary().legalMovesUci
        let alternative = legal.first { $0 != recorded[1] }!
        expect(s.tryMove(alternative) == .variation, "an alternative starts a variation")
        expect(s.activePath == [recorded[0], alternative], "the active path follows the variation")
        expect(s.saveState == .saved, "the variation is kept on this device")

        // An illegal move is refused with feedback and changes nothing.
        expect(s.tryMove("a0a9") != .variation && s.feedback != nil, "an illegal move is refused")
        expect(s.activePath == [recorded[0], alternative], "a refused move leaves the path")

        // Return to game parks the line at its origin; Resume brings it back.
        s.returnToRecordedLine()
        expect(!s.isInVariation && s.parked != nil && s.cursor == 1, "return parks the line at its origin")
        s.resumeVariation()
        expect(s.isInVariation && s.parked == nil, "resume brings the line back")

        // Selecting a recorded ply leaves the variation and keeps the selected position.
        s.selectPly(3)
        expect(s.cursor == 3 && s.parked != nil, "navigation parks the line")
        guard case .found(let stored) = store.load(source.draftId) else { fatalError("Replay proof: draft missing") }
        expect(stored.cursor == 3 && stored.activeMode == "recorded", "the draft records the selected ply")

        // Navigation after the line is parked still updates where a relaunch returns.
        s.selectPly(2)
        guard case .found(let moved) = store.load(source.draftId), moved.cursor == 2 else {
            fatalError("Replay proof: navigation after parking must update the kept cursor")
        }
        s.selectPly(3)

        // On disk the draft has the web record's shape: the owner-less key and an explicit null owner.
        let file = try FileManager.default.contentsOfDirectory(at: scratch, includingPropertiesForKeys: nil)[0]
        let onDisk = try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as! [String: Any]
        expect(onDisk["draftId"] as? String == "archive:\(gameId)::\(source.revision)", "the draft key is the web key")
        expect(onDisk["ownerScope"] is NSNull, "the owner is written as null")

        // A new session for the same game restores the line; at an explicit position it comes back parked.
        let reopened = session(store)
        reopened.restoreDraft()
        expect(reopened.cursor == 3 && reopened.parked?.movesUci == [alternative], "a reopened game restores its draft")
        let linked = session(store)
        linked.selectPly(2)
        linked.restoreDraft(keepPosition: true)
        expect(linked.cursor == 2 && linked.parked != nil && !linked.isInVariation, "an explicit position stays")

        // The kept record as it stands now, after the navigation above rewrote it.
        guard case .found(let current) = store.load(source.draftId) else { fatalError("Replay proof: draft missing") }

        // Another release of the game keeps its own draft: it finds none, and saving there leaves this one untouched.
        let otherSource = ReplaySource(kind: "archive", id: gameId, revision: "other@sha256:0")
        let otherRelease = session(store, source: otherSource)
        otherRelease.restoreDraft()
        expect(otherRelease.draftNotice == nil && otherRelease.parked == nil, "another release starts with no draft")
        otherRelease.selectPly(1)
        _ = otherRelease.tryMove(alternative)
        expect(store.load(otherSource.draftId) != .none, "another release's variation is kept under its own key")
        guard case .found(let untouched) = store.load(source.draftId), untouched == current else {
            fatalError("Replay proof: another release must not overwrite this release's draft")
        }

        // A draft whose recorded release differs from the release its key names is refused and left as stored.
        var mislabelled = current
        mislabelled.sourceRevision = "other@sha256:0"
        _ = store.save(mislabelled)
        let mismatch = session(store)
        mismatch.restoreDraft()
        expect(mismatch.draftNotice != nil && mismatch.parked == nil, "a draft from another release is refused")
        expect(store.load(source.draftId) == .found(mislabelled), "a refused draft stays as stored")
        _ = store.save(current)

        // A draft that names another game is refused, though its moves and release match.
        var elsewhere = current
        elsewhere.sourceId = "another-game"
        _ = store.save(elsewhere)
        let misnamed = session(store)
        misnamed.restoreDraft()
        expect(misnamed.draftNotice != nil && misnamed.parked == nil, "a draft naming another game is refused")
        _ = store.save(current)

        // A damaged draft is explained, not installed.
        var broken = current
        broken.variationMovesUci = ["B9-C7"]
        _ = store.save(broken)
        let damaged = session(store)
        damaged.restoreDraft()
        expect(damaged.draftNotice != nil && damaged.parked == nil, "a damaged draft is refused")

        // Taking back a variation's only move leaves no line, and no kept copy to come back.
        let single = session(store)
        single.selectPly(1)
        _ = single.tryMove(alternative)
        single.undoVariationMove()
        expect(store.load(source.draftId) == .none && single.saveState == .none, "an emptied variation is removed")

        // Following the recorded move after parking is navigation too, so a relaunch returns to the new ply.
        let follower = session(ReplayDraftStore(directory: scratch.appendingPathComponent("follow")))
        follower.selectPly(1)
        _ = follower.tryMove(alternative)
        follower.returnToRecordedLine()
        expect(follower.tryMove(recorded[1]) == .followed, "the recorded move follows the record")
        let followStore = ReplayDraftStore(directory: scratch.appendingPathComponent("follow"))
        guard case .found(let followed) = followStore.load(source.draftId), followed.cursor == 2 else {
            fatalError("Replay proof: following the record must update the kept cursor")
        }

        // A replacement that never reached the store leaves the line it replaced on disk: taking it back removes
        // nothing, shows no line, and says the earlier one returns on reopening.
        let flaky = FlakyStore(directory: scratch.appendingPathComponent("flaky"))
        let keeper = session(flaky)
        keeper.selectPly(1)
        _ = keeper.tryMove(alternative)
        keeper.returnToRecordedLine()
        guard case .found(let original) = flaky.load(source.draftId) else { fatalError("Replay proof: draft missing") }
        flaky.failSaves = true
        // The same move again is still a new line that the store has not received.
        expect(keeper.tryMove(alternative) == .variation, "a replacement starts")
        if case .unsaved = keeper.saveState {} else { fatalError("Replay proof: a failing store cannot save") }
        flaky.failSaves = false
        keeper.undoVariationMove()
        expect(keeper.variation == nil && keeper.parked == nil, "no line is shown after the take-back")
        guard case .unsaved(let kept) = keeper.saveState, kept.contains("still holds an earlier variation"),
              kept.contains("returns when the game is opened again") else {
            fatalError("Replay proof: the take-back must say the earlier stored line remains")
        }
        expect(flaky.load(source.draftId) == .found(original), "the replaced line stays on disk as stored")
        expect(keeper.willReplaceDraft, "a new variation is warned that it replaces the line kept on disk")

        // With the store confirmed empty, taking back an unsaved first move promises nothing.
        let emptyFlaky = FlakyStore(directory: scratch.appendingPathComponent("empty-flaky"))
        let fresh = session(emptyFlaky)
        fresh.restoreDraft()
        fresh.selectPly(1)
        emptyFlaky.failSaves = true
        _ = fresh.tryMove(alternative)
        fresh.undoVariationMove()
        expect(fresh.saveState == .none && !fresh.willReplaceDraft, "an empty store leaves nothing to report")

        // When the store could not be read, taking back an unsaved first move says the contents are unconfirmed.
        let blindFlaky = FlakyStore(directory: scratch.appendingPathComponent("blind-flaky"))
        blindFlaky.failLoads = true
        blindFlaky.failSaves = true
        let blind = session(blindFlaky)
        blind.restoreDraft()
        blind.selectPly(1)
        _ = blind.tryMove(alternative)
        blind.undoVariationMove()
        guard case .unsaved(let unconfirmed) = blind.saveState, unconfirmed.contains("could not confirm") else {
            fatalError("Replay proof: an unread store must be reported as unconfirmed")
        }

        let replacement = legal.first { $0 != recorded[1] && $0 != alternative }!

        // A restore replaces what the session showed, so the restored identity belongs only to the restored line.
        let reinstall = FlakyStore(directory: scratch.appendingPathComponent("reinstall"))
        let twice = session(reinstall)
        twice.selectPly(1)
        _ = twice.tryMove(alternative)
        twice.returnToRecordedLine()
        reinstall.failSaves = true
        _ = twice.tryMove(replacement)
        twice.restoreDraft()
        expect(twice.variation == nil && twice.parked?.movesUci == [alternative], "restore shows only the stored line")

        // The reverse: an active stored line restores over a parked, unsaved replacement, which goes away.
        let activeStore = FlakyStore(directory: scratch.appendingPathComponent("active-restore"))
        let active = session(activeStore)
        active.selectPly(1)
        _ = active.tryMove(alternative)
        activeStore.failSaves = true
        active.returnToRecordedLine()
        _ = active.tryMove(replacement)
        active.returnToRecordedLine()
        active.restoreDraft()
        expect(active.variation?.movesUci == [alternative] && active.parked == nil,
               "restore shows the stored active line and drops the parked replacement")

        // A parked line that differs from the stored one says the stored one would be replaced.
        let hidden = FlakyStore(directory: scratch.appendingPathComponent("hidden"))
        let hider = session(hidden)
        hider.selectPly(1)
        _ = hider.tryMove(alternative)
        hider.returnToRecordedLine()
        expect(!hider.keepsUnshownDraft, "a saved parked line is the line this Mac keeps")
        hidden.failSaves = true
        _ = hider.tryMove(replacement)
        hider.returnToRecordedLine()
        expect(hider.parked?.movesUci == [replacement] && hider.keepsUnshownDraft,
               "a parked line that never saved leaves the stored line named as at risk")

        // A removal that fails is reported as unconfirmed, not as the variation still being there.
        let blocked = scratch.appendingPathComponent("blocked")
        let blockedStore = FlakyStore(directory: blocked)
        let remover = session(blockedStore)
        remover.selectPly(1)
        _ = remover.tryMove(alternative)
        let draftFile = blocked.appendingPathComponent("\(source.draftId.addingPercentEncoding(withAllowedCharacters: .alphanumerics)!).json")
        try FileManager.default.removeItem(at: draftFile)
        try FileManager.default.createDirectory(at: draftFile, withIntermediateDirectories: true)
        remover.undoVariationMove()
        guard case .unsaved(let unconfirmedRemoval) = remover.saveState,
              unconfirmedRemoval.contains("could not be confirmed") else {
            fatalError("Replay proof: a failed removal must be reported as unconfirmed")
        }

        // A failed Resume changes only the mode: the stored copy is still this line, and taking it back removes it.
        let resumeFlaky = FlakyStore(directory: scratch.appendingPathComponent("resume-flaky"))
        let resumer = session(resumeFlaky)
        resumer.selectPly(1)
        _ = resumer.tryMove(alternative)
        resumer.returnToRecordedLine()
        resumeFlaky.failSaves = true
        resumer.resumeVariation()
        resumeFlaky.failSaves = false
        resumer.undoVariationMove()
        expect(resumeFlaky.load(source.draftId) == .none && resumer.saveState == .none,
               "taking back this line removes its stored copy after a failed Resume")

        // A read that fails for any reason but absence reports the store unavailable, not empty.
        expect(store.load("archive:absent:") == .none, "an absent draft reads as none")
        if case .unavailable = store.load(String(repeating: "x", count: 400)) {} else {
            fatalError("Replay proof: an unreadable draft name must read as unavailable")
        }

        // Removal takes only a draft file: a folder at its place stays, with its contents, and is reported.
        let odd = scratch.appendingPathComponent("odd")
        let name = source.draftId.addingPercentEncoding(withAllowedCharacters: .alphanumerics)!
        let folder = odd.appendingPathComponent("\(name).json", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("keep".utf8).write(to: folder.appendingPathComponent("inside.txt"))
        expect(ReplayDraftStore(directory: odd).delete(source.draftId) != nil, "a folder is not removed as a draft")
        expect(FileManager.default.fileExists(atPath: folder.appendingPathComponent("inside.txt").path),
               "a folder's contents survive")
        let linkFolder = scratch.appendingPathComponent("linked")
        try FileManager.default.createDirectory(at: linkFolder, withIntermediateDirectories: true)
        let target = scratch.appendingPathComponent("target.json")
        try Data("{}".utf8).write(to: target)
        let link = linkFolder.appendingPathComponent("\(name).json")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: target)
        expect(ReplayDraftStore(directory: linkFolder).delete(source.draftId) != nil, "a link is not removed as a draft")
        expect((try? FileManager.default.destinationOfSymbolicLink(atPath: link.path)) != nil, "the link survives")

        // A position the core has adjudicated takes no variation move, even where pieces could still move.
        let fixtures = try JSONSerialization.jsonObject(with: Data(contentsOf: repoRoot
            .appendingPathComponent("data/fixtures/rules/pikafish_examples.json"))) as! [String: Any]
        var checkedTerminal = false
        for ex in fixtures["examples"] as! [[String: Any]] {
            let fen = ex["fen"] as! String
            let moves = ex["moves_uci"] as! [String]
            let ended = ReplaySession(source: ReplaySource(kind: "archive", id: "fixture", revision: "r"), rootFen: fen,
                                      rulesProfile: "program-2023.11-v1", recordedMovesUci: moves, store: nil)
            ended.selectPly(moves.count)
            let summary = try ended.positionSession().summary()
            guard summary.outcome != nil, let move = summary.legalMovesUci.first else { continue }
            expect(ended.tryMove(move) != .variation && ended.variation == nil, "an adjudicated position takes no move")

            // A stored line that continues past the adjudicated position is refused and left as stored.
            let endedSource = ReplaySource(kind: "archive", id: "fixture", revision: "r")
            let endedStore = ReplayDraftStore(directory: scratch.appendingPathComponent("ended"))
            let record: [String: Any] = [
                "schemaVersion": 1, "draftId": endedSource.draftId, "sourceKind": "archive", "sourceId": "fixture",
                "sourceRevision": "r", "ownerScope": NSNull(), "rootFen": fen, "rulesProfile": "program-2023.11-v1",
                "recordedMovesDigest": recordedMovesDigest(rootFen: fen, movesUci: moves),
                "variationOriginPly": moves.count, "variationMovesUci": [move], "cursor": moves.count,
                "activeMode": "variation", "updatedAt": 0,
            ]
            let draft = try JSONDecoder().decode(ReplayDraft.self, from: JSONSerialization.data(withJSONObject: record))
            expect(endedStore.save(draft) == nil, "the fixture draft is stored")
            let reader = ReplaySession(source: endedSource, rootFen: fen, rulesProfile: "program-2023.11-v1",
                                       recordedMovesUci: moves, store: endedStore)
            reader.restoreDraft()
            expect(reader.draftNotice != nil && reader.variation == nil && reader.parked == nil,
                   "a stored line past adjudication is refused")
            expect(endedStore.load(endedSource.draftId) == .found(draft), "a refused draft stays as stored")
            checkedTerminal = true
            break
        }
        expect(checkedTerminal, "the fixtures hold an adjudicated position with moves left")

        // Without a store, nothing is kept.
        let memoryOnly = session(nil)
        memoryOnly.selectPly(1)
        _ = memoryOnly.tryMove(alternative)
        expect(memoryOnly.saveState == .none, "a session without a store claims no save")

        // The digest covers the web app's text ("<root>\n<moves joined by spaces>"), so both clients name a
        // record the same way; the expected value is `printf 'x\na b' | shasum -a 256`.
        expect(recordedMovesDigest(rootFen: "x", movesUci: ["a", "b"])
            == "cb71d0caccd1a90937da4f8490dda50ee8edfb48ff1b02fa8f040a57815a6e4d", "digest text")

        print("SWIFT_REPLAY_SESSION_PROOF_OK: sample archive, variation, return, resume, draft restore, refusals")
    }
}
