import CryptoKit
import Foundation

/// What a replay shows: an archive game at a published release.
public struct ReplaySource: Equatable {
    public let kind: String
    public let id: String
    /// The archive release the record comes from; a draft reapplies only to the same release.
    public let revision: String

    public init(kind: String, id: String, revision: String) {
        self.kind = kind
        self.id = id
        self.revision = revision
    }

    /// The key of this source's single retained draft, spelled as the web app spells it for a draft without an
    /// owner (`<kind>:<id>:`), so one record keeps one key on every client. An archive game's key also names its
    /// release (`archive:<id>::<release>`), so a variation kept for one release is never overwritten from another.
    public var draftId: String { kind == "archive" ? "\(kind):\(id)::\(revision)" : "\(kind):\(id):" }
}

public struct ReplayVariation: Equatable {
    public let originPly: Int
    public let movesUci: [String]
}

public enum DraftSaveState: Equatable {
    case none
    case saved
    case unsaved(reason: String)
}

public enum ReplayMoveResult: Equatable {
    /// The move is the recorded one, so the replay moves along the record.
    case followed
    /// The move starts or extends the variation.
    case variation
    case rejected(message: String)
}

/// One retained variation per source, kept on this device only. Its fields match the web app's draft record.
public struct ReplayDraft: Codable, Equatable {
    public var schemaVersion: Int
    public var draftId: String
    public var sourceKind: String
    public var sourceId: String
    public var sourceRevision: String
    public var ownerScope: String?
    public var rootFen: String
    public var rulesProfile: String
    public var recordedMovesDigest: String
    public var variationOriginPly: Int
    public var variationMovesUci: [String]
    public var cursor: Int
    public var activeMode: String
    public var updatedAt: Int64

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, draftId, sourceKind, sourceId, sourceRevision, ownerScope, rootFen, rulesProfile
        case recordedMovesDigest, variationOriginPly, variationMovesUci, cursor, activeMode, updatedAt
    }

    /// Writes every field the web record has, `ownerScope` included as null, so both clients write one shape.
    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(schemaVersion, forKey: .schemaVersion)
        try c.encode(draftId, forKey: .draftId)
        try c.encode(sourceKind, forKey: .sourceKind)
        try c.encode(sourceId, forKey: .sourceId)
        try c.encode(sourceRevision, forKey: .sourceRevision)
        try c.encode(ownerScope, forKey: .ownerScope)
        try c.encode(rootFen, forKey: .rootFen)
        try c.encode(rulesProfile, forKey: .rulesProfile)
        try c.encode(recordedMovesDigest, forKey: .recordedMovesDigest)
        try c.encode(variationOriginPly, forKey: .variationOriginPly)
        try c.encode(variationMovesUci, forKey: .variationMovesUci)
        try c.encode(cursor, forKey: .cursor)
        try c.encode(activeMode, forKey: .activeMode)
        try c.encode(updatedAt, forKey: .updatedAt)
    }
}

public enum DraftRead: Equatable {
    case found(ReplayDraft)
    case none
    case unavailable(reason: String)
}

