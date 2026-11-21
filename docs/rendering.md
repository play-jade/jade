# Rendering, Geometry, and Asset Contract (`BoardView` v1)

This document defines the canonical board coordinate system, viewport projection, half-open pointer hit testing, `BoardView` v1 rendering contract, six-layer compositing order, alpha boundary rules, motion/accessibility invariants, and deterministic asset bundle budgets shared across Web, macOS, iOS, Android, Windows, and Linux clients in `play-jade/jade`.

---

## 1. Canonical Coordinates and Viewport Projection

### 1.1 Canonical Board Coordinates

All game state, move generation, notation, and `BoardView` payloads use Red-perspective canonical integer coordinates:

- **Files**: `x ∈ {0, 1, 2, 3, 4, 5, 6, 7, 8}` increasing left-to-right from Red's perspective (`x = 0` is Red's file 9 / left edge; `x = 8` is Red's file 1 / right edge).
- **Ranks**: `y ∈ {0, 1, 2, 3, 4, 5, 6, 7, 8, 9}` increasing from Red's home rank (`y = 0`) across the river (`y = 4` to `y = 5`) to Black's home rank (`y = 9`).

### 1.2 Viewport Transform

Given viewport dimensions `W, H` (in logical UI points / CSS pixels) and non-negative outer padding `p >= 0`:

```text
s  = min((W - 2*p) / 9, (H - 2*p) / 10)
ox = (W - 8*s) / 2
oy = (H - 9*s) / 2
```

- **Availability Guard**: If `s <= 0`, the board is unavailable for both drawing and pointer/touch input; adapters MUST skip rendering board geometry and return `None` for all hit tests.
- **Screen Intersection Projection**:
  - **Red below** (`orientation = "red-below"`):
    ```text
    screen(x, y) = (ox + s * x, oy + s * (9 - y))
    ```
  - **Black below** (`orientation = "black-below"`):
    ```text
    screen(x, y) = (ox + s * (8 - x), oy + s * y)
    ```

---

## 2. Half-Open Pointer Hit Testing (Never Clamp)

Given a pointer or touch location `(px, py)` in the same logical viewport coordinate space (when `s > 0`):

```text
u = (px - ox) / s
v = (py - oy) / s
```

1. **Half-Open Acceptance Window**: Accept **only** pointers satisfying both:
   ```text
   -0.5 <= u < 8.5   and   -0.5 <= v < 9.5
   ```
   Any point outside these half-open bounds MUST return `None` (no square). Renderers and input handlers MUST **never clamp** out-of-bounds margin clicks onto edge files or ranks.
2. **Grid Cell Quantization**:
   ```text
   c = floor(u + 0.5)    // integer in 0..=8
   r = floor(v + 0.5)    // integer in 0..=9
   ```
   Using half-open bounds `[-0.5, 8.5)` and `[-0.5, 9.5)` paired with `floor(t + 0.5)` partitions the interactive board plane cleanly with zero dead zones and deterministic tie-breaking along midpoint boundaries (`k + 0.5` maps to `k + 1`).
3. **Canonical Square Resolution**:
   - **Red below**: `(x, y) = (c, 9 - r)`
   - **Black below**: `(x, y) = (8 - c, r)`

---

## 3. `BoardView` v1 Presentation Contract

Every platform renderer consumes an immutable `BoardView` v1 snapshot derived from `crates/xiangqi-core` without embedding rules logic inside UI code:

