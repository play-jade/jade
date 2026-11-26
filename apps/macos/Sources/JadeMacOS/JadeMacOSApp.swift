import AppKit
import AVFoundation
import CoreGraphics
import Foundation
import ImageIO
import JadeCoreBindings
import Security
import SQLite3
import SwiftUI
import UniformTypeIdentifiers

func resolveJadeRepoRoot() -> String {
    if let env = ProcessInfo.processInfo.environment["JADE_REPO_ROOT"], !env.isEmpty {
        return env
    }
    if let res = Bundle.main.resourcePath {
        let candidate = (res as NSString).appendingPathComponent("assets/board/materials")
        if FileManager.default.fileExists(atPath: candidate) {
            return res
        }
    }
    return FileManager.default.currentDirectoryPath
}

/// Native macOS client built with SwiftUI + AppKit (`NSView` Core Graphics orthographic 2.5D board),
/// VoiceOver `NSAccessibilityElement` children, `AVAudioEngine`/`AVAudioPlayerNode` scheduled piece audio,
/// C++26 `jade-uci-engine` local opponent integration, Apple Keychain credential storage, and SQLite3 offline persistence.
@main
struct JadeMacOSLauncher {
    static func main() throws {
        let args = CommandLine.arguments
        if let idx = args.firstIndex(of: "--render-proof"), idx + 1 < args.count {
            let outPath = args[idx + 1]
            let repoRoot = args.firstIndex(of: "--repo-root").flatMap { i in
                i + 1 < args.count ? args[i + 1] : nil
            } ?? resolveJadeRepoRoot()
            try HeadlessCoreGraphicsProof.render(repoRoot: repoRoot, outputPath: outPath)
            print("MACOS_COREGRAPHICS_PROOF_OK: \(outPath)")
            return
        }
        NSApplication.shared.setActivationPolicy(.regular)
        DispatchQueue.main.async {
            NSApplication.shared.activate(ignoringOtherApps: true)
        }
        JadeMacOSGuiApp.main()
    }
}

struct JadeMacOSGuiApp: App {
    @StateObject private var controller = MacBoardController()

    var body: some Scene {
        WindowGroup("Jade") {
            LamplitTableView(controller: controller)
                .frame(minWidth: 960, minHeight: 680)
        }
        .windowStyle(.hiddenTitleBar)
        .defaultSize(width: 1440, height: 900)
        .commands {
            CommandGroup(after: .toolbar) {
                Toggle("Sound", isOn: $controller.soundEnabled)
                    .keyboardShortcut("m", modifiers: [.command, .shift])
            }
            CommandMenu("Board") {
                Button("New Offline Game") { controller.resetOfflineGame() }
                    .keyboardShortcut("n", modifiers: [.command])
                Button("Flip Orientation") { controller.redBelow.toggle() }
                    .keyboardShortcut("f", modifiers: [.command])
                Button("Undo Move") { controller.undo() }
                    .keyboardShortcut("z", modifiers: [.command])
                Button("Play Engine Move") { controller.requestEngineMove() }
                    .keyboardShortcut("e", modifiers: [.command])
                Divider()
                Button("Open PGN Record…") { controller.importPgnInteractive() }
                    .keyboardShortcut("o", modifiers: [.command])
                Button("Export Game PGN…") { controller.exportPgnInteractive() }
                    .keyboardShortcut("s", modifiers: [.command])
                    // The export describes the offline game; a replay is the archive's record, not this Mac's game.
                    .disabled(controller.replay != nil)
            }
            CommandMenu("Replay") {
                Button("Open Sample Game…") { controller.showSamplePicker = true }
                    .keyboardShortcut("o", modifiers: [.command, .shift])
                Divider()
                // Stepping moves along the recorded line; within a variation, Take Back is the only edit.
                Button("Previous Move") { controller.replayStep(-1) }
                    .keyboardShortcut(.leftArrow, modifiers: [.command])
                    .disabled(controller.replay == nil || controller.replay?.isInVariation == true)
                Button("Next Move") { controller.replayStep(1) }
                    .keyboardShortcut(.rightArrow, modifiers: [.command])
                    .disabled(controller.replay == nil || controller.replay?.isInVariation == true)
                Button("Return to Game") { controller.replayReturnToGame() }
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                    .disabled(controller.replay?.isInVariation != true)
                Button("Resume Variation") { controller.replayResume() }
                    .disabled(controller.replay?.parked == nil)
                Divider()
                Button("Close Replay") { controller.closeReplay() }
                    .keyboardShortcut("w", modifiers: [.command, .shift])
                    .disabled(controller.replay == nil)
            }
        }

        Settings {
            JadeSettingsView(controller: controller)
        }
    }
}

/// Lamplit-table visual tokens: the board is a lit object on a dark ground, and the window
/// chrome recedes. One place for these values so piece-set, board and palette theming can
/// replace them later.
enum LamplitTheme {
    static let ground = Color(red: 0x1D / 255.0, green: 0x24 / 255.0, blue: 0x21 / 255.0)
    static let paper = Color(red: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0)
    static let paleJade = Color(red: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0)
    /// Vermilion lightened for small text on the dark ground; the web uses the same `#E69383`.
    static let vermilionOnDark = Color(red: 0xE6 / 255.0, green: 0x93 / 255.0, blue: 0x83 / 255.0)
    static let mutedText = Color(red: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0).opacity(0.62)
    static let hairline = Color(red: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0).opacity(0.12)

    static func display(_ size: CGFloat) -> Font {
        NSFont(name: "NotoSerifDisplay-Regular", size: size) != nil
            ? .custom("NotoSerifDisplay-Regular", size: size)
            : .system(size: size, weight: .regular, design: .serif)
    }

    /// Registers the brand display face from the repository's brand sources for this process only.
    static func registerFonts(repoRoot: String) {
        let url = URL(fileURLWithPath: repoRoot)
            .appendingPathComponent("assets/brand/source/fonts/NotoSerifDisplay-Regular.ttf")
        if FileManager.default.fileExists(atPath: url.path) {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }
}

struct LamplitTableView: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        ZStack {
            LamplitGround(repoRoot: controller.repoRoot)
            HStack(spacing: 0) {
                PlayersColumn(controller: controller)
                    .frame(width: 260)
                MacCoreGraphicsBoardRepresentable(controller: controller)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if controller.replay != nil {
                    ReplayColumn(controller: controller)
                        .frame(width: 300)
                } else {
                    MovesColumn(controller: controller)
                        .frame(width: 280)
                }
            }
            VStack {
                Spacer()
                ControlCapsule(controller: controller)
                    .padding(.bottom, 22)
            }
        }
        .ignoresSafeArea()
        .environment(\.colorScheme, .dark)
        .foregroundStyle(LamplitTheme.paper)
        .sheet(isPresented: $controller.showSamplePicker) {
            SampleGamePicker(controller: controller)
        }
    }
}