/// Drafts as one JSON file per source in a directory on this device. One app session writes a game's draft at a
/// time, as one web tab does; other writers to the same files (a second copy of the app, another process) are
/// outside this model, so the latest write wins. Open, so a test can stand in a store whose writes fail on demand.
open class ReplayDraftStore {
    private let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    /// The store under the user's Application Support folder.
    public static func applicationSupport() -> ReplayDraftStore? {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return nil
        }
        return ReplayDraftStore(directory: base.appendingPathComponent("Jade/replay-drafts", isDirectory: true))
    }

    /// The file that holds a draft: the SHA-256 of its key's exact bytes, so every key is one fixed-length name that
    /// no other key shares, whatever its length or case and on a case-insensitive volume too. The key itself stays
    /// in the record.
    package static func fileName(for draftId: String) -> String {
        SHA256.hash(data: Data(draftId.utf8)).map { String(format: "%02x", $0) }.joined() + ".json"
    }

    private func fileURL(_ draftId: String) -> URL {
        directory.appendingPathComponent(Self.fileName(for: draftId))
    }

    /// Only a missing file counts as no draft; a folder that cannot be read reports why.
    private static func isMissing(_ error: Error) -> Bool {
        guard let cocoa = error as? CocoaError else { return false }
        return cocoa.code == .fileNoSuchFile || cocoa.code == .fileReadNoSuchFile
    }

    open func load(_ draftId: String) -> DraftRead {
        do {
            let data = try Data(contentsOf: fileURL(draftId))
            return .found(try JSONDecoder().decode(ReplayDraft.self, from: data))
        } catch where Self.isMissing(error) {
            return .none
        } catch is DecodingError {
            return .unavailable(reason: "The saved variation cannot be read.")
        } catch {
            return .unavailable(reason: error.localizedDescription)
        }
    }

    /// Removes the source's draft; nil on success or when there was none, otherwise why it remains.
    open func delete(_ draftId: String) -> String? {
        let path = fileURL(draftId).path
        // The entry itself is inspected, links not followed: only a regular file is a draft, and anything else found
        // there is left and reported. `unlink` removes one name and never a folder's contents. Only a missing entry
        // counts as already removed.
        var info = stat()
        if lstat(path, &info) != 0 {
            return errno == ENOENT ? nil : String(cString: strerror(errno))
        }
        guard info.st_mode & S_IFMT == S_IFREG else {
            return "The saved variation's place holds something other than a file."
        }
        if unlink(path) == 0 || errno == ENOENT { return nil }
        return String(cString: strerror(errno))
    }

    /// Replaces the source's draft; nil on success, otherwise why it was not kept.
    open func save(_ draft: ReplayDraft) -> String? {
        do {
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(draft).write(to: fileURL(draft.draftId), options: .atomic)
            return nil
        } catch {
            return error.localizedDescription
        }
    }
}

/// SHA-256 over the root and recorded moves, so a draft only reapplies to the record it was made on.
/// The same text as the web app's digest, so a draft names its record identically on every client.
public func recordedMovesDigest(rootFen: String, movesUci: [String]) -> String {
    let bytes = Data("\(rootFen)\n\(movesUci.joined(separator: " "))".utf8)
    return SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
}

private let uciMove = try! NSRegularExpression(pattern: "^[a-i][0-9][a-i][0-9]$")

private func isUciMove(_ text: String) -> Bool {
    uciMove.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) != nil
}

/// Replay state for one recorded line: a cursor on the record, one optional variation, and the
/// variation set aside by Return to game. The board, move list, and engine all read `activePath`.
public final class ReplaySession {
    public let source: ReplaySource
    public let rootFen: String
    public let rulesProfile: String
    /// The immutable recorded line; the session never edits it.
    public let recordedMovesUci: [String]

    public private(set) var cursor = 0
    public private(set) var variation: ReplayVariation?
    /// The variation set aside by Return to game, which Resume brings back.
    public private(set) var parked: ReplayVariation?
    public private(set) var saveState: DraftSaveState = .none
    /// Why a stored draft could not be applied to this record; the stored draft stays untouched.
    public private(set) var draftNotice: String?
    public private(set) var feedback: String?

    private let store: ReplayDraftStore?
    private let now: () -> Date

    /// What this Mac's store holds for the game, as far as this session knows. A failed write is atomic and
    /// leaves the previous file in place, so it changes nothing here; reads and removals that fail make it unknown.
    private enum StoredContents: Equatable {
        case unknown
        case empty
        /// A line this session made or restored, by its identity.
        case line(UUID)
        /// A draft this session would not apply to the record.
        case refused
    }
    private var stored: StoredContents = .unknown
    /// The identity of the line shown or parked: new when a variation starts, kept through every edit, Return, and
    /// Resume, so a stored copy is recognized as this line by where it came from, not by matching moves.
    private var lineId: UUID?

    /// Without a store, the session keeps its variation in memory only.
    public init(
        source: ReplaySource,
        rootFen: String,
        rulesProfile: String,
        recordedMovesUci: [String],
        store: ReplayDraftStore?,
        now: @escaping () -> Date = Date.init
    ) {
        self.source = source
        self.rootFen = rootFen
        self.rulesProfile = rulesProfile
        self.recordedMovesUci = recordedMovesUci
        self.store = store
        self.now = now
    }

    public var isInVariation: Bool { variation != nil }