| Field | Type | Description |
| :--- | :--- | :--- |
| `revision` | `u64` | Monotonic view sequence number used to reconcile transitions |
| `orientation` | `"red-below" \| "black-below"` | Controls intersection projection only; never rotates glyphs or lighting |
| `theme` | `"light" \| "dark"` | Selects the light board surface (`#EDE5D6` under `paper-grain.png`) or the dark one (`#222B27` under `dark-grain.png`) |
| `material_family` | `"pale-jade" \| "boxwood" \| "ivory-stone"` | Selects baked orthographic piece and audio material family |
| `glyph_set` | `"simplified-prototype-v1" \| "traditional-prototype-v1"` | Selects curated `zh-Hans` or `zh-Hant` Xiangqi piece faces |
| `pieces` | `Vec<PieceInstance>` | Stable piece `token_id`, `side` (`red`/`black`), `role`, and canonical `(x, y)` |
| `selected_square` | `Option<(u8, u8)>` | Currently selected friendly piece intersection |
| `legal_moves` | `Vec<LegalTarget>` | Destination `(x, y)` tagged `kind: "quiet" \| "capture"` |
| `last_move` | `Option<{ from: (u8, u8), to: (u8, u8) }>` | Previous ply origin and destination intersections |
| `checked_squares` | `Vec<(u8, u8)>` | Square(s) of any General currently in check |
| `focused_square` | `Option<(u8, u8)>` | Roving keyboard / assistive-technology grid focus cursor |
| `transition` | `Option<MoveTransition>` | Optional active piece motion (`token_id`, `from`, `to`, `started_at_ms`) |
| `drag` | `Option<DragOverlay>` | Active pointer drag (`token_id`, logical pointer `(px, py)`, hover target) |
| `annotations` | `Vec<BoardAnnotation>` | Study/review arrows and square callouts |

---

## 4. Six-Layer Compositing Order

All platform renderers MUST draw strictly in the following bottom-to-top order so shadows, pieces, and non-color state markers never occlude one another erroneously:

1. **Layer 1 — Board Surface & Exact Markings**:
   - Board surface fill `#EDE5D6` (light) or `#222B27` (dark), with the seamless `paper-grain.png` or `dark-grain.png` tile laid over it at `32%` or `24%` opacity. The page around the board keeps the palette grounds `#F3EEE4` and `#1D2421`.
   - Outer frame, 10 horizontal rank lines (`y = 0..9`), continuous outer vertical boundary files (`x = 0` and `x = 8` crossing the river), split interior files (`x = 1..7` stopping at `y = 4` and resuming at `y = 5`), palace diagonals (`(3,0)–(5,2)`, `(5,0)–(3,2)`, `(3,7)–(5,9)`, `(5,7)–(3,9)`), and L-shaped cannon/soldier fiducial corner ticks (`(1,2)`, `(7,2)`, `(1,7)`, `(7,7)` and `x ∈ {0,2,4,6,8}` on `y ∈ {3,6}`, omitting outward arms on `x = 0` and `x = 8`).
2. **Layer 2 — Under-Piece State Indicators**:
   - Last-move corner brackets at both the origin and the destination: four L-shaped corners at `0.52 × d` from the intersection, arms `max(5, 0.16 * s)` long, `2` px, deep jade `#415447` (light) or pale jade `#DCE3CE` (dark), where `d = 0.88 * s` is the piece diameter.
   - Check indicator: a dashed `#A54837` vermilion ring of radius `0.58 * d`, `2.6` px, dashes `5` on `3` off, around the checked General.
   - River inscription `楚 河` and `汉 界` (`漢 界` with traditional faces) in a Song serif (Songti SC Bold on Apple, Noto Serif CJK SC on the web) at `0.44 * s`, deep jade at `68%` (light) or pale jade at `56%` (dark).
3. **Layer 3 — Contact Shadows**:
   - `contact-shadow.png` (`@1x`) / `contact-shadow@2x.png` (`@2x`) drawn at `box_size = s` centered on each stationary piece's projected intersection. All stationary shadows render in Layer 3 before any piece disc in Layer 4 so a shadow never darkens a neighboring piece's carved top surface.
4. **Layer 4 — Stationary Pieces**:
   - Baked orthographic piece sprites (`96×96` logical box scaled to `s × s`, giving visible disc diameter `0.86 * s`, squarely within the `82%–88%` spacing specification).
5. **Layer 5 — Moving or Dragged Piece**:
   - Active animated or pointer-dragged piece rendered above all stationary pieces with an elevated, slightly softer shadow offset (`1.6×` standard shadow offset) and subtle `1.04×` lift scale (suppressed when reduced motion is active).
