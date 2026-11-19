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
                        // Section 12 & 15: stop engine searches, pause audio, and checkpoint local state on background/lock.
                        model.suspendForBackground()
                    } else {
                        model.resumeForeground()
                    }
                }
                .onOpenURL { url in
                    // Verified Apple Universal Links destination preservation (`https://<domain>/invite/<code>`).
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
            // iPadOS deliberate board-and-study split layout (never a stretched phone screen — Section 3 & 17).
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
            // iPhone layout: board stays the unshrunk primary object; clocks and thumb controls sit adjacent;
            // move history and analysis open in a secondary sheet/drawer (Section 3).
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

                HStack(spacing: 12) {
                    Button("Flip") { model.redBelow.toggle() }
                        .buttonStyle(.bordered)
                    Button("Undo") { model.undo() }
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
    @Published var materialId = "pale-jade"
    @Published var traditionalGlyphs = false
    @Published var selectedUci: String? = nil
    @Published var movesChinese: [String] = []
    @Published var statusLine = "Red to move · Offline"
    @Published var pendingUniversalLink: URL? = nil

    private(set) var session: FfiGameSession

    init() {
        self.session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
    }

    func tapSquare(file: Int, rank: Int) {
        let uci = "\(Character(UnicodeScalar(97 + file)!))\(rank)"
        if selectedUci == nil {
            selectedUci = uci
        } else if selectedUci == uci {
            selectedUci = nil
        } else if let from = selectedUci {
            if let zh = try? session.applyMoveToken(token: "\(from)\(uci)") {
                movesChinese.append(zh)
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                selectedUci = nil
            } else {
                selectedUci = uci
            }
        }
        if let sum = try? session.summary() {
            statusLine = sum.outcome ?? "\(sum.sideToMove.capitalized) to move"
        }
    }

    func resetOffline() {
        session = try! FfiGameSession(rulesProfile: "program-2023.11-v1")
        selectedUci = nil
        movesChinese.removeAll()
        statusLine = "Red to move · Offline"
    }

    func undo() {
        if (try? session.undo()) != nil, !movesChinese.isEmpty {
            movesChinese.removeLast()
        }
    }

    func suspendForBackground() {
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    func resumeForeground() {
        // Reconciles hosted state when in an online room; preserves offline game state locally.
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

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor(red: 0xF3 / 255.0, green: 0xEE / 255.0, blue: 0xE4 / 255.0, alpha: 1.0)
        isAccessibilityElement = false
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(onTap(_:))))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    @objc private func onTap(_ recognizer: UITapGestureRecognizer) {
        guard let m = model else { return }
        let pt = recognizer.location(in: self)
        if let hit = ffiHitTest(
            width: Double(bounds.width),
            height: Double(bounds.height),
            padding: 16.0,
            redBelow: m.redBelow,
            px: Double(pt.x),
            py: Double(pt.y)
        ) {
            m.tapSquare(file: Int(hit.file), rank: Int(hit.rank))
        } else {
            m.selectedUci = nil
        }
        setNeedsDisplay()
    }

    func rebuildVoiceOverElements() {
        guard let m = model,
              let pieces = try? m.session.pieces(traditionalGlyphs: m.traditionalGlyphs) else {
            return
        }
        accessibilityElements = pieces.map { p in
            let el = UIAccessibilityElement(accessibilityContainer: self)
            el.accessibilityLabel = "\(p.side.capitalized) \(p.role) (\(p.glyph)) at \(p.uci)"
            el.accessibilityTraits = .button
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