    /// True when a new variation would replace the one retained for this game: parked here, or kept only on disk.
    public var willReplaceDraft: Bool {
        variation == nil && (parked != nil || keepsUnshownDraft)
    }

    /// True when this Mac's store holds a line other than the one shown or parked, which a new save would replace.
    public var keepsUnshownDraft: Bool {
        if case .line(let id) = stored, id != lineId { return true }
        return false
    }

    public var activePath: [String] {
        if let variation {
            return Array(recordedMovesUci.prefix(variation.originPly)) + variation.movesUci
        }
        return Array(recordedMovesUci.prefix(cursor))
    }

    /// A rules session at the active position, for the board and for checking moves.
    public func positionSession() throws -> FfiGameSession {
        let game = try FfiGameSession.fromFen(fen: rootFen, rulesProfile: rulesProfile)
        for uci in activePath {
            _ = try game.applyMoveToken(token: uci)
        }
        return game
    }

    public func selectPly(_ ply: Int) {
        feedback = nil
        parkVariation()
        cursor = max(0, min(recordedMovesUci.count, ply))
        // A kept line's draft also records the position shown, so a relaunch returns to it.
        if parked != nil { persist() }
    }

    /// Plays a UCI move at the active position: the recorded move moves along the record; any other legal
    /// move starts or extends the variation.
    @discardableResult
    public func tryMove(_ uci: String) -> ReplayMoveResult {
        feedback = nil
        let summary: FfiStateSummary
        do {
            summary = try positionSession().summary()
        } catch {
            return reject(error.localizedDescription)
        }
        // The core is the referee: a position it has adjudicated takes no further move, even where the board
        // still has geometrically legal ones.
        if summary.outcome != nil {
            return reject("The game is over at this position under \(rulesProfile).")
        }
        guard summary.legalMovesUci.contains(uci) else {
            return reject("\(uci) is not a legal move here.")
        }
        if variation == nil, cursor < recordedMovesUci.count, recordedMovesUci[cursor] == uci {
            // Following the record is navigation, so a kept line's draft records the new position as Next does.
            selectPly(cursor + 1)
            return .followed
        }
        if let current = variation {
            variation = ReplayVariation(originPly: current.originPly, movesUci: current.movesUci + [uci])
        } else {
            // A new variation replaces the retained one.
            variation = ReplayVariation(originPly: cursor, movesUci: [uci])
            parked = nil
            lineId = UUID()
        }
        persist()
        return .variation
    }

    /// Takes back the variation's last move; taking back its only move leaves the variation.
    public func undoVariationMove() {
        guard let current = variation else { return }
        if current.movesUci.count > 1 {
            variation = ReplayVariation(originPly: current.originPly, movesUci: Array(current.movesUci.dropLast()))
            persist()
        } else {
            cursor = current.originPly
            variation = nil
            let taken = lineId
            lineId = nil
            guard store != nil else { return }
            // Taking back the only move leaves no line. What happens on disk follows from what the store holds.
            switch stored {
            case .line(let id) where id == taken:
                removeStoredDraft()
            case .line:
                saveState = .unsaved(reason: "This Mac still holds an earlier variation for this game; "
                    + "it returns when the game is opened again.")
            case .empty, .refused:
                saveState = .none
            case .unknown:
                saveState = .unsaved(reason: "This Mac could not confirm what it holds for this game.")
            }
        }
    }

    /// Leaves the variation and returns to its origin on the recorded line, keeping it to resume.
    public func returnToRecordedLine() {
        if parkVariation() { persist() }
    }

    public func resumeVariation() {
        guard let line = parked else { return }
        variation = line
        parked = nil
        persist()
    }

    /// Reapplies this game's stored variation when it was made on the same record, release, and rules.
    /// With `keepPosition`, the variation comes back resumable and the cursor stays where it is.
    public func restoreDraft(keepPosition: Bool = false) {
        guard let store else { return }
        draftNotice = nil
        switch store.load(source.draftId) {
        case .none:
            stored = .empty
            return
        case .unavailable(let reason):
            stored = .unknown
            saveState = .unsaved(reason: reason)
        case .found(let draft):
            if let problem = draftProblem(draft) {
                stored = .refused
                draftNotice = problem
                return
            }
            let line = ReplayVariation(originPly: draft.variationOriginPly, movesUci: draft.variationMovesUci)
            // The restored line replaces whatever this session showed, so only the line that receives the new
            // identity remains.
            variation = nil
            parked = nil
            if !keepPosition { cursor = draft.cursor }
            if draft.activeMode == "variation" && !keepPosition {
                variation = line
            } else {
                parked = line
            }
            let id = UUID()
            lineId = id
            stored = .line(id)
            saveState = .saved
        }
    }