6. **Layer 6 — Foreground Focus, Selection & Annotation Overlays**:
   - Selected piece ring: radius `0.54 * d`, `2.5` px, deep jade (light) or pale jade (dark).
   - Quiet legal-move destination dots: radius `max(4.5, 0.13 * s)`, deep jade at `78%` (light) or pale jade at `85%` (dark).
   - Legal capture target ring: radius `0.53 * d`, `2.5` px, vermilion.
   - Keyboard / switch-control focus ring and optional study review arrows.

---

## 5. Orientation Flip & Studio Lighting Invariant

Flipping the board between `red-below` and `black-below` updates **only** the board-to-screen intersection coordinate mapping (`Section 1.2`).

- **Upright Piece Faces**: Piece sprites MUST NEVER be rotated `180°` when `black-below` is active. Every Chinese character (`帅/帥`, `将/將`, `炮/砲`, etc.) remains upright and legible from the viewer's seat.
- **Fixed Studio Key Light & Camera**: All three baked material families share an identical top-down orthographic camera and fixed upper-left studio key light (`azimuth = -45°`, `elevation = 52°`, ambient `0.46`). Upper-left bevel highlights, upper-left recessed engraving cut-wall shadows, lower-right stroke-lip catchlights, and lower-right contact shadows (`offset = (+0.021*s, +0.029*s)`) stay fixed in screen space across orientation flips.

---

## 6. Color Space and Straight vs. Premultiplied Alpha Boundary

### 6.1 Palette Tokens (`IEC 61966-2-1 sRGB`)

| Token | Hex | Primary Rendering Role | Contrast Ratio |
| :--- | :--- | :--- | :--- |
| `pale-jade` | `#DCE3CE` | Flagship nephrite disc body & dark-mode board linework | `#252A25` on `#DCE3CE`: `11.08:1` |
| `deep-jade` | `#415447` | Rim depth accent, selection ring, legal-move dots | `#DCE3CE` on `#415447`: `6.16:1` |
| `charcoal` | `#252A25` | Black-side recessed engraving & light-mode grid lines | `#252A25` on `#F3EEE4`: `12.64:1` |
| `vermilion` | `#A54837` | Red-side recessed engraving, capture brackets, check halo | `#A54837` on `#F3EEE4`: `5.07:1` (`4.45:1` on `#DCE3CE` — bold graphic strokes) |
| `warm-paper` | `#F3EEE4` | Light page ground; text on dark grounds | Primary light ground |
| `dark-ground` | `#1D2421` | Dark page ground around the board | `#F3EEE4` on `#1D2421`: `13.69:1` |

*(Boxwood `#DFC496` pairs `#99392A` deep vermilion with `#252A25` charcoal; Ivory Stone `#EDE6D6` pairs `#A54837` vermilion with `#252A25` charcoal.)*

### 6.2 Straight-Alpha Storage & Premultiplied Adapter Boundary

1. **On-Disk Asset Convention (`straight` alpha)**:
   - Every generated PNG in `assets/board/` and `docs/visual-reference/` is encoded as 8-bit `sRGB` `RGBA` with **`alpha_convention = "straight"`**.
   - Partially transparent anti-aliased disc boundary pixels (`0 < alpha < 255`) carry pure illuminated stone/wood rim RGB unpolluted by background black or white; fully transparent exterior pixels (`alpha == 0`) are strictly `[0, 0, 0, 0]`.
2. **Renderer Upload Boundary (`straight -> premultiplied`)**:
   - Platform graphics pipelines that perform bilinear/trilinear texture filtering, mipmap box averaging, or Porter-Duff `source-over` compositing MUST convert straight RGBA to **premultiplied alpha** (`(R*A, G*A, B*A, A)`) **once at decode / texture-upload time**:
     - **Web (`WebGL2` / `Canvas2D`)**: Set `gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true)` or `createImageBitmap(blob, { premultiplyAlpha: "premultiply", colorSpaceConversion: "default" })`.
     - **Apple (`Core Graphics` / `Metal`)**: Decode via `CGImage` / `MTKTextureLoader` into premultiplied sRGB (`CGImageAlphaInfo.premultipliedLast` / `MTLPixelFormatRGBA8Unorm_sRGB`).
     - **Android / Compose / Skia**: Decode with `AlphaType.PREMUL` (`BitmapFactory.Options.inPremultiplied = true`).
     - **Windows (`Win2D` / `Direct2D`)**: Upload with `D2D1_ALPHA_MODE_PREMULTIPLIED` in `DXGI_FORMAT_R8G8B8A8_UNORM_SRGB`.
     - **Rust (`tiny-skia`)**: Convert straight pixels to premultiplied before filtering (`crates/asset-pipeline/src/gallery.rs` `SpriteImage::from_png_path`).

