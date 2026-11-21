#if canImport(CoreGraphics) && canImport(CoreText) && canImport(ImageIO)
import CoreGraphics
import CoreText
import Foundation
import ImageIO

/// Shared Core Graphics Xiangqi board and piece renderer used across native macOS (`NSView`)
/// and iOS/iPadOS (`UIView`). Supports high-density pre-rendered material sprites when
/// `repoRoot` is present and seamless procedural Core Graphics + Core Text carved discs in
/// standalone simulator/device app bundles.
public enum CoreGraphicsBoardPainter {
    public static func paint(
        ctx: CGContext,
        width: CGFloat,
        height: CGFloat,
        padding: CGFloat = 24.0,
        redBelow: Bool,
        darkSurface: Bool,
        materialId: String,
        traditionalGlyphs: Bool,
        pieces: [FfiPieceItem],
        selectedUci: String?,
        legalDestUcis: Set<String>,
        lastMoveFromUci: String? = nil,
        lastMoveToUci: String? = nil,
        checkUci: String? = nil,
        repoRoot: String? = nil
    ) {
        let s = min((width - 2.0 * padding) / 9.0, (height - 2.0 * padding) / 10.0)
        guard s > 0 else { return }
        let ox = (width - 8.0 * s) / 2.0
        let oy = (height - 9.0 * s) / 2.0

        // Surface, ink and marker values match the web board (apps/web/app/components/XiangqiBoard.vue), so the
        // board reads the same on every client.
        let bg = darkSurface
            ? CGColor(srgbRed: 0x22 / 255.0, green: 0x2B / 255.0, blue: 0x27 / 255.0, alpha: 1.0)
            : CGColor(srgbRed: 0xED / 255.0, green: 0xE5 / 255.0, blue: 0xD6 / 255.0, alpha: 1.0)
        let lineInk = darkSurface
            ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 0.72)
            : CGColor(srgbRed: 0x25 / 255.0, green: 0x2A / 255.0, blue: 0x25 / 255.0, alpha: 0.76)
        let markerInk = darkSurface
            ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 1.0)
            : CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 1.0)
        let riverInk = darkSurface
            ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 0.56)
            : CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 0.68)
        let pieceDiameter = s * 0.88
        let vermilion = CGColor(srgbRed: 0xA5 / 255.0, green: 0x48 / 255.0, blue: 0x37 / 255.0, alpha: 1.0)

        ctx.setFillColor(bg)
        ctx.fill(CGRect(x: 0, y: 0, width: width, height: height))
        // Low-contrast grain over the surface, as the web board lays its grain tile.
        if let root = repoRoot, !root.isEmpty,
           let grain = loadCGImage(url: URL(fileURLWithPath: root)
               .appendingPathComponent("assets/board/materials/\(darkSurface ? "dark-grain" : "paper-grain").png")) {
            ctx.saveGState()
            ctx.clip(to: CGRect(x: 0, y: 0, width: width, height: height))
            ctx.setAlpha(darkSurface ? 0.24 : 0.32)
            ctx.draw(grain, in: CGRect(x: 0, y: 0, width: CGFloat(grain.width), height: CGFloat(grain.height)), byTiling: true)
            ctx.restoreGState()
        }

        // Outer frame + exact 9x10 grid + river + palace diagonals
        ctx.setStrokeColor(lineInk)
        ctx.setLineWidth(2.0)
        ctx.stroke(CGRect(x: ox - 5, y: oy - 5, width: 8 * s + 10, height: 9 * s + 10))

        ctx.setLineWidth(1.15)
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
        ctx.move(to: CGPoint(x: ox + 3 * s, y: oy)); ctx.addLine(to: CGPoint(x: ox + 5 * s, y: oy + 2 * s))
        ctx.move(to: CGPoint(x: ox + 5 * s, y: oy)); ctx.addLine(to: CGPoint(x: ox + 3 * s, y: oy + 2 * s))
        ctx.move(to: CGPoint(x: ox + 3 * s, y: oy + 7 * s)); ctx.addLine(to: CGPoint(x: ox + 5 * s, y: oy + 9 * s))
        ctx.move(to: CGPoint(x: ox + 5 * s, y: oy + 7 * s)); ctx.addLine(to: CGPoint(x: ox + 3 * s, y: oy + 9 * s))
        ctx.strokePath()

        // Cannon & soldier fiducial L-crosses
        let fiducials: [(Int, Int)] = [
            (1, 2), (7, 2), (0, 3), (2, 3), (4, 3), (6, 3), (8, 3),
            (1, 7), (7, 7), (0, 6), (2, 6), (4, 6), (6, 6), (8, 6),
        ]
        ctx.setLineWidth(1.1)
        let gap = max(2.5, s * 0.07)
        let arm = max(4.5, s * 0.15)
        for (fx, fy) in fiducials {
            let (cx, cy) = screenPoint(ox: ox, oy: oy, s: s, file: fx, rank: fy, redBelow: redBelow)
            let screenFile = redBelow ? fx : (8 - fx)
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                if (screenFile == 0 && dx < 0) || (screenFile == 8 && dx > 0) { continue }
                let gx = cx + CGFloat(dx) * gap
                let gy = cy + CGFloat(dy) * gap
                ctx.move(to: CGPoint(x: gx, y: gy + CGFloat(dy) * arm))
                ctx.addLine(to: CGPoint(x: gx, y: gy))
                ctx.addLine(to: CGPoint(x: gx + CGFloat(dx) * arm, y: gy))
            }
        }
        ctx.strokePath()

        // River inscriptions (`楚河` / `漢界` or `汉界`)
        let riverY = oy + 4.5 * s
        let leftText = "楚 河"
        let rightText = traditionalGlyphs ? "漢 界" : "汉 界"
        drawCenteredLabel(ctx: ctx, text: leftText, center: CGPoint(x: ox + 2.0 * s, y: riverY), fontSize: s * 0.44, color: riverInk, serif: true)
        drawCenteredLabel(ctx: ctx, text: rightText, center: CGPoint(x: ox + 6.0 * s, y: riverY), fontSize: s * 0.44, color: riverInk, serif: true)

        // Last-move corner brackets (a shape cue, never color alone) and the dashed check ring
        let half = pieceDiameter * 0.52
        let bracketArm = max(5, s * 0.16)
        ctx.setStrokeColor(markerInk)
        ctx.setLineWidth(2.0)
        for uciOpt in [lastMoveFromUci, lastMoveToUci] {
            guard let uci = uciOpt, let (f, r) = parseUci(uci) else { continue }
            let (mx, my) = screenPoint(ox: ox, oy: oy, s: s, file: f, rank: r, redBelow: redBelow)
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let cx = mx + CGFloat(dx) * half
                let cy = my + CGFloat(dy) * half
                ctx.move(to: CGPoint(x: cx - CGFloat(dx) * bracketArm, y: cy))
                ctx.addLine(to: CGPoint(x: cx, y: cy))
                ctx.addLine(to: CGPoint(x: cx, y: cy - CGFloat(dy) * bracketArm))
            }
        }
        ctx.strokePath()
        if let chk = checkUci, let (f, r) = parseUci(chk) {
            let (kx, ky) = screenPoint(ox: ox, oy: oy, s: s, file: f, rank: r, redBelow: redBelow)
            ctx.saveGState()
            ctx.setStrokeColor(vermilion)
            ctx.setLineWidth(2.6)
            ctx.setLineDash(phase: 0, lengths: [5, 3])
            let cr = pieceDiameter * 0.58
            ctx.strokeEllipse(in: CGRect(x: kx - cr, y: ky - cr, width: cr * 2, height: cr * 2))
            ctx.restoreGState()
        }

        let scriptDir = traditionalGlyphs ? "traditional" : "simplified"
        let shadowImg: CGImage? = repoRoot.flatMap { root in
            guard !root.isEmpty else { return nil }
            let url = URL(fileURLWithPath: root).appendingPathComponent("assets/board/materials/contact-shadow@2x.png")
            return loadCGImage(url: url)
        }

        let occupied = Set(pieces.map(\.uci))
        // The sprite box is one grid spacing (docs/rendering.md §4), as on the web.
        let spriteSpan = s
        for p in pieces {
            let (sx, sy) = screenPoint(ox: ox, oy: oy, s: s, file: Int(p.file), rank: Int(p.rank), redBelow: redBelow)
            let dstRect = CGRect(x: sx - spriteSpan / 2, y: sy - spriteSpan / 2, width: spriteSpan, height: spriteSpan)

            let cgSprite: CGImage? = repoRoot.flatMap { root in
                guard !root.isEmpty else { return nil }
                let spriteURL = URL(fileURLWithPath: root)
                    .appendingPathComponent("assets/board/materials/\(materialId)/\(scriptDir)/\(p.side)-\(p.role)@2x.png")
                return loadCGImage(url: spriteURL)
            }

            if let cgSprite {
                if let sh = shadowImg {
                    ctx.saveGState()
                    ctx.translateBy(x: dstRect.minX, y: dstRect.maxY)
                    ctx.scaleBy(x: 1, y: -1)
                    ctx.draw(sh, in: CGRect(origin: .zero, size: dstRect.size))
                    ctx.restoreGState()
                }
                ctx.saveGState()
                ctx.translateBy(x: dstRect.minX, y: dstRect.maxY)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(cgSprite, in: CGRect(origin: .zero, size: dstRect.size))
                ctx.restoreGState()
            } else {
                drawProceduralDisc(ctx: ctx, center: CGPoint(x: sx, y: sy), radius: s * 0.43, materialId: materialId, side: p.side, glyph: p.glyph)
            }

            if selectedUci == p.uci {
                ctx.setStrokeColor(markerInk)
                ctx.setLineWidth(2.5)
                let ringR = pieceDiameter * 0.54
                ctx.strokeEllipse(in: CGRect(x: sx - ringR, y: sy - ringR, width: ringR * 2, height: ringR * 2))
            }
        }

        // Legal destination indicators (dots for empty intersections, vermilion rings for captures)
        for dest in legalDestUcis {
            guard let (f, rVal) = parseUci(dest) else { continue }
            let (sx, sy) = screenPoint(ox: ox, oy: oy, s: s, file: f, rank: rVal, redBelow: redBelow)
            if occupied.contains(dest) {
                ctx.setStrokeColor(vermilion)
                ctx.setLineWidth(2.5)
                let cr = pieceDiameter * 0.53
                ctx.strokeEllipse(in: CGRect(x: sx - cr, y: sy - cr, width: cr * 2, height: cr * 2))
            } else {
                let pipColor = darkSurface
                    ? CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 0.85)
                    : CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 0.78)
                ctx.setFillColor(pipColor)
                let pr = max(4.5, s * 0.13)
                ctx.fillEllipse(in: CGRect(x: sx - pr, y: sy - pr, width: pr * 2, height: pr * 2))
            }
        }
    }

    public static func pieceAccessibilityRect(
        width: CGFloat,
        height: CGFloat,
        padding: CGFloat = 24.0,
        redBelow: Bool,
        file: Int,
        rank: Int
    ) -> CGRect {
        let s = min((width - 2.0 * padding) / 9.0, (height - 2.0 * padding) / 10.0)
        guard s > 0 else { return .zero }
        let ox = (width - 8.0 * s) / 2.0
        let oy = (height - 9.0 * s) / 2.0
        let (sx, sy) = screenPoint(ox: ox, oy: oy, s: s, file: file, rank: rank, redBelow: redBelow)
        let r = s * 0.45
        return CGRect(x: sx - r, y: sy - r, width: r * 2, height: r * 2)
    }

    private static func screenPoint(ox: CGFloat, oy: CGFloat, s: CGFloat, file: Int, rank: Int, redBelow: Bool) -> (CGFloat, CGFloat) {
        let sx = redBelow ? ox + CGFloat(file) * s : ox + CGFloat(8 - file) * s
        let sy = redBelow ? oy + CGFloat(9 - rank) * s : oy + CGFloat(rank) * s
        return (sx, sy)
    }

    private static func parseUci(_ uci: String) -> (Int, Int)? {
        guard uci.count == 2,
              let fAscii = uci.first?.asciiValue,
              fAscii >= 97 && fAscii <= 105,
              let rVal = Int(String(uci.last!)),
              (0...9).contains(rVal) else { return nil }
        return (Int(fAscii) - 97, rVal)
    }

    private static func drawProceduralDisc(
        ctx: CGContext,
        center: CGPoint,
        radius: CGFloat,
        materialId: String,
        side: String,
        glyph: String
    ) {
        let shadowColor = CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0.22)
        ctx.setFillColor(shadowColor)
        ctx.fillEllipse(in: CGRect(x: center.x - radius + 1.5, y: center.y - radius + 2.5, width: radius * 2, height: radius * 2))

        let discFill: CGColor
        switch materialId {
        case "boxwood":
            discFill = CGColor(srgbRed: 0xDF / 255.0, green: 0xC7 / 255.0, blue: 0x9C / 255.0, alpha: 1.0)
        case "ivory-stone":
            discFill = CGColor(srgbRed: 0xED / 255.0, green: 0xE7 / 255.0, blue: 0xDA / 255.0, alpha: 1.0)
        default:
            discFill = CGColor(srgbRed: 0xDC / 255.0, green: 0xE3 / 255.0, blue: 0xCE / 255.0, alpha: 1.0)
        }
        let ink = side == "red"
            ? CGColor(srgbRed: 0xA5 / 255.0, green: 0x48 / 255.0, blue: 0x37 / 255.0, alpha: 1.0)
            : CGColor(srgbRed: 0x25 / 255.0, green: 0x2A / 255.0, blue: 0x25 / 255.0, alpha: 1.0)
        let rim = CGColor(srgbRed: 0x41 / 255.0, green: 0x54 / 255.0, blue: 0x47 / 255.0, alpha: 0.9)

        ctx.setFillColor(discFill)
        ctx.fillEllipse(in: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
        ctx.setStrokeColor(rim)
        ctx.setLineWidth(1.6)
        ctx.strokeEllipse(in: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))

        let innerR = radius * 0.83
        ctx.setStrokeColor(ink.copy(alpha: 0.55) ?? ink)
        ctx.setLineWidth(1.1)
        ctx.strokeEllipse(in: CGRect(x: center.x - innerR, y: center.y - innerR, width: innerR * 2, height: innerR * 2))

        drawCenteredLabel(ctx: ctx, text: glyph, center: center, fontSize: radius * 1.12, color: ink)
    }

    private static func drawCenteredLabel(
        ctx: CGContext,
        text: String,
        center: CGPoint,
        fontSize: CGFloat,
        color: CGColor,
        serif: Bool = false
    ) {
        // The river inscription uses a Song serif, as the web board does; PingFang stands in where it is missing.
        var font = CTFontCreateWithName("PingFangSC-Semibold" as CFString, fontSize, nil)
        if serif {
            let song = CTFontCreateWithName("STSongti-SC-Bold" as CFString, fontSize, nil)
            if (CTFontCopyPostScriptName(song) as String) == "STSongti-SC-Bold" { font = song }
        }
        let attrs: [CFString: Any] = [
            kCTFontAttributeName: font,
            kCTForegroundColorAttributeName: color,
        ]
        guard let attrStr = CFAttributedStringCreate(nil, text as CFString, attrs as CFDictionary) else { return }
        let line = CTLineCreateWithAttributedString(attrStr)
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        var leading: CGFloat = 0
        let lineWidth = CGFloat(CTLineGetTypographicBounds(line, &ascent, &descent, &leading))

        ctx.saveGState()
        ctx.textMatrix = CGAffineTransform(scaleX: 1.0, y: -1.0)
        let tx = center.x - lineWidth / 2.0
        let ty = center.y + (ascent - descent) / 2.0
        ctx.textPosition = CGPoint(x: tx, y: ty)
        CTLineDraw(line, ctx)
        ctx.restoreGState()
    }

    private static func loadCGImage(url: URL) -> CGImage? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        return CGImageSourceCreateImageAtIndex(src, 0, nil)
    }
}
#endif

