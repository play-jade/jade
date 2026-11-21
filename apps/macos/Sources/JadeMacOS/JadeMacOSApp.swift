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
    static let vermilionOnDark = Color(red: 0xC4 / 255.0, green: 0x5A / 255.0, blue: 0x45 / 255.0)
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
                MovesColumn(controller: controller)
                    .frame(width: 280)
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

    var body: some View {
        let toMove = controller.sideToMove == side && controller.outcomeText == nil
        let isEngine = side == "black" && controller.autoEngineBlack
        let title = side == "red" ? "Red" : "Black"
        let who = isEngine ? "Engine" : "This Mac"
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Circle()
                    .fill(toMove ? LamplitTheme.paleJade : LamplitTheme.hairline)
                    .frame(width: 8, height: 8)
                    .shadow(color: toMove ? LamplitTheme.paleJade.opacity(0.6) : .clear, radius: 6)
                Text("\(who)\(toMove ? " · to move" : "")".uppercased())
                    .font(.system(size: 11, weight: .medium))
                    .tracking(1.8)
                    .foregroundStyle(toMove ? LamplitTheme.paleJade : LamplitTheme.mutedText)
            }
            Text(title)
                .font(LamplitTheme.display(56))
                .foregroundStyle(toMove ? LamplitTheme.paper : LamplitTheme.mutedText)
            if isEngine && controller.engineThinking {
                Text("Thinking…")
                    .font(.system(size: 13))
                    .foregroundStyle(LamplitTheme.mutedText)
            }
            let taken = controller.capturedGlyphs(ownedBy: side)
            if !taken.isEmpty {
                Text(taken.joined(separator: " "))
                    .font(.system(size: 17, weight: .semibold, design: .serif))
                    .foregroundStyle(LamplitTheme.mutedText)
                    .accessibilityLabel("\(title) has lost \(taken.count) pieces")
            }
        }
        .animation(.easeOut(duration: 0.18), value: toMove)
    }
}

struct MovesColumn: View {
    @ObservedObject var controller: MacBoardController

    var body: some View {
        let rows = stride(from: 0, to: controller.movesNotated.count, by: 2).map { i in
            (number: i / 2 + 1,
             red: controller.movesNotated[i],
             black: i + 1 < controller.movesNotated.count ? controller.movesNotated[i + 1] : "")
        }
        let lastPly = controller.movesNotated.count - 1
        VStack(alignment: .leading, spacing: 0) {
            Spacer(minLength: 64)
            ScrollViewReader { proxy in
                ScrollView(showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(rows, id: \.number) { row in
                            HStack(alignment: .firstTextBaseline, spacing: 0) {
                                Text("\(row.number)")
                                    .font(.system(size: 12).monospacedDigit())
                                    .foregroundStyle(LamplitTheme.mutedText)
                                    .frame(width: 32, alignment: .leading)
                                moveText(row.red, current: (row.number - 1) * 2 == lastPly)
                                moveText(row.black, current: (row.number - 1) * 2 + 1 == lastPly)
                            }
                            .padding(.vertical, 6)
                            .id(row.number)
                        }
                    }
                }
                .defaultScrollAnchor(.bottom)
                .onChange(of: controller.movesNotated.count) {
                    if let last = rows.last { proxy.scrollTo(last.number, anchor: .bottom) }
                }
            }
            Rectangle().fill(LamplitTheme.hairline).frame(height: 1).padding(.vertical, 14)
            Text(controller.outcomeText ?? (controller.inCheck ? "Check" : "Offline · Untimed"))
                .font(.system(size: 12))
                .foregroundStyle(controller.inCheck || controller.outcomeText != nil
                    ? LamplitTheme.vermilionOnDark : LamplitTheme.mutedText)
        }
        .padding(.trailing, 56)
        .padding(.bottom, 96)
    }

    private func moveText(_ text: String, current: Bool) -> some View {
        Text(text)
            .font(.system(size: 15, weight: current ? .semibold : .regular).monospacedDigit())
            .foregroundStyle(current ? LamplitTheme.paper : LamplitTheme.paper.opacity(0.78))
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
            capsuleButton("arrow.uturn.backward", "Take back (⌘Z)") { controller.undo() }
                .disabled(controller.movesUci.isEmpty || controller.engineThinking)
            capsuleButton("cpu", "Engine move (⌘E)") { controller.requestEngineMove() }
                .disabled(controller.engineThinking || controller.outcomeText != nil)
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
        for uci in offlineStore.loadMoves() {
            if let zh = try? session.applyMoveToken(token: uci) {
                movesUci.append(uci)
                movesChinese.append(zh)
            }
        }
        refreshStatus()
    }

    func activateSquare(file: Int, rank: Int) {
        if engineThinking { return }
        let uci = "\(Character(UnicodeScalar(97 + file)!))\(rank)"
        if selectedUci == nil {
            selectIfFriendly(uci: uci)
        } else if selectedUci == uci {
            selectedUci = nil
            legalDestUcis.removeAll()
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
        guard !engineThinking,
              let sum = try? session.summary(),
              sum.outcome == nil else { return }
        let currentMoves = movesUci
        let root = repoRoot
        engineThinking = true
        statusLine = "C++26 engine searching…"

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let best = LocalUciProcessRunner.computeBestMove(repoRoot: root, movesUci: currentMoves)
            DispatchQueue.main.async {
                guard let self else { return }
                self.engineThinking = false
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
        session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        selectedUci = nil
        legalDestUcis.removeAll()
        movesUci.removeAll()
        movesChinese.removeAll()
        offlineStore.clearMoves()
        refreshStatus()
    }

    func undo() {
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
        guard let replay = try? FfiGameSession(rulesProfile: "program-2023.11-v1") else { return }
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
        let pieces = try session.pieces(traditionalGlyphs: false)

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
            pieces: pieces,
            selectedUci: "e2",
            legalDestUcis: [],
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