/// Dark ground with a warm pool of key light from the upper left (the studio light of
/// docs/rendering.md §5) and a soft vignette, so the board reads as an object on a table.
struct LamplitGround: View {
    let repoRoot: String

    var body: some View {
        GeometryReader { geo in
            ZStack {
                LamplitTheme.ground
                if let grain = NSImage(contentsOfFile: (repoRoot as NSString)
                    .appendingPathComponent("assets/board/materials/dark-grain.png")) {
                    Image(nsImage: grain)
                        .resizable(resizingMode: .tile)
                        .opacity(0.9)
                }
                RadialGradient(
                    colors: [LamplitTheme.paper.opacity(0.09), LamplitTheme.paper.opacity(0)],
                    center: UnitPoint(x: 0.40, y: 0.34),
                    startRadius: 0,
                    endRadius: max(geo.size.width, geo.size.height) * 0.55
                )
                RadialGradient(
                    colors: [Color.black.opacity(0), Color.black.opacity(0.55)],
                    center: .center,
                    startRadius: min(geo.size.width, geo.size.height) * 0.45,
                    endRadius: max(geo.size.width, geo.size.height) * 0.85
                )
            }
        }
        .allowsHitTesting(false)
    }
}

struct PlayersColumn: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        let top = controller.redBelow ? "black" : "red"
        let bottom = controller.redBelow ? "red" : "black"
        VStack(alignment: .leading) {
            SidePlate(controller: controller, side: top)
            Spacer()
            SidePlate(controller: controller, side: bottom)
        }
        .padding(.leading, 56)
        .padding(.vertical, 64)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct SidePlate: View {
    @ObservedObject var controller: MacBoardController
    let side: String

    private var toMove: Bool { controller.sideToMove == side && controller.outcomeText == nil }
    private var isEngine: Bool { side == "black" && controller.autoEngineBlack && controller.replay == nil }
    private var title: String { side == "red" ? "Red" : "Black" }
    private var caption: String {
        let who = controller.replayPlayers.map { side == "red" ? $0.red : $0.black }
            ?? (isEngine ? "Engine" : "This Mac")
        return (toMove ? "\(who) · to move" : who).uppercased()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            captionRow
            Text(title)
                .font(LamplitTheme.display(56))
                .foregroundStyle(toMove ? LamplitTheme.paper : LamplitTheme.mutedText)
            if isEngine && controller.engineThinking {
                Text("Thinking…")
                    .font(.system(size: 13))
                    .foregroundStyle(LamplitTheme.mutedText)
            }
            capturedRow
        }
        .animation(.easeOut(duration: 0.18), value: toMove)
    }

    private var captionRow: some View {
        let dot: Color = toMove ? LamplitTheme.paleJade : LamplitTheme.hairline
        let glow: Color = toMove ? LamplitTheme.paleJade.opacity(0.6) : .clear
        let ink: Color = toMove ? LamplitTheme.paleJade : LamplitTheme.mutedText
        return HStack(spacing: 10) {
            Circle()
                .fill(dot)
                .frame(width: 8, height: 8)
                .shadow(color: glow, radius: 6)
            Text(caption)
                .font(.system(size: 11, weight: .medium))
                .tracking(1.8)
                .foregroundStyle(ink)
        }
    }

    @ViewBuilder
    private var capturedRow: some View {
        let taken = controller.capturedGlyphs(ownedBy: side)
        if !taken.isEmpty {
            Text(taken.joined(separator: " "))
                .font(.system(size: 17, weight: .semibold, design: .serif))
                .foregroundStyle(LamplitTheme.mutedText)
                .accessibilityLabel("\(title) has lost \(taken.count) pieces")
        }
    }
}

/// One move number's Red and Black plies, and which of them is the latest.
struct MoveRow: Identifiable {
    let number: Int
    let red: String
    let black: String
    let redIsLatest: Bool
    let blackIsLatest: Bool
    var id: Int { number }
}

struct MovesColumn: View {
    @ObservedObject var controller: MacBoardController

    /// Built outside `body` and typed explicitly, so the view body stays small enough for every Swift type checker.
    private var rows: [MoveRow] {
        let moves = controller.movesNotated
        let lastPly = moves.count - 1
        var out: [MoveRow] = []
        var i = 0
        while i < moves.count {
            let black = i + 1 < moves.count ? moves[i + 1] : ""
            out.append(MoveRow(number: i / 2 + 1, red: moves[i], black: black,
                               redIsLatest: i == lastPly, blackIsLatest: i + 1 == lastPly))
            i += 2
        }
        return out
    }

    private var footerText: String {
        if let outcome = controller.outcomeText { return outcome }
        return controller.inCheck ? "Check" : "Offline · Untimed"
    }

    private var footerColor: Color {
        let alert = controller.inCheck || controller.outcomeText != nil
        return alert ? LamplitTheme.vermilionOnDark : LamplitTheme.mutedText
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Spacer(minLength: 64)
            movesList
            Rectangle().fill(LamplitTheme.hairline).frame(height: 1).padding(.vertical, 14)
            Text(footerText)
                .font(.system(size: 12))
                .foregroundStyle(footerColor)
        }
        .padding(.trailing, 56)
        .padding(.bottom, 96)
    }

    private var movesList: some View {
        let rows = self.rows
        return ScrollViewReader { proxy in
            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(rows) { row in
                        MoveRowView(row: row)
                    }
                }
            }
            .defaultScrollAnchor(.bottom)
            .onChange(of: controller.movesNotated.count) {
                if let last = rows.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
        }
    }
}

struct MoveRowView: View {
    let row: MoveRow

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 0) {
            Text("\(row.number)")
                .font(.system(size: 12).monospacedDigit())
                .foregroundStyle(LamplitTheme.mutedText)
                .frame(width: 32, alignment: .leading)
            MoveText(text: row.red, current: row.redIsLatest)
            MoveText(text: row.black, current: row.blackIsLatest)
        }
        .padding(.vertical, 6)
        .id(row.id)
    }
}

struct MoveText: View {
    let text: String
    let current: Bool