---

## 7. Motion Timing, Transition Reconciliation, and Reduced Motion

- **Standard Move Envelope**: Piece placement lift/settle transitions run for **`120–180 ms`** (canonical default **`150 ms`**, cubic ease-out) accompanied at landing/onset by the material-matched modal percussion sample (`place-1.wav` / `place-2.wav` alternating on quiet moves, `capture.wav` on captures, `check.wav` when delivering check).
- **Deterministic Reconciliation**: `BoardView.revision` is authoritative. If a new revision arrives while an animation is still in progress (rapid replay scrubbing, premove execution, takeback, or network state sync), the renderer MUST immediately settle the prior transition to its canonical intersection and animate at most the newest single-move delta—never accumulating an animation queue behind authoritative game state.
- **Reduced Motion**: When `prefers-reduced-motion: reduce` (Web), `NSWorkspace.accessibilityDisplayShouldReduceMotion` / `UIAccessibility.isReduceMotionEnabled` (Apple), `ANIMATOR_DURATION_SCALE == 0` (Android), `UISettings.AnimationsEnabled == false` (Windows), or `gtk-enable-animations == false` (Linux) is active:
  - Suppress continuous piece translation, drag-lift scaling, and shadow elevation.
  - Place pieces instantaneously at their destination intersections while **preserving** selection rings, legal-move indicators, last-move corner brackets, check halos, and focus indicators.

---

## 8. WCAG 2.2 Non-Color State Distinction & Semantic Accessibility Tree

1. **Non-Color-Only State Geometry (WCAG 2.2 SC 1.4.1 & 1.4.11)**:
   - **Piece Side**: Distinguished simultaneously by vermilion vs. charcoal ink AND side-specific traditional/simplified ideographs (`帅/帥` vs. `将/將`, `仕` vs. `士`, `相` vs. `象`, `兵` vs. `卒`, and Traditional `炮` vs. `砲`).
   - **Selected Piece**: Continuous solid ring around the piece.
   - **Quiet Legal Destination**: Small centered filled disc on an empty intersection.
   - **Legal Capture Target**: Vermilion ring around the capturable piece; it marks an occupied point, where a destination dot marks an empty one.
   - **Last Move**: Corner brackets at the origin and the destination.
   - **Check**: Dashed ring around the checked General, told from the solid selection ring by its dashes.
2. **Parallel Native Semantic Accessibility Tree & Keyboard Control**:
   - Canvas/GPU board surfaces MUST expose a synchronized `9 × 10` semantic grid (`role="grid"` on Web, `NSAccessibilityGridRole` / `UIAccessibilityContainer` on Apple, Compose `Semantics` on Android) ordered consistently with the active orientation.
   - Every cell exposes localized coordinates, occupant side/role, and state (`selected`, `in check`, `legal move`, `legal capture`).
   - **Keyboard Navigation**: Arrow keys move `focused_square` across `x = 0..8, y = 0..9` (`2px+` high-contrast focus ring meeting WCAG 2.2 SC 2.4.11/2.4.13); `Enter` or `Space` selects a friendly piece or executes a legal move to `focused_square`; `Escape` cancels selection. Move arrivals, captures, check, and terminal adjudication emit polite live-region announcements.

---

## 9. Deterministic Asset Pipeline & Bundle Budgets

All board vectors, 2.5D baked sprites, modal percussion WAV samples, visual reference plates, and SHA-256 manifest records are generated and verified in pure Rust via `crates/asset-pipeline`:

```sh
# Fresh public checkout verification (requires zero external font files or network access):
cargo run --release -p jade-asset-pipeline -- --verify

# Full regeneration from pinned SIL OFL 1.1 Noto Serif CJK SC Bold 2.003 source font:
cargo run --release -p jade-asset-pipeline -- --font-path /path/to/NotoSerifCJKsc-Bold.otf
```

### 9.1 Curated Glyph Sets (`assets/board/glyphs.json`)

- **`simplified-prototype-v1` (`zh-Hans`)**: Red `帅 仕 相 车 马 炮 兵` · Black `将 士 象 车 马 炮 卒`
- **`traditional-prototype-v1` (`zh-Hant`)**: Red `帥 仕 相 車 馬 炮 兵` · Black `將 士 象 車 馬 砲 卒` *(curated Xiangqi faces; `將` `U+5C07` for Black General and stone-radical `砲` `U+7832` for Black Cannon)*
- **Provenance & Review Status**: Extracted from `Noto Serif CJK SC Bold 2.003` (`SIL-OFL-1.1`, `SHA-256: 7ed2e04e7b96734dd67599be8ff3e625aba23c5c550a489070deb8a38a422568`) and marked `"prototype-licensed-fallback-pending-native-calligraphy-review"`.

### 9.2 Verified Web Material Bundle Byte Budgets (`< 1,000,000` Bytes)

Each single-script web material bundle consists of 17 files: all 14 baked piece sprites (`96×96` `@1x` or `192×192` `@2x`), the matching `contact-shadow` sprite, `paper-grain.png` (`256×256`), and `dark-grain.png` (`256×256`):

| Bundle ID | Material Family | Glyph Set | `@1x` Bytes | `@2x` Bytes | Budget Ceiling |
| :--- | :--- | :--- | ---: | ---: | ---: |
| `pale-jade.simplified` | `pale-jade` (Flagship) | `simplified-prototype-v1` | `139,207` B | `359,286` B | `1,000,000` B |
| `pale-jade.traditional` | `pale-jade` (Flagship) | `traditional-prototype-v1` | `140,666` B | `361,963` B | `1,000,000` B |
| `boxwood.simplified` | `boxwood` | `simplified-prototype-v1` | `147,621` B | `406,674` B | `1,000,000` B |
| `boxwood.traditional` | `boxwood` | `traditional-prototype-v1` | `148,901` B | `407,222` B | `1,000,000` B |
| `ivory-stone.simplified` | `ivory-stone` | `simplified-prototype-v1` | `129,503` B | `335,313` B | `1,000,000` B |
| `ivory-stone.traditional` | `ivory-stone` | `traditional-prototype-v1` | `130,950` B | `337,848` B | `1,000,000` B |

### 9.3 Acoustic Percussion Samples (`assets/audio/`)

Synthesized deterministically at `44,100 Hz` 16-bit mono PCM WAV with Hertzian contact pulse, surface micro-roughness burst, coupled wooden-board cavity resonance, and inharmonic circular-plate modes (`place-1.wav` `82 ms`, `place-2.wav` `85 ms`, `capture.wav` `112 ms`, `check.wav` `120 ms`) across `assets/audio/jade/`, `assets/audio/boxwood/`, and `assets/audio/ivory-stone/`.

### 9.4 Visual Reference Plates (`docs/visual-reference/`)

- [`glyph-contact-sheet.png`](visual-reference/glyph-contact-sheet.png) — Complete 14-face Simplified and Traditional contact sheet across `pale-jade`, `boxwood`, and `ivory-stone` on light and dark grounds.
- [`board-opening-light-dark.png`](visual-reference/board-opening-light-dark.png) — Full 32-piece `9 × 10` opening board in Light (`#F3EEE4`) and Dark (`#1D2421`) presentations across both `red-below` and `black-below` orientations.
- [`board-crowded-phone-360px.png`](visual-reference/board-crowded-phone-360px.png) — Crowded middlegame rendered at the smallest supported mobile viewport (`360 × 420` px) demonstrating stroke legibility and non-color-only state markers.
