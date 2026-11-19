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
    private let offlineStore = MacOfflineSqliteStore()

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

enum CoreGraphicsBoardPainter {
    static func paint(
        ctx: CGContext,
        width: CGFloat,
        height: CGFloat,
        redBelow: Bool,
        darkSurface: Bool,
        materialId: String,
        traditionalGlyphs: Bool,
        pieces: [FfiPieceItem],
        selectedUci: String?,
        legalDestUcis: Set<String>,
        repoRoot: String
    ) {
        let p: CGFloat = 24.0
        let s = min((width - 2.0 * p) / 9.0, (height - 2.0 * p) / 10.0)
        guard s > 0 else { return }
        let ox = (width - 8.0 * s) / 2.0
        let oy = (height - 9.0 * s) / 2.0

        let bg = darkSurface
            ? CGColor(srgbRed: 0x1D / 255.0, green: 0x24 / 255.0, blue: 0x21 / 255.0, alpha: 1.0)
            : CGColor(srgbRed: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0, alpha: 1.0)
        let lineInk = darkSurface
            ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 0.78)
            : CGColor(srgbRed: 0x25 / 255.0, green: 0x2A / 255.0, blue: 0x25 / 255.0, alpha: 0.88)

        ctx.setFillColor(bg)
        ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))

        // Outer frame + exact 9x10 grid + river + palace diagonals
        ctx.setStrokeColor(lineInk)
        ctx.setLineWidth(2.0)
        ctx.stroke(CGRect(x: ox - 5, y: oy - 5, width: 8 * s + 10, height: 9 * s + 10))

        ctx.setLineWidth(1.25)
        for r in 0...9 {
            let y = oy + CGFloat(r) * s
            ctx.move(to: CGPoint(x: ox, y: y))
            ctx.addLine(to: CGPoint(x: ox + 8 * s, y: y))
        }
        for f in 0...8 {
            let x = ox + CGFloat(f) * s
            if f == 0 || f == 8 {
                ctx.move(to: CGPoint(x: x, y: oy))
                ctx.addLine(to: CGPoint(x: x, y: oy + 9 * s))
            } else {
                ctx.move(to: CGPoint(x: x, y: oy))
                ctx.addLine(to: CGPoint(x: x, y: oy + 4 * s))
                ctx.move(to: CGPoint(x: x, y: oy + 5 * s))
                ctx.addLine(to: CGPoint(x: x, y: oy + 9 * s))
            }
        }
        // Palace diagonals
        ctx.move(to: CGPoint(x: ox + 3 * s, y: oy)); ctx.addLine(to: CGPoint(x: ox + 5 * s, y: oy + 2 * s))
        ctx.move(to: CGPoint(x: ox + 5 * s, y: oy)); ctx.addLine(to: CGPoint(x: ox + 3 * s, y: oy + 2 * s))
        ctx.move(to: CGPoint(x: ox + 3 * s, y: oy + 7 * s)); ctx.addLine(to: CGPoint(x: ox + 5 * s, y: oy + 9 * s))
        ctx.move(to: CGPoint(x: ox + 5 * s, y: oy + 7 * s)); ctx.addLine(to: CGPoint(x: ox + 3 * s, y: oy + 9 * s))
        ctx.strokePath()

        let scriptDir = traditionalGlyphs ? "traditional" : "simplified"
        let shadowURL = URL(fileURLWithPath: repoRoot)
            .appendingPathComponent("assets/board/materials/contact-shadow@2x.png")
        let shadowImg = loadCGImage(url: shadowURL)

        let occupied = Set(pieces.map(\.uci))
        let spriteSpan = s * 0.96
        for p in pieces {
            let sx = redBelow ? ox + CGFloat(p.file) * s : ox + CGFloat(8 - p.file) * s
            let sy = redBelow ? oy + CGFloat(9 - p.rank) * s : oy + CGFloat(p.rank) * s
            let dstRect = CGRect(x: sx - spriteSpan / 2, y: sy - spriteSpan / 2, width: spriteSpan, height: spriteSpan)

            if let sh = shadowImg {
                ctx.saveGState()
                ctx.translateBy(x: dstRect.minX, y: dstRect.maxY)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(sh, in: CGRect(x: 2.0, y: -3.0, width: spriteSpan, height: spriteSpan))
                ctx.restoreGState()
            }

            let spriteName = "\(p.side)-\(p.role)@2x.png"
            let spriteURL = URL(fileURLWithPath: repoRoot)
                .appendingPathComponent("assets/board/materials/\(materialId)/\(scriptDir)/\(spriteName)")
            if let cgSprite = loadCGImage(url: spriteURL) {
                ctx.saveGState()
                ctx.translateBy(x: dstRect.minX, y: dstRect.maxY)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(cgSprite, in: CGRect(origin: .zero, size: dstRect.size))
                ctx.restoreGState()
            }

            if selectedUci == p.uci {
                ctx.setStrokeColor(CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 1.0))
                ctx.setLineWidth(3.0)
                let ringR = s * 0.45
                ctx.strokeEllipse(in: CGRect(x: sx - ringR, y: sy - ringR, width: ringR * 2, height: ringR * 2))
            }
        }

        // Legal destination indicators (dots for empty intersections, vermilion rings for captures)
        for dest in legalDestUcis {
            guard dest.count == 2,
                  let fAscii = dest.first?.asciiValue,
                  let rVal = Int(String(dest.last!)) else { continue }
            let f = Int(fAscii) - 97
            let sx = redBelow ? ox + CGFloat(f) * s : ox + CGFloat(8 - f) * s
            let sy = redBelow ? oy + CGFloat(9 - rVal) * s : oy + CGFloat(rVal) * s
            if occupied.contains(dest) {
                ctx.setStrokeColor(CGColor(srgbRed: 0xA5 / 255.0, green: 0x48 / 255.0, blue: 0x37 / 255.0, alpha: 0.95))
                ctx.setLineWidth(2.6)
                let cr = s * 0.46
                ctx.strokeEllipse(in: CGRect(x: sx - cr, y: sy - cr, width: cr * 2, height: cr * 2))
            } else {
                let pipColor = darkSurface
                    ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 0.85)
                    : CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 0.82)
                ctx.setFillColor(pipColor)
                let pr = max(4.5, s * 0.13)
                ctx.fillEllipse(in: CGRect(x: sx - pr, y: sy - pr, width: pr * 2, height: pr * 2))
            }
        }
    }

    private static func loadCGImage(url: URL) -> CGImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(src, 0, nil)
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
        case "ivory-stone": family = "soft-felt"
        default: family = "jade-stone"
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