    var body: some View {
        let weight: Font.Weight = current ? .semibold : .regular
        let color: Color = current ? LamplitTheme.paper : LamplitTheme.paper.opacity(0.78)
        Text(text)
            .font(.system(size: 15, weight: weight).monospacedDigit())
            .foregroundStyle(color)
            .underline(current, color: LamplitTheme.paleJade.opacity(0.6))
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Floating glass capsule: the only on-canvas controls. Everything else lives in menus and Settings.
struct ControlCapsule: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        HStack(spacing: 4) {
            capsuleButton("arrow.up.arrow.down", "Flip board (⌘F)") { controller.redBelow.toggle() }
            if let replay = controller.replay {
                capsuleButton("chevron.left", "Previous move (⌘←)") { controller.replayStep(-1) }
                    .disabled(replay.isInVariation || replay.cursor == 0)
                capsuleButton("chevron.right", "Next move (⌘→)") { controller.replayStep(1) }
                    .disabled(replay.isInVariation || replay.cursor == replay.recordedMovesUci.count)
                capsuleButton("arrow.uturn.backward", "Take back variation move (⌘Z)") { controller.undo() }
                    .disabled(!replay.isInVariation || controller.engineThinking)
                capsuleButton("cpu", "Try engine suggestion (⌘E)") { controller.requestEngineMove() }
                    .disabled(controller.engineThinking || controller.outcomeText != nil)
                capsuleButton("xmark", "Close replay (⇧⌘W)") { controller.closeReplay() }
            } else {
                capsuleButton("arrow.uturn.backward", "Take back (⌘Z)") { controller.undo() }
                    .disabled(controller.movesUci.isEmpty || controller.engineThinking)
                capsuleButton("cpu", "Engine move (⌘E)") { controller.requestEngineMove() }
                    .disabled(controller.engineThinking || controller.outcomeText != nil)
            }
            capsuleButton("arrow.counterclockwise", "New game (⌘N)") { controller.resetOfflineGame() }
            capsuleButton(controller.soundEnabled ? "speaker.wave.2" : "speaker.slash",
                          controller.soundEnabled ? "Sound on" : "Sound off") {
                controller.soundEnabled.toggle()
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(.ultraThinMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(LamplitTheme.hairline))
        .shadow(color: .black.opacity(0.4), radius: 18, y: 10)
    }

    private func capsuleButton(_ symbol: String, _ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .regular))
                .frame(width: 44, height: 44)
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(LamplitTheme.paper.opacity(0.82))
        .help(label)
        .accessibilityLabel(label)
    }
}

struct JadeSettingsView: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        Form {
            Picker("Piece set", selection: $controller.materialId) {
                Text("Pale Jade").tag("pale-jade")
                Text("Honey Boxwood").tag("boxwood")
                Text("Ivory Stone").tag("ivory-stone")
            }
            Toggle("Traditional faces (繁體)", isOn: $controller.traditionalGlyphs)
            Toggle("Dark board surface", isOn: $controller.darkSurface)
            Toggle("Engine plays Black", isOn: $controller.autoEngineBlack)
            Toggle("Piece sounds", isOn: $controller.soundEnabled)
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .padding(.vertical, 8)
    }
}

final class MacBoardController: ObservableObject {
    @Published var redBelow: Bool = true
    @Published var materialId: String = "pale-jade"
    @Published var traditionalGlyphs: Bool = false
    @Published var darkSurface: Bool = false
    @Published var soundEnabled: Bool = true
    @Published var autoEngineBlack: Bool = true
    @Published var engineThinking: Bool = false
    @Published var selectedUci: String? = nil
    @Published var legalDestUcis: Set<String> = []
    @Published var focusedSquare: (Int, Int) = (4, 0)
    @Published var movesUci: [String] = []
    @Published var movesChinese: [String] = []
    @Published var statusLine: String = "Red to move · program-2023.11-v1"
    /// Moves in the reader's notation: Chinese when the first preferred language is Chinese, WXF otherwise.
    @Published var movesNotated: [String] = []
    @Published var sideToMove: String = "red"
    @Published var inCheck: Bool = false
    @Published var outcomeText: String? = nil
    @Published private(set) var pieceCounts: [String: Int] = [:]
    /// The archive game being replayed, or nil while playing the offline game.
    @Published private(set) var replay: ReplaySession? = nil
    @Published private(set) var replayInfo: SampleGameInfo? = nil
    @Published private(set) var replayRecordedNotated: [String] = []
    @Published private(set) var replayVariationNotated: [String] = []
    /// Bumped after each replay change, so views that read the session redraw.
    @Published private(set) var replayRevision = 0
    @Published var showSamplePicker = false
    var replayPlayers: (red: String, black: String)? {
        replayInfo.map { ($0.redName, $0.blackName) }
    }
    private let replayStore = ReplayDraftStore.applicationSupport()
    private static let lastReplayGameKey = "jade.replay.lastGameId"
    private static let lastReplayPlyKey = "jade.replay.lastPly"
    private static let startFen = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1"

    private let notateWxf = !(Locale.preferredLanguages.first ?? "en").hasPrefix("zh")
    private static let startingCounts: [String: Int] = [
        "general": 1, "advisor": 2, "elephant": 2, "horse": 2, "chariot": 2, "cannon": 2, "soldier": 5,
    ]
    private static let roleOrder = ["chariot", "horse", "cannon", "elephant", "advisor", "soldier", "general"]

    let repoRoot: String
    private(set) var session: FfiGameSession
    private let audio: MacPieceAudioEngine
    private let offlineStore = AppleOfflineSqliteStore(
        directory: NSTemporaryDirectory(),
        filename: "jade_macos_offline.sqlite3"
    )

    init() {
        let root = resolveJadeRepoRoot()
        self.repoRoot = root
        LamplitTheme.registerFonts(repoRoot: root)
        self.audio = MacPieceAudioEngine(repoRoot: root)
        self.session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        loadOfflineGameFromStore()
        // A replay open when the app quit comes back at the same game and position.
        if let gameId = UserDefaults.standard.string(forKey: Self.lastReplayGameKey) {
            openSampleGame(id: gameId, ply: UserDefaults.standard.integer(forKey: Self.lastReplayPlyKey))
        }
    }

    private func loadOfflineGameFromStore() {
        session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        movesUci.removeAll()
        movesChinese.removeAll()
        for uci in offlineStore.loadMoves() {
            if let zh = try? session.applyMoveToken(token: uci) {
                movesUci.append(uci)
                movesChinese.append(zh)
            }
        }
        refreshStatus()
    }

    // MARK: Replay of a sample archive game

    /// The embedded sample collection's games; empty when the core cannot read it.
    func sampleGames() -> [SampleGameInfo] {
        guard let json = try? ffiSampleArchiveGamesJson(offset: 0, limit: 50),
              let page = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              let release = page["release"] as? [String: Any],
              let games = page["games"] as? [[String: Any]] else { return [] }
        return games.compactMap { SampleGameInfo(summary: $0, release: release) }
    }

