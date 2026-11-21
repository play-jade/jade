import Foundation
import JadeCoreBindings

@main
struct SwiftRulesFixtureProof {
    static func main() throws {
        let pkgDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let repoRoot = pkgDir.deletingLastPathComponent().deletingLastPathComponent()

        // 1. Verify opening position & Chinese notation via UniFFI Swift
        let game = try FfiGameSession(rulesProfile: "program-2023.11-v1")
        let initial = try game.summary()
        guard initial.sideToMove == "red", initial.legalMovesUci.count == 44 else {
            fatalError("Unexpected initial state: \(initial)")
        }
        let zh = try game.applyMoveToken(token: "h2e2")
        guard zh == "炮二平五" else {
            fatalError("Expected 炮二平五, got \(zh)")
        }
        let rm = try game.renderModel(traditionalGlyphs: true, selectedUci: "b9")
        guard rm.pieces.count == 32,
              rm.lastMoveFromUci == "h2",
              rm.lastMoveToUci == "e2",
              rm.selectedUci == "b9",
              rm.legalDestsUci.contains("c7") else {
            fatalError("Unexpected renderModel: \(rm)")
        }

        // 2. Verify Section 4 non-clamping hit test via UniFFI Swift
        if ffiHitTest(width: 486.0, height: 540.0, padding: 18.0, redBelow: true, px: 2.0, py: 2.0) != nil {
            fatalError("Expected nil outside outer half-cell")
        }
        guard let hit = ffiHitTest(width: 486.0, height: 540.0, padding: 18.0, redBelow: true, px: 43.0, py: 495.0),
              hit.uci == "a0" else {
            fatalError("Expected a0 at bottom-left origin")
        }

        // 3. Verify all 39 published Pikafish repetition/chase examples through Swift UniFFI!
        let fixturesURL = repoRoot.appendingPathComponent("data/fixtures/rules/pikafish_examples.json")
        let data = try Data(contentsOf: fixturesURL)
        let root = try JSONSerialization.jsonObject(with: data) as! [String: Any]
        let examples = root["examples"] as! [[String: Any]]
        guard examples.count == 39 else {
            fatalError("Expected 39 examples, got \(examples.count)")
        }

        for ex in examples {
            let id = ex["id"] as! String
            let fen = ex["fen"] as! String
            let moves = ex["moves_uci"] as! [String]
            let expectedOutcome = ex["expected_outcome"] as! String

            let session = try FfiGameSession.fromFen(fen: fen, rulesProfile: "program-2023.11-v1")
            for mv in moves {
                _ = try session.applyMoveToken(token: mv)
            }
            let endSummary = try session.summary()
            guard endSummary.outcome == expectedOutcome else {
                fatalError("Swift UniFFI mismatch on \(id): expected \(expectedOutcome), got \(String(describing: endSummary.outcome))")
            }
        }

        print("SWIFT_UNIFFI_FIXTURE_PROOF_OK: 39/39 Pikafish examples + geometry + Chinese notation verified")
    }
}
