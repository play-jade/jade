import AVFoundation
import CoreGraphics
import Foundation
import JadeCoreBindings
import PencilKit
import Security
import SQLite3
import SwiftUI
import UIKit

/// Native iOS and iPadOS client using SwiftUI + UIKit (`UIView` Core Graphics board + VoiceOver
/// `UIAccessibilityElement` container), distinct iPhone thumb-first layout vs. iPad split-screen
/// study layout with optional Apple Pencil `PKCanvasView` overlay, `AVAudioSession` interruption
/// handling, Keychain session storage, and SQLite3 offline play.
@main
struct JadeIOSApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var model = IOSBoardModel()

    var body: some Scene {
        WindowGroup {
            JadeAdaptiveAppleRootView(model: model)
                .onChange(of: scenePhase) { _, phase in
                    if phase != .active {
                        model.suspendForBackground()
                    } else {
                        model.resumeForeground()
                    }
                }
                .onOpenURL { url in
                    model.handleUniversalLink(url)
                }
        }
    }
}

struct JadeAdaptiveAppleRootView: View {
    @ObservedObject var model: IOSBoardModel
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @State private var showMoveSheet = false
    @State private var enablePencilAnnotation = false

    var body: some View {
        let isPadRegular = UIDevice.current.userInterfaceIdiom == .pad && horizontalSizeClass == .regular
        if isPadRegular {
            NavigationSplitView {
                List {
                    Section("Study & Replay") {
                        ForEach(Array(model.movesChinese.enumerated()), id: \.offset) { idx, mv in
                            Text("\(idx + 1). \(mv)")
                                .font(.system(.body, design: .monospaced))
                        }
                    }
                    Section("Presentation") {
                        Picker("Material", selection: $model.materialId) {
                            Text("Pale Jade").tag("pale-jade")
                            Text("Boxwood").tag("boxwood")
                            Text("Ivory Stone").tag("ivory-stone")
                        }
                        Toggle("Traditional Faces", isOn: $model.traditionalGlyphs)
                        Toggle("Dark Surface", isOn: $model.darkSurface)
                        Toggle("Pencil Study Overlay", isOn: $enablePencilAnnotation)
                    }
                }
                .navigationTitle("Jade · 玉枰象棋")
            } detail: {
                ZStack {
                    IOSCoreGraphicsBoardRepresentable(model: model)
                    if enablePencilAnnotation {
                        IOSPencilAnnotationOverlay()
                            .allowsHitTesting(true)
                    }
                }
                .toolbar {
                    ToolbarItemGroup(placement: .primaryAction) {
                        Button("New Offline") { model.resetOffline() }
                        Button("Flip") { model.redBelow.toggle() }
                        Button("Undo") { model.undo() }
                    }
                }
            }
        } else {
            VStack(spacing: 10) {
                HStack {
                    Text("Jade · 玉枰象棋")
                        .font(.headline)
                    Spacer()
                    Text(model.statusLine)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 12)

                IOSCoreGraphicsBoardRepresentable(model: model)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)

                HStack(spacing: 10) {
                    Button("Flip") { model.redBelow.toggle() }
                        .buttonStyle(.bordered)
                    Button("Undo") { model.undo() }
                        .buttonStyle(.bordered)
                    Button("Surface") { model.darkSurface.toggle() }
                        .buttonStyle(.bordered)
                    Button("Moves (\(model.movesChinese.count))") { showMoveSheet = true }
                        .buttonStyle(.borderedProminent)
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
            }
            .sheet(isPresented: $showMoveSheet) {
                NavigationStack {
                    List(Array(model.movesChinese.enumerated()), id: \.offset) { idx, mv in
                        Text("\(idx + 1). \(mv)")
                    }
                    .navigationTitle("Move History")
                }
                .presentationDetents([.medium, .large])
            }
        }
    }
}

final class IOSBoardModel: ObservableObject {
    @Published var redBelow = true
    @Published var darkSurface = false
    @Published var materialId = "pale-jade"
    @Published var traditionalGlyphs = false
    @Published var selectedUci: String? = nil
    @Published var movesChinese: [String] = []
    @Published var statusLine = "Red to move · Offline"
    @Published var pendingUniversalLink: URL? = nil

    private let offlineStore = AppleOfflineSqliteStore(
        directory: NSSearchPathForDirectoriesInDomains(.applicationSupportDirectory, .userDomainMask, true).first
            ?? NSTemporaryDirectory(),
        filename: "jade_ios_offline.sqlite3"
    )
    private(set) var session: FfiGameSession

    init() {
        let s = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        var restoredZh: [String] = []
        for uci in offlineStore.loadMoves() {
            if let zh = try? s.applyMoveToken(token: uci) {
                restoredZh.append(zh)
            } else {
                break
            }
        }
        self.session = s
        self.movesChinese = restoredZh
        refreshStatus()
    }

    func currentRenderModel() -> FfiBoardRenderModel? {
        try? session.renderModel(traditionalGlyphs: traditionalGlyphs, selectedUci: selectedUci)
    }