    /// Opens a sample game: the recorded line, then this Mac's saved variation for it, then `ply` when
    /// no saved variation says where the visitor was.
    func openSampleGame(id: String, ply: Int? = nil) {
        guard let json = try? ffiSampleArchiveGameDetailJson(gameId: id),
              let detail = try? JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any],
              let release = detail["release"] as? [String: Any],
              let summary = detail["summary"] as? [String: Any],
              let record = detail["record"] as? [String: Any],
              let info = SampleGameInfo(summary: summary, release: release),
              let rootFen = record["initial_fen"] as? String,
              let moves = record["moves_uci"] as? [String] else {
            statusLine = "The sample game \(id) could not be opened"
            UserDefaults.standard.removeObject(forKey: Self.lastReplayGameKey)
            return
        }
        let next = ReplaySession(
            source: ReplaySource(kind: "archive", id: id, revision: info.releaseId),
            rootFen: rootFen,
            rulesProfile: "program-2023.11-v1",
            recordedMovesUci: moves,
            store: replayStore
        )
        next.selectPly(ply ?? moves.count)
        next.restoreDraft()
        replay = next
        replayInfo = info.withRecord(record)
        replayRecordedNotated = notate(moves, from: rootFen)
        selectedUci = nil
        legalDestUcis.removeAll()
        UserDefaults.standard.set(id, forKey: Self.lastReplayGameKey)
        syncReplay()
    }

    func closeReplay() {
        guard replay != nil else { return }
        replay = nil
        replayInfo = nil
        replayRecordedNotated = []
        replayVariationNotated = []
        selectedUci = nil
        legalDestUcis.removeAll()
        UserDefaults.standard.removeObject(forKey: Self.lastReplayGameKey)
        UserDefaults.standard.removeObject(forKey: Self.lastReplayPlyKey)
        loadOfflineGameFromStore()
    }

    func replaySelectPly(_ ply: Int) {
        guard let replay, !engineThinking else { return }
        replay.selectPly(ply)
        syncReplay()
    }

    /// One ply along the recorded line, or along the variation while one is shown.
    func replayStep(_ delta: Int) {
        guard let replay, !engineThinking, !replay.isInVariation else { return }
        replay.selectPly(replay.cursor + delta)
        syncReplay()
    }

    func replayReturnToGame() {
        guard let replay else { return }
        replay.returnToRecordedLine()
        syncReplay()
    }

    func replayResume() {
        guard let replay else { return }
        replay.resumeVariation()
        syncReplay()
    }

    /// Shows the replay's active position on the board and its lines in the move list.
    private func syncReplay() {
        guard let replay else { return }
        // Every replay change can move the position, so no earlier selection outlives it.
        selectedUci = nil
        legalDestUcis.removeAll()
        if let game = try? replay.positionSession() { session = game }
        movesUci = replay.activePath
        movesChinese = []
        if let line = replay.variation ?? replay.parked {
            let prefix = Array(replay.recordedMovesUci.prefix(line.originPly))
            replayVariationNotated = Array(notate(prefix + line.movesUci, from: replay.rootFen).dropFirst(prefix.count))
        } else {
            replayVariationNotated = []
        }
        UserDefaults.standard.set(replay.cursor, forKey: Self.lastReplayPlyKey)
        replayRevision += 1
        refreshStatus()
    }

    /// Notates `moves` from `fen` in the reader's dialect, each before it is applied.
    private func notate(_ moves: [String], from fen: String) -> [String] {
        guard let game = try? FfiGameSession.fromFen(fen: fen, rulesProfile: "program-2023.11-v1") else { return moves }
        return moves.map { uci in
            let text = (try? game.notateMoveToken(token: uci, wxf: notateWxf)) ?? uci
            _ = try? game.applyMoveToken(token: uci)
            return text
        }
    }

    private func requestReplayEngineMove(_ replay: ReplaySession) {
        // The local engine is given the standard start plus moves, so it advises only from that start.
        guard replay.rootFen == Self.startFen else {
            statusLine = "Engine suggestions need a game that starts from the standard position"
            return
        }
        let path = replay.activePath
        let root = repoRoot
        engineThinking = true
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let best = LocalUciProcessRunner.computeBestMove(repoRoot: root, movesUci: path)
            DispatchQueue.main.async {
                guard let self else { return }
                self.engineThinking = false
                // The suggestion applies only to the position it was computed for.
                guard self.replay === replay, replay.activePath == path else { return }
                guard let best else {
                    self.statusLine = "The engine gave no suggestion here"
                    return
                }
                replay.tryMove(best)
                if self.soundEnabled { self.audio.playPlacementOnce(materialId: self.materialId) }
                self.syncReplay()
            }
        }
    }

    func activateSquare(file: Int, rank: Int) {
        if engineThinking { return }
        let uci = "\(Character(UnicodeScalar(97 + file)!))\(rank)"
        if selectedUci == nil {
            selectIfFriendly(uci: uci)
        } else if selectedUci == uci {
            selectedUci = nil
            legalDestUcis.removeAll()
        } else if let from = selectedUci, let replay {
            if case .rejected = replay.tryMove("\(from)\(uci)") {
                selectIfFriendly(uci: uci)
            } else {
                if soundEnabled { audio.playPlacementOnce(materialId: materialId) }
                selectedUci = nil
                legalDestUcis.removeAll()
                syncReplay()
                return
            }
        } else if let from = selectedUci {
            let moveToken = "\(from)\(uci)"
            if let zh = try? session.applyMoveToken(token: moveToken) {
                movesUci.append(moveToken)
                movesChinese.append(zh)
                offlineStore.appendMove(uci: moveToken)
                if soundEnabled {
                    audio.playPlacementOnce(materialId: materialId)
                }
                selectedUci = nil
                legalDestUcis.removeAll()
                refreshStatus()
                maybeTriggerAutoEngine()
                return
            } else {
                selectIfFriendly(uci: uci)
            }
        }
        refreshStatus()
    }

    private func selectIfFriendly(uci: String) {
        guard let sum = try? session.summary() else { return }
        let prefix = uci
        let dests = sum.legalMovesUci
            .filter { $0.hasPrefix(prefix) && $0.count == 4 }
            .map { String($0.dropFirst(2)) }
        if !dests.isEmpty {
            selectedUci = uci
            legalDestUcis = Set(dests)
        } else {
            selectedUci = nil
            legalDestUcis.removeAll()
        }
    }

    private func maybeTriggerAutoEngine() {
        guard autoEngineBlack,
              let sum = try? session.summary(),
              sum.outcome == nil,
              sum.sideToMove == "black" else {
            return
        }
        requestEngineMove()
    }

    func requestEngineMove() {
        if let replay {
            if !engineThinking { requestReplayEngineMove(replay) }
            return
        }
        guard !engineThinking,
              let sum = try? session.summary(),
              sum.outcome == nil else { return }
        let currentMoves = movesUci
        let root = repoRoot
        let origin = session
        engineThinking = true
        statusLine = "C++26 engine searching…"

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let best = LocalUciProcessRunner.computeBestMove(repoRoot: root, movesUci: currentMoves)
            DispatchQueue.main.async {
                guard let self else { return }
                self.engineThinking = false
                // The answer belongs to the offline position it was computed for; after a replay opened or the
                // game changed, it applies nowhere.
                guard self.replay == nil, self.session === origin, self.movesUci == currentMoves else { return }
                if let bestMove = best,
                   let zh = try? self.session.applyMoveToken(token: bestMove) {
                    self.movesUci.append(bestMove)
                    self.movesChinese.append(zh)
                    self.offlineStore.appendMove(uci: bestMove)
                    if self.soundEnabled {
                        self.audio.playPlacementOnce(materialId: self.materialId)
                    }
                }
                self.refreshStatus()
            }
        }
    }

    func resetOfflineGame() {
        closeReplay()
        session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        selectedUci = nil
        legalDestUcis.removeAll()
        movesUci.removeAll()
        movesChinese.removeAll()
        offlineStore.clearMoves()
        refreshStatus()
    }

    func undo() {
        if let replay {
            replay.undoVariationMove()
            syncReplay()
            return
        }
        if (try? session.undo()) != nil, !movesChinese.isEmpty {
            _ = movesUci.popLast()
            _ = movesChinese.popLast()
            offlineStore.popLastMove()
            selectedUci = nil
            legalDestUcis.removeAll()
            refreshStatus()
        }
    }

    func formatAsPgn() -> String {
        var lines = [
            "[Event \"Jade macOS Offline Game\"]",
            "[RulesProfile \"program-2023.11-v1\"]",
            "[Result \"*\"]",
            ""
        ]
        var body: [String] = []
        for (idx, uci) in movesUci.enumerated() {
            let zh = idx < movesChinese.count ? movesChinese[idx] : uci
            if idx % 2 == 0 {
                body.append("\(idx / 2 + 1). \(uci) {\(zh)}")
            } else {
                body.append("\(uci) {\(zh)}")
            }
        }
        body.append("*")
        lines.append(body.joined(separator: " "))
        return lines.joined(separator: "\n") + "\n"
    }

    @discardableResult
    func writePgn(to url: URL) -> Bool {
        do {
            try formatAsPgn().write(to: url, atomically: true, encoding: .utf8)
            statusLine = "Exported \(movesUci.count) plies to \(url.lastPathComponent)"
            return true
        } catch {
            statusLine = "PGN export failed: \(error.localizedDescription)"
            return false
        }
    }

    func exportPgnInteractive() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "jade-game.pgn"
        panel.canCreateDirectories = true
        if panel.runModal() == .OK, let url = panel.url {
            writePgn(to: url)
        }
    }

    func importPgnInteractive() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url {
            do {
                let raw = try String(contentsOf: url, encoding: .utf8)
                loadFromPgnText(raw)
            } catch {
                statusLine = "PGN import failed: \(error.localizedDescription)"
            }
        }
    }

    @discardableResult
    func loadFromPgnText(_ raw: String) -> Bool {
        var stripped = ""
        var inBrace = false
        for line in raw.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("[") || trimmed.hasPrefix(";") || trimmed.isEmpty { continue }
            for ch in trimmed {
                if ch == "{" { inBrace = true }
                else if ch == "}" { inBrace = false }
                else if !inBrace { stripped.append(ch) }
            }
            stripped.append(" ")
        }
        let fresh = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        var importedUci: [String] = []
        var importedZh: [String] = []
        for rawTok in stripped.split(whereSeparator: { $0.isWhitespace }) {
            var candidateTok = String(rawTok)
            if let dotIdx = candidateTok.firstIndex(of: "."),
               candidateTok[..<dotIdx].allSatisfy({ $0.isASCII && $0.isNumber }) {
                let afterDots = candidateTok[dotIdx...].drop(while: { $0 == "." })
                candidateTok = String(afterDots)
            }
            let tok = candidateTok.trimmingCharacters(in: CharacterSet(charactersIn: "."))
            if tok.isEmpty || tok.allSatisfy({ $0.isASCII && $0.isNumber }) { continue }
            if ["1-0", "0-1", "1/2-1/2", "*"].contains(tok) { continue }
            let prevLegal = (try? fresh.summary())?.legalMovesUci ?? []
            if let zh = try? fresh.applyMoveToken(token: tok) {
                let targetHash = (try? fresh.summary())?.positionKeySha256
                if prevLegal.contains(tok) {
                    importedUci.append(tok)
                } else if let targetHash,
                          let matchedUci = prevLegal.first(where: { cand in
                              let probe = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
                              for past in importedUci { _ = try? probe.applyMoveToken(token: past) }
                              _ = try? probe.applyMoveToken(token: cand)
                              return (try? probe.summary())?.positionKeySha256 == targetHash
                          }) {
                    importedUci.append(matchedUci)
                } else {
                    importedUci.append(tok)
                }
                importedZh.append(zh)
            } else {
                statusLine = "PGN import failed at ply \(importedUci.count + 1) (\(tok)); current game preserved"
                return false
            }
        }
        guard !importedUci.isEmpty else {
            statusLine = "PGN import failed: no moves found; current game preserved"
            return false
        }
        closeReplay()
        session = fresh
        movesUci = importedUci
        movesChinese = importedZh
        offlineStore.clearMoves()
        for u in importedUci { offlineStore.appendMove(uci: u) }
        selectedUci = nil
        legalDestUcis.removeAll()
        refreshStatus()
        return true
    }

    private func refreshStatus() {
        if let sum = try? session.summary() {
            sideToMove = sum.sideToMove
            inCheck = sum.inCheck
            if let outcome = sum.outcome {
                let reason = (sum.reason ?? "").replacingOccurrences(of: "_", with: " ")
                outcomeText = "\(outcome.replacingOccurrences(of: "_", with: " ").capitalized) · \(reason)"
                statusLine = "Finished: \(outcome) (\(sum.reason ?? ""))"
            } else {
                outcomeText = nil
                let checkNote = sum.inCheck ? " · CHECK!" : ""
                statusLine = "\(sum.sideToMove.capitalized) to move · \(sum.plyCount) plies\(checkNote)"
            }
        }
        var counts: [String: Int] = [:]
        for p in (try? session.pieces(traditionalGlyphs: traditionalGlyphs)) ?? [] {
            counts["\(p.side)-\(p.role)", default: 0] += 1
        }
        pieceCounts = counts
        rebuildNotation()
    }

    /// Replays the recorded moves on a scratch session, notating each one before it is applied.
    private func rebuildNotation() {
        let start = self.replay.map { $0.rootFen } ?? Self.startFen
        guard let replay = try? FfiGameSession.fromFen(fen: start, rulesProfile: "program-2023.11-v1") else { return }
        var out: [String] = []
        for (idx, uci) in movesUci.enumerated() {
            let fallback = idx < movesChinese.count ? movesChinese[idx] : uci
            out.append((try? replay.notateMoveToken(token: uci, wxf: notateWxf)) ?? fallback)
            _ = try? replay.applyMoveToken(token: uci)
        }
        movesNotated = out
    }

    /// Faces of the pieces `side` has lost, most valuable first.
    func capturedGlyphs(ownedBy side: String) -> [String] {
        let faces: [String: (red: String, black: String)] = traditionalGlyphs
            ? ["general": ("帥", "將"), "advisor": ("仕", "士"), "elephant": ("相", "象"), "horse": ("馬", "馬"),
               "chariot": ("車", "車"), "cannon": ("炮", "砲"), "soldier": ("兵", "卒")]
            : ["general": ("帅", "将"), "advisor": ("仕", "士"), "elephant": ("相", "象"), "horse": ("马", "马"),
               "chariot": ("车", "车"), "cannon": ("炮", "炮"), "soldier": ("兵", "卒")]
        var out: [String] = []
        for role in Self.roleOrder {
            let missing = (Self.startingCounts[role] ?? 0) - (pieceCounts["\(side)-\(role)"] ?? 0)
            guard missing > 0, let face = faces[role] else { continue }
            out.append(contentsOf: Array(repeating: side == "red" ? face.red : face.black, count: missing))
        }
        return out
    }
}

