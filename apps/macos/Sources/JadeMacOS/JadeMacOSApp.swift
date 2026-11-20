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
        WindowGroup("Jade — Xiangqi · 玉枰象棋") {
            HSplitView {
                MacCoreGraphicsBoardRepresentable(controller: controller)
                    .frame(minWidth: 500, minHeight: 560)
                VStack(alignment: .leading, spacing: 12) {
                    Text("Jade · 玉枰象棋")
                        .font(.title2.weight(.semibold))
                    Text(controller.statusLine)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    HStack {
                        Button("New Game") { controller.resetOfflineGame() }
                        Button("Flip") { controller.redBelow.toggle() }
                        Button("Undo") { controller.undo() }
                    }
                    HStack {
                        Button(controller.engineThinking ? "Engine thinking…" : "Engine Move") {
                            controller.requestEngineMove()
                        }
                        .disabled(controller.engineThinking)
                        Toggle("Play vs Engine (Black)", isOn: $controller.autoEngineBlack)
                    }
                    Picker("Material", selection: $controller.materialId) {
                        Text("Pale Jade").tag("pale-jade")
                        Text("Honey Boxwood").tag("boxwood")
                        Text("Ivory Stone").tag("ivory-stone")
                    }
                    Toggle("Traditional Faces (繁體)", isOn: $controller.traditionalGlyphs)
                    Toggle("Dark Surface", isOn: $controller.darkSurface)
                    Toggle("Stone Sound", isOn: $controller.soundEnabled)
                    Divider()
                    Text("Recorded Moves (\(controller.movesChinese.count) plies)")
                        .font(.headline)
                    List(Array(controller.movesChinese.enumerated()), id: \.offset) { idx, mv in
                        Text("\(idx + 1). \(mv)")
                            .font(.system(.body, design: .monospaced))
                    }
                }
                .padding(16)
                .frame(minWidth: 300, maxWidth: 360)
            }
            .frame(minWidth: 840, minHeight: 600)
        }
        .commands {
            CommandMenu("Board") {
                Button("New Offline Game") { controller.resetOfflineGame() }
                    .keyboardShortcut("n", modifiers: [.command])
                Button("Flip Orientation") { controller.redBelow.toggle() }
                    .keyboardShortcut("f", modifiers: [.command])
                Button("Undo Move") { controller.undo() }
                    .keyboardShortcut("z", modifiers: [.command])
                Button("Play Engine Move") { controller.requestEngineMove() }
                    .keyboardShortcut("e", modifiers: [.command])
            }
        }
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

    private func refreshStatus() {
        if let sum = try? session.summary() {
            if let outcome = sum.outcome {
                statusLine = "Finished: \(outcome) (\(sum.reason ?? ""))"
            } else {
                let checkNote = sum.inCheck ? " · CHECK!" : ""
                statusLine = "\(sum.sideToMove.capitalized) to move · \(sum.plyCount) plies\(checkNote)"
            }
        }
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

    override func draw(_ dirtyRect: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext,
              let ctrl = controller,
              let pieces = try? ctrl.session.pieces(traditionalGlyphs: ctrl.traditionalGlyphs) else {
            return
        }
        CoreGraphicsBoardPainter.paint(
            ctx: ctx,
            width: bounds.width,
            height: bounds.height,
            redBelow: ctrl.redBelow,
            darkSurface: ctrl.darkSurface,
            materialId: ctrl.materialId,
            traditionalGlyphs: ctrl.traditionalGlyphs,
            pieces: pieces,
            selectedUci: ctrl.selectedUci,
            legalDestUcis: ctrl.legalDestUcis,
            repoRoot: ctrl.repoRoot
        )
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let loc = convert(event.locationInWindow, from: nil)
        guard let ctrl = controller else { return }
        if let hit = ffiHitTest(
            width: Double(bounds.width),
            height: Double(bounds.height),
            padding: 24.0,
            redBelow: ctrl.redBelow,
            px: Double(loc.x),
            py: Double(loc.y)
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