final class MacOfflineSqliteStore {
    private var db: OpaquePointer?

    init() {
        let path = (NSTemporaryDirectory() as NSString).appendingPathComponent("jade_macos_offline.sqlite3")
        if sqlite3_open(path, &db) == SQLITE_OK {
            sqlite3_exec(
                db,
                "CREATE TABLE IF NOT EXISTS offline_moves (seq INTEGER PRIMARY KEY AUTOINCREMENT, uci TEXT NOT NULL);",
                nil, nil, nil
            )
        }
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    func loadMoves() -> [String] {
        guard let db else { return [] }
        var stmt: OpaquePointer?
        var out: [String] = []
        if sqlite3_prepare_v2(db, "SELECT uci FROM offline_moves ORDER BY seq ASC;", -1, &stmt, nil) == SQLITE_OK {
            while sqlite3_step(stmt) == SQLITE_ROW {
                if let cStr = sqlite3_column_text(stmt, 0) {
                    out.append(String(cString: cStr))
                }
            }
        }
        sqlite3_finalize(stmt)
        return out
    }

    func appendMove(uci: String) {
        guard let db else { return }
        var stmt: OpaquePointer?
        if sqlite3_prepare_v2(db, "INSERT INTO offline_moves (uci) VALUES (?);", -1, &stmt, nil) == SQLITE_OK {
            sqlite3_bind_text(stmt, 1, (uci as NSString).utf8String, -1, nil)
            sqlite3_step(stmt)
        }
        sqlite3_finalize(stmt)
    }

    func clearMoves() {
        if let db { sqlite3_exec(db, "DELETE FROM offline_moves;", nil, nil, nil) }
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