/// One sample game as the picker and the replay header show it, with its release's honest labels.
struct SampleGameInfo: Identifiable {
    let id: String
    let redName: String
    let blackName: String
    let event: String
    let date: String
    let result: String
    let releaseId: String
    let isSyntheticFixture: Bool

    init?(summary: [String: Any], release: [String: Any]) {
        guard let id = summary["game_id"] as? String,
              let releaseId = release["release_id"] as? String else { return nil }
        self.id = id
        redName = summary["red_display_name"] as? String ?? "Red"
        blackName = summary["black_display_name"] as? String ?? "Black"
        event = summary["event_name"] as? String ?? ""
        date = summary["date_display"] as? String ?? ""
        result = summary["recorded_result"] as? String ?? ""
        self.releaseId = releaseId
        isSyntheticFixture = release["is_synthetic_fixture"] as? Bool ?? true
    }

    var recordedResultReason = ""
    var rulesProvenance = ""

    /// The record's own result and the rules it was played under, which the replay shows apart from its rules.
    func withRecord(_ record: [String: Any]) -> SampleGameInfo {
        var copy = self
        copy.recordedResultReason = record["recorded_result_reason"] as? String ?? ""
        copy.rulesProvenance = record["original_rules_provenance"] as? String ?? ""
        return copy
    }