    func tapSquare(file: Int, rank: Int) {
        let uci = "\(Character(UnicodeScalar(97 + file)!))\(rank)"
        let legalMoves = (try? session.summary())?.legalMovesUci ?? []
        if selectedUci == nil {
            if legalMoves.contains(where: { $0.hasPrefix(uci) }) {
                selectedUci = uci
            }
        } else if selectedUci == uci {
            selectedUci = nil
        } else if let from = selectedUci {
            let moveUci = "\(from)\(uci)"
            if let zh = try? session.applyMoveToken(token: moveUci) {
                movesChinese.append(zh)
                offlineStore.appendMove(uci: moveUci)
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                selectedUci = nil
            } else if legalMoves.contains(where: { $0.hasPrefix(uci) }) {
                selectedUci = uci
            } else {
                selectedUci = nil
            }
        }
        refreshStatus()
    }

    func resetOffline() {
        session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        selectedUci = nil
        movesChinese.removeAll()
        offlineStore.clearMoves()
        refreshStatus()
    }

    func undo() {
        if (try? session.undo()) != nil, !movesChinese.isEmpty {
            movesChinese.removeLast()
            offlineStore.popLastMove()
            selectedUci = nil
            refreshStatus()
        }
    }

    func refreshStatus() {
        if let sum = try? session.summary() {
            if let outcome = sum.outcome {
                statusLine = "Finished: \(outcome)"
            } else {
                let chk = sum.inCheck ? " · Check" : ""
                statusLine = "\(sum.sideToMove.capitalized) to move · \(sum.plyCount) plies\(chk)"
            }
        }
    }

    func suspendForBackground() {
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    func resumeForeground() {
        refreshStatus()
    }

    func handleUniversalLink(_ url: URL) {
        pendingUniversalLink = url
    }
}

struct IOSCoreGraphicsBoardRepresentable: UIViewRepresentable {
    @ObservedObject var model: IOSBoardModel

    func makeUIView(context: Context) -> IOSBoardUIView {
        let v = IOSBoardUIView()
        v.model = model
        return v
    }

    func updateUIView(_ uiView: IOSBoardUIView, context: Context) {
        uiView.model = model
        uiView.rebuildVoiceOverElements()
        uiView.setNeedsDisplay()
    }
}

final class IOSBoardUIView: UIView {
    weak var model: IOSBoardModel?
    private static let boardPadding: CGFloat = 16.0

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = true
        contentMode = .redraw
        isAccessibilityElement = false
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(onTap(_:))))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext(),
              let m = model,
              let render = m.currentRenderModel() else { return }

        CoreGraphicsBoardPainter.paint(
            ctx: ctx,
            width: bounds.width,
            height: bounds.height,
            padding: Self.boardPadding,
            redBelow: m.redBelow,
            darkSurface: m.darkSurface,
            materialId: m.materialId,
            traditionalGlyphs: m.traditionalGlyphs,
            pieces: render.pieces,
            selectedUci: render.selectedUci,
            legalDestUcis: Set(render.legalDestsUci),
            lastMoveFromUci: render.lastMoveFromUci,
            lastMoveToUci: render.lastMoveToUci,
            checkUci: render.checkUci,
            repoRoot: ProcessInfo.processInfo.environment["JADE_REPO_ROOT"]
        )
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        rebuildVoiceOverElements()
        setNeedsDisplay()
    }

    @objc private func onTap(_ recognizer: UITapGestureRecognizer) {
        guard let m = model else { return }
        let pt = recognizer.location(in: self)
        if let hit = ffiHitTest(
            width: Double(bounds.width),
            height: Double(bounds.height),
            padding: Double(Self.boardPadding),
            redBelow: m.redBelow,
            px: Double(pt.x),
            py: Double(pt.y)
        ) {
            m.tapSquare(file: Int(hit.file), rank: Int(hit.rank))
        } else {
            m.selectedUci = nil
        }
        rebuildVoiceOverElements()
        setNeedsDisplay()
    }

    func rebuildVoiceOverElements() {
        guard let m = model,
              let render = m.currentRenderModel() else {
            return
        }
        accessibilityElements = render.pieces.map { p in
            let el = UIAccessibilityElement(accessibilityContainer: self)
            let selSuffix = render.selectedUci == p.uci ? ", selected" : ""
            el.accessibilityLabel = "\(p.side.capitalized) \(p.role) (\(p.glyph)) at \(p.uci)\(selSuffix)"
            el.accessibilityTraits = .button
            el.accessibilityFrameInContainerSpace = CoreGraphicsBoardPainter.pieceAccessibilityRect(
                width: bounds.width,
                height: bounds.height,
                padding: Self.boardPadding,
                redBelow: m.redBelow,
                file: Int(p.file),
                rank: Int(p.rank)
            )
            return el
        }
    }
}

struct IOSPencilAnnotationOverlay: UIViewRepresentable {
    func makeUIView(context: Context) -> PKCanvasView {
        let canvas = PKCanvasView()
        canvas.backgroundColor = .clear
        canvas.isOpaque = false
        canvas.drawingPolicy = .pencilOnly
        return canvas
    }

    func updateUIView(_ uiView: PKCanvasView, context: Context) {}
}