#if canImport(SQLite3)
import Foundation
import SQLite3

public final class AppleOfflineSqliteStore {
    private var db: OpaquePointer?

    public init(directory: String, filename: String) {
        try? FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: true)
        let path = (directory as NSString).appendingPathComponent(filename)
        if sqlite3_open(path, &db) == SQLITE_OK {
            sqlite3_exec(
                db,
                "CREATE TABLE IF NOT EXISTS offline_moves (seq INTEGER PRIMARY KEY AUTOINCREMENT, uci TEXT NOT NULL);",
                nil,
                nil,
                nil
            )
        }
    }

    deinit {
        if let db { sqlite3_close(db) }
    }

    public func loadMoves() -> [String] {
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

    public func appendMove(uci: String) {
        guard let db else { return }
        var stmt: OpaquePointer?
        if sqlite3_prepare_v2(db, "INSERT INTO offline_moves (uci) VALUES (?);", -1, &stmt, nil) == SQLITE_OK {
            sqlite3_bind_text(stmt, 1, (uci as NSString).utf8String, -1, nil)
            sqlite3_step(stmt)
        }
        sqlite3_finalize(stmt)
    }

    public func popLastMove() {
        guard let db else { return }
        sqlite3_exec(
            db,
            "DELETE FROM offline_moves WHERE seq = (SELECT MAX(seq) FROM offline_moves);",
            nil,
            nil,
            nil
        )
    }

    public func clearMoves() {
        guard let db else { return }
        sqlite3_exec(db, "DELETE FROM offline_moves;", nil, nil, nil)
    }
}
#endif