    /// The data's provenance, stated on every screen that shows it.
    var provenance: String { isSyntheticFixture ? "Synthetic sample · not a real archive game" : "Archive release" }
}

struct SampleGamePicker: View {
    @ObservedObject var controller: MacBoardController
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let games = controller.sampleGames()
        VStack(alignment: .leading, spacing: 12) {
            Text("Sample games").font(LamplitTheme.display(28))
            if let first = games.first {
                Text(first.provenance).font(.system(size: 12)).foregroundStyle(LamplitTheme.vermilionOnDark)
            }
            if games.isEmpty {
                Text("The sample collection could not be read.").foregroundStyle(LamplitTheme.mutedText)
            }
            ForEach(games) { game in
                Button {
                    dismiss()
                    controller.openSampleGame(id: game.id)
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(game.redName) – \(game.blackName)").font(.system(size: 15, weight: .semibold))
                        Text([game.event, game.date, game.result].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.system(size: 12))
                            .foregroundStyle(LamplitTheme.mutedText)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 6)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(24)
        .frame(width: 440)
    }
}

/// The replay's move list: the recorded line with the shown ply, the variation, and what this Mac keeps.
struct ReplayColumn: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        if let replay = controller.replay, let info = controller.replayInfo {
            VStack(alignment: .leading, spacing: 10) {
                Spacer(minLength: 48)
                Text(info.event).font(.system(size: 13, weight: .semibold))
                Text(info.provenance).font(.system(size: 11)).foregroundStyle(LamplitTheme.vermilionOnDark)
                recordFacts(info, replay: replay)
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 2) {
                        plyButton(0, "Start", current: !replay.isInVariation && replay.cursor == 0)
                        ForEach(Array(controller.replayRecordedNotated.enumerated()), id: \.offset) { index, text in
                            let ply = index + 1
                            plyButton(ply, "\((index / 2) + 1)\(index % 2 == 0 ? "." : "…") \(text)",
                                      current: !replay.isInVariation && replay.cursor == ply)
                        }
                    }
                }
                variationSection(replay)
                Rectangle().fill(LamplitTheme.hairline).frame(height: 1)
                statusSection(replay)
            }
            .padding(.trailing, 40)
            .padding(.bottom, 96)
            .id(controller.replayRevision)
        }
    }

    /// The archive's recorded result and rules provenance, kept apart from the rules this replay checks moves by.
    private func recordFacts(_ info: SampleGameInfo, replay: ReplaySession) -> some View {
        let result = [info.result, info.recordedResultReason].filter { !$0.isEmpty }
            .map { $0.replacingOccurrences(of: "_", with: " ") }.joined(separator: " · ")
        return VStack(alignment: .leading, spacing: 2) {
            Text("Recorded result: \(result.isEmpty ? "not recorded" : result)")
            if !info.rulesProvenance.isEmpty {
                Text("Recorded under: \(info.rulesProvenance)")
            }
            Text("Replay rules: \(replay.rulesProfile)")
        }
        .font(.system(size: 11))
        .foregroundStyle(LamplitTheme.mutedText)
    }

    private func plyButton(_ ply: Int, _ label: String, current: Bool) -> some View {
        Button { controller.replaySelectPly(ply) } label: {
            Text(label)
                .font(.system(size: 14, weight: current ? .semibold : .regular).monospacedDigit())
                .foregroundStyle(current ? LamplitTheme.paper : LamplitTheme.paper.opacity(0.7))
                .underline(current, color: LamplitTheme.paleJade.opacity(0.6))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 3)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(current ? "\(label), shown" : label)
    }

    @ViewBuilder
    private func variationSection(_ replay: ReplaySession) -> some View {
        if let line = replay.variation ?? replay.parked {
            VStack(alignment: .leading, spacing: 6) {
                Text(replay.isInVariation
                     ? "Variation from ply \(line.originPly)"
                     : "Variation set aside at ply \(line.originPly)")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(LamplitTheme.paleJade)
                Text(controller.replayVariationNotated.joined(separator: "  "))
                    .font(.system(size: 13))
                HStack {
                    if replay.isInVariation {
                        Button("Return to game") { controller.replayReturnToGame() }
                    } else {
                        Button("Resume") { controller.replayResume() }
                    }
                }
                if replay.keepsUnshownDraft {
                    Text("This Mac keeps an earlier variation for this game. Any change saved from here on replaces it.")
                        .font(.system(size: 11))
                        .foregroundStyle(LamplitTheme.mutedText)
                }
                if replay.willReplaceDraft {
                    Text("Starting a new variation, from the board or the engine, replaces this one.")
                        .font(.system(size: 11))
                        .foregroundStyle(LamplitTheme.mutedText)
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Text("Play any legal move on the board to try a variation.")
                if replay.willReplaceDraft {
                    // The line this Mac keeps is not shown here, but a new variation would still replace it.
                    Text("Starting a new variation replaces the one this Mac keeps for this game.")
                }
            }
            .font(.system(size: 12))
            .foregroundStyle(LamplitTheme.mutedText)
        }
    }

    @ViewBuilder
    private func statusSection(_ replay: ReplaySession) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            switch replay.saveState {
            case .saved: Text("Saved on this Mac").foregroundStyle(LamplitTheme.mutedText)
            case .unsaved(let reason): Text("Not saved: \(reason)").foregroundStyle(LamplitTheme.vermilionOnDark)
            case .none: EmptyView()
            }
            if let notice = replay.draftNotice {
                Text(notice).foregroundStyle(LamplitTheme.vermilionOnDark)
            }
            if let feedback = replay.feedback {
                Text(feedback).foregroundStyle(LamplitTheme.vermilionOnDark)
            }
            if controller.engineThinking {
                Text("Engine thinking…").foregroundStyle(LamplitTheme.mutedText)
            }
        }
        .font(.system(size: 12))
    }
}