    /// The current line as a draft record, or nil when there is no line to keep.
    public func currentDraft() -> ReplayDraft? {
        guard let line = variation ?? parked else { return nil }
        return ReplayDraft(
            schemaVersion: 1,
            draftId: source.draftId,
            sourceKind: source.kind,
            sourceId: source.id,
            sourceRevision: source.revision,
            ownerScope: nil,
            rootFen: rootFen,
            rulesProfile: rulesProfile,
            recordedMovesDigest: recordedMovesDigest(rootFen: rootFen, movesUci: recordedMovesUci),
            variationOriginPly: line.originPly,
            variationMovesUci: line.movesUci,
            cursor: cursor,
            activeMode: variation != nil ? "variation" : "recorded",
            updatedAt: Int64(now().timeIntervalSince1970 * 1000)
        )
    }

    private func reject(_ message: String) -> ReplayMoveResult {
        feedback = message
        return .rejected(message: message)
    }

    private func removeStoredDraft() {
        guard let store else { return }
        if let reason = store.delete(source.draftId) {
            stored = .unknown
            saveState = .unsaved(reason: "Removing the variation from this Mac could not be confirmed: \(reason)")
        } else {
            stored = .empty
            saveState = .none
        }
    }

    /// Sets the active variation aside at its origin on the recorded line; false when none is active.
    @discardableResult
    private func parkVariation() -> Bool {
        guard let current = variation else { return false }
        parked = current
        cursor = current.originPly
        variation = nil
        return true
    }

    private func persist() {
        guard let store else { return }
        guard let draft = currentDraft() else {
            saveState = .none
            return
        }
        if let reason = store.save(draft) {
            saveState = .unsaved(reason: reason)
        } else {
            if let lineId { stored = .line(lineId) }
            saveState = .saved
        }
    }

    /// Why a stored draft cannot be installed on this record, or nil when it can.
    private func draftProblem(_ draft: ReplayDraft) -> String? {
        if draft.schemaVersion != 1 { return "The saved variation uses a format this version does not read." }
        // The file name is only where the record was found; the record must itself name this game.
        if draft.draftId != source.draftId || draft.sourceKind != source.kind || draft.sourceId != source.id
            || draft.ownerScope != nil {
            return "The saved variation belongs to another game."
        }
        if draft.sourceRevision != source.revision {
            return "The saved variation was made on release \(draft.sourceRevision); this record is \(source.revision)."
        }
        if draft.rulesProfile != rulesProfile {
            return "The saved variation follows \(draft.rulesProfile), not \(rulesProfile)."
        }
        if draft.rootFen != rootFen
            || draft.recordedMovesDigest != recordedMovesDigest(rootFen: rootFen, movesUci: recordedMovesUci) {
            return "The saved variation was made on a different recorded line."
        }
        let origin = draft.variationOriginPly
        let moves = draft.variationMovesUci
        guard (0...recordedMovesUci.count).contains(origin),
              (0...recordedMovesUci.count).contains(draft.cursor),
              draft.activeMode == "variation" || draft.activeMode == "recorded",
              !moves.isEmpty,
              moves.allSatisfy(isUciMove) else {
            return "The saved variation is incomplete."
        }
        do {
            let game = try FfiGameSession.fromFen(fen: rootFen, rulesProfile: rulesProfile)
            for uci in recordedMovesUci.prefix(origin) {
                _ = try game.applyMoveToken(token: uci)
            }
            for uci in moves {
                let summary = try game.summary()
                guard summary.outcome == nil, summary.legalMovesUci.contains(uci) else {
                    return "The saved variation does not replay under the rules."
                }
                _ = try game.applyMoveToken(token: uci)
            }
        } catch {
            return "The saved variation does not replay under the rules."
        }
        return nil
    }
}