enum LocalUciProcessRunner {
    static func computeBestMove(repoRoot: String, movesUci: [String]) -> String? {
        let candidates = [
            (repoRoot as NSString).appendingPathComponent("engines/mobile-bridge/build/jade-uci-engine"),
            (Bundle.main.bundlePath as NSString).appendingPathComponent("Contents/MacOS/jade-uci-engine"),
        ]
        guard let binPath = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            return nil
        }

        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: binPath)
        let inPipe = Pipe()
        let outPipe = Pipe()
        proc.standardInput = inPipe
        proc.standardOutput = outPipe

        do {
            try proc.run()
        } catch {
            return nil
        }

        let posCmd = movesUci.isEmpty
            ? "position startpos\n"
            : "position startpos moves \(movesUci.joined(separator: " "))\n"
        let script = "uci\nisready\n\(posCmd)go movetime 300\n"
        if let data = script.data(using: .utf8) {
            inPipe.fileHandleForWriting.write(data)
        }

        var collected = ""
        let handle = outPipe.fileHandleForReading
        while true {
            let chunk = handle.availableData
            if chunk.isEmpty { break }
            if let str = String(data: chunk, encoding: .utf8) {
                collected += str
                if let range = collected.range(of: "bestmove ") {
                    let tail = collected[range.upperBound...]
                    let token = tail.split(whereSeparator: { $0.isWhitespace }).first.map(String.init)
                    if let quitData = "quit\n".data(using: .utf8) {
                        try? inPipe.fileHandleForWriting.write(contentsOf: quitData)
                    }
                    proc.terminate()
                    if let tok = token, tok != "(none)", tok.count == 4 {
                        return tok
                    }
                    return nil
                }
            }
        }
        proc.waitUntilExit()
        return nil
    }
}

struct MacCoreGraphicsBoardRepresentable: NSViewRepresentable {
    @ObservedObject var controller: MacBoardController

    func makeNSView(context: Context) -> MacCoreGraphicsBoardNSView {
        let view = MacCoreGraphicsBoardNSView()
        view.controller = controller
        return view
    }

    func updateNSView(_ nsView: MacCoreGraphicsBoardNSView, context: Context) {
        nsView.controller = controller
        nsView.rebuildVoiceOverChildren()
        nsView.needsDisplay = true
    }
}

final class MacCoreGraphicsBoardNSView: NSView {
    weak var controller: MacBoardController?
    private var axElements: [NSAccessibilityElement] = []

    override var acceptsFirstResponder: Bool { true }
    override var isFlipped: Bool { true }
    override var isOpaque: Bool { false }

    /// The board slab: the 8×9 grid plus a 0.75-spacing margin on every side, fitted into the
    /// view with room for the controls capsule below. Within the slab the painter's padding is
    /// 0.25 spacing, because its viewport already reserves half a spacing around the outer lines.
    private func slabGeometry() -> (rect: CGRect, padding: CGFloat)? {
        let avail = CGRect(x: bounds.minX + 24, y: bounds.minY + 56,
                           width: bounds.width - 48, height: bounds.height - 56 - 92)
        let s = floor(min(avail.width / 9.5, avail.height / 10.5))
        guard s > 0 else { return nil }
        let w = 9.5 * s
        let h = 10.5 * s
        let rect = CGRect(x: avail.midX - w / 2, y: avail.minY + (avail.height - h) / 2, width: w, height: h)
        return (rect.integral, 0.25 * s)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext,
              let ctrl = controller,
              let (slab, padding) = slabGeometry(),
              let model = try? ctrl.session.renderModel(traditionalGlyphs: ctrl.traditionalGlyphs, selectedUci: nil) else {
            return
        }
        let shape = CGPath(roundedRect: slab, cornerWidth: 6, cornerHeight: 6, transform: nil)
        let surface = ctrl.darkSurface
            ? CGColor(srgbRed: 0x1D / 255.0, green: 0x24 / 255.0, blue: 0x21 / 255.0, alpha: 1)
            : CGColor(srgbRed: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0, alpha: 1)

        // Cast shadows fall to the lower right, away from the upper-left key light. Offsets are in
        // the unflipped base space, so a negative y moves the shadow down the screen.
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 26, height: -38), blur: 80, color: CGColor(gray: 0, alpha: 0.55))
        ctx.addPath(shape); ctx.setFillColor(surface); ctx.fillPath()
        ctx.restoreGState()
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 8, height: -12), blur: 22, color: CGColor(gray: 0, alpha: 0.38))
        ctx.addPath(shape); ctx.setFillColor(surface); ctx.fillPath()
        ctx.restoreGState()

        ctx.saveGState()
        ctx.addPath(shape)
        ctx.clip()
        ctx.translateBy(x: slab.minX, y: slab.minY)
        CoreGraphicsBoardPainter.paint(
            ctx: ctx,
            width: slab.width,
            height: slab.height,
            padding: padding,
            redBelow: ctrl.redBelow,
            darkSurface: ctrl.darkSurface,
            materialId: ctrl.materialId,
            traditionalGlyphs: ctrl.traditionalGlyphs,
            pieces: model.pieces,
            selectedUci: ctrl.selectedUci,
            legalDestUcis: ctrl.legalDestUcis,
            lastMoveFromUci: model.lastMoveFromUci,
            lastMoveToUci: model.lastMoveToUci,
            checkUci: model.checkUci,
            repoRoot: ctrl.repoRoot
        )
        ctx.restoreGState()

        // A fine top-left highlight and edge line give the slab a physical rim.
        ctx.saveGState()
        ctx.addPath(shape)
        ctx.setStrokeColor(CGColor(gray: 0, alpha: 0.35))
        ctx.setLineWidth(1)
        ctx.strokePath()
        ctx.move(to: CGPoint(x: slab.minX + 6, y: slab.minY + 0.5))
        ctx.addLine(to: CGPoint(x: slab.maxX - 6, y: slab.minY + 0.5))
        ctx.setStrokeColor(CGColor(gray: 1, alpha: ctrl.darkSurface ? 0.08 : 0.6))
        ctx.strokePath()
        ctx.restoreGState()
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let loc = convert(event.locationInWindow, from: nil)
        guard let ctrl = controller, let (slab, padding) = slabGeometry() else { return }
        if let hit = ffiHitTest(
            width: Double(slab.width),
            height: Double(slab.height),
            padding: Double(padding),
            redBelow: ctrl.redBelow,
            px: Double(loc.x - slab.minX),
            py: Double(loc.y - slab.minY)
        ) {
            ctrl.activateSquare(file: Int(hit.file), rank: Int(hit.rank))
        } else {
            ctrl.selectedUci = nil
            ctrl.legalDestUcis.removeAll()
        }
        needsDisplay = true
    }

    override func keyDown(with event: NSEvent) {
        guard let ctrl = controller else { return }
        switch event.keyCode {
        case 123: ctrl.focusedSquare.0 = max(0, ctrl.focusedSquare.0 - 1)
        case 124: ctrl.focusedSquare.0 = min(8, ctrl.focusedSquare.0 + 1)
        case 126: ctrl.focusedSquare.1 = min(9, ctrl.focusedSquare.1 + 1)
        case 125: ctrl.focusedSquare.1 = max(0, ctrl.focusedSquare.1 - 1)
        case 36, 49:
            ctrl.activateSquare(file: ctrl.focusedSquare.0, rank: ctrl.focusedSquare.1)
        default:
            super.keyDown(with: event)
            return
        }
        needsDisplay = true
    }

    func rebuildVoiceOverChildren() {
        guard let ctrl = controller,
              let pieces = try? ctrl.session.pieces(traditionalGlyphs: ctrl.traditionalGlyphs) else {
            return
        }
        axElements = pieces.map { p in
            let el = NSAccessibilityElement()
            el.setAccessibilityRole(.button)
            el.setAccessibilityLabel("\(p.side.capitalized) \(p.role) (\(p.glyph)) at \(p.uci)")
            return el
        }
        setAccessibilityChildren(axElements)
    }
}

enum HeadlessCoreGraphicsProof {
    static func render(repoRoot: String, outputPath: String) throws {
        let session = try FfiGameSession(rulesProfile: "program-2023.11-v1")
        _ = try session.applyMoveToken(token: "h2e2")
        _ = try session.applyMoveToken(token: "h9g7")
        _ = try session.applyMoveToken(token: "h0g2")
        let model = try session.renderModel(traditionalGlyphs: false, selectedUci: nil)

        let w = 760
        let h = 840
        let cs = CGColorSpace(name: CGColorSpace.sRGB)!
        guard let ctx = CGContext(
            data: nil,
            width: w,
            height: h,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: cs,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            fatalError("Failed to create CGContext")
        }
        ctx.translateBy(x: 0, y: CGFloat(h))
        ctx.scaleBy(x: 1, y: -1)

        CoreGraphicsBoardPainter.paint(
            ctx: ctx,
            width: CGFloat(w),
            height: CGFloat(h),
            redBelow: true,
            darkSurface: false,
            materialId: "pale-jade",
            traditionalGlyphs: false,
            pieces: model.pieces,
            selectedUci: "e2",
            legalDestUcis: [],
            lastMoveFromUci: model.lastMoveFromUci,
            lastMoveToUci: model.lastMoveToUci,
            checkUci: model.checkUci,
            repoRoot: repoRoot
        )

        guard let outImg = ctx.makeImage() else {
            fatalError("Failed to extract CGImage")
        }
        let outURL = URL(fileURLWithPath: outputPath)
        guard let dest = CGImageDestinationCreateWithURL(outURL as CFURL, UTType.png.identifier as CFString, 1, nil) else {
            fatalError("Failed to create PNG destination")
        }
        CGImageDestinationAddImage(dest, outImg, nil)
        guard CGImageDestinationFinalize(dest) else {
            fatalError("Failed to write PNG proof")
        }
    }
}

final class MacPieceAudioEngine {
    private let repoRoot: String
    private var audioPlayer: AVAudioPlayer?
    private var toggle = 0

    init(repoRoot: String) {
        self.repoRoot = repoRoot
    }

    func playPlacementOnce(materialId: String) {
        toggle = (toggle + 1) % 2
        let family: String
        switch materialId {
        case "boxwood": family = "boxwood"
        case "ivory-stone": family = "ivory-stone"
        default: family = "jade"
        }
        let slug = toggle == 0 ? "place-1.wav" : "place-2.wav"
        let url = URL(fileURLWithPath: repoRoot)
            .appendingPathComponent("assets/audio/\(family)/\(slug)")
        if let p = try? AVAudioPlayer(contentsOf: url) {
            self.audioPlayer = p
            p.volume = 0.75
            p.play()
        }
    }
}

enum AppleKeychainSessionStore {
    static func saveRefreshToken(installId: String, token: String) -> Bool {
        let data = Data(token.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "play.jade.macos.dev",
            kSecAttrAccount as String: installId,
            kSecValueData as String: data,
        ]
        SecItemDelete(query as CFDictionary)
        return SecItemAdd(query as CFDictionary, nil) == errSecSuccess
    }
}
