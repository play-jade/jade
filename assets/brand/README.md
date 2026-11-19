# Jade Brand Identity and Asset Provenance

Jade is the English product identity for `play-jade/jade`. The current owner-used Chinese display is **玉枰象棋** (`yùpíng xiàngqí`), shown alongside `Jade — Xiangqi`. `玉枰` is an attested literary expression for a jade game board (`枰`: game board) and carries no claim of a specialized historical Xiangqi rule tradition. Regional wording and native lettering across `zh-Hans-CN`, `zh-Hant-TW`, and `zh-Hant-HK` remain scheduled for native-reader production review.

**象** (`U+8C61`) is the signet character connecting the mark to Xiangqi and its elephant piece. It is separate from the Chinese product name (`玉枰象棋`) and separate from the fourteen gameplay piece faces used on the playable board.

## Existing-Use Note

During identity review, **Jade River Chinese Chess** (Airscape Works) was recorded as an adjacent Xiangqi product name. The GitHub organization is finalized as `play-jade` (`play-jade/jade` public, `play-jade/cinnabar` private). Primary web domain selection and permanent native app bundle/application identifiers remain separate configuration values.

## Palette (`sRGB`)

| Token | Hex | Role | Contrast Notes |
| :--- | :--- | :--- | :--- |
| `pale-jade` | `#DCE3CE` | Signature disc and soft material highlights | `charcoal` on `pale-jade`: `11.08:1`; `vermilion` on `pale-jade`: `4.45:1` (reserve for large graphic accents only) |
| `deep-jade` | `#415447` | Supporting brand color, selected accents, dark green surfaces | `pale-jade` on `deep-jade`: `6.16:1` |
| `charcoal` (Ink) | `#252A25` | Charcoal engraving and primary text on light surfaces | `charcoal` on `warm-paper`: `12.64:1` |
| `vermilion` | `#A54837` | Avatar field, restrained accents, red-side material direction | `vermilion` on `warm-paper`: `5.07:1` |
| `warm-paper` | `#F3EEE4` | Warm interface and editorial surfaces | Primary light ground |
| `dark-ground` | `#1D2421` | Dark presentation background and supporting surfaces | `warm-paper` on `dark-ground`: `13.69:1` |

Machine-readable tokens live in [`palette.json`](palette.json) and [`palette.css`](palette.css).

## Canonical Brand Files

- [`avatar-master.png`](avatar-master.png) — `1254 × 1254` RGB pale-jade disc on vermilion material master (`SHA-256: 01a1fafddc2c4b658d2a9d15e32f5416156824ab41216567b1ac793366bba150`). Companion dark-jade and warm-paper masters are stored at [`avatar-dark-master.png`](avatar-dark-master.png) and [`avatar-paper-master.png`](avatar-paper-master.png). The engraved character in the generated material raster is an identity study, not a production-verified gameplay glyph.
- [`wordmark.svg`](wordmark.svg) and [`wordmark-light.svg`](wordmark-light.svg) — Titlecase serif `Jade` wordmark outlined from **Noto Serif Display Regular 2.003** (SIL Open Font License 1.1) with source kerning plus `+0.012 em` tracking. Both SVGs contain pure vector paths with no live text dependencies. Maintain clear space of at least `0.25 ×` capital `J` height on all sides; minimum recommended width is `96 px` (use the signet below `96 px`).
- [`mark.svg`](mark.svg) and [`mark-on-vermilion.svg`](mark-on-vermilion.svg) — Independent flat vector signet prototype constructed from **Noto Serif CJK SC Bold 2.003** (`© 2017–2024 Adobe`, SIL Open Font License 1.1) glyph `U+8C61` (`象`) on a geometric disc (`SHA-256` recorded in [`source/font_sources.json`](source/font_sources.json)). **Review status**: prototype fallback; native-language calligraphy review remains a required production step before marking custom calligraphy final.
- [`exports/`](exports/) — Square `500 × 500` GitHub organization PNG exports verified below `1,000,000` bytes (`Jade_Organization_Avatar.png`: `289,864` bytes), `256 × 256`, `128 × 128`, `64 × 64`, and `32 × 32` size exports, plus circular and rounded-square crop proofs and [`exports/Avatar_Exports.json`](exports/Avatar_Exports.json).
- [`proofs/`](proofs/) — Multi-size pixel and nearest-neighbor inspection sheets across light and dark grounds.
- [`source/`](source/) — Generation prompt ([`source/jade_avatar_prompt.txt`](source/jade_avatar_prompt.txt)), variant provenance ([`source/jade_avatar_variants.json`](source/jade_avatar_variants.json)), wordmark geometry ([`source/wordmark_geometry.json`](source/wordmark_geometry.json)), font SHA-256 provenance ([`source/font_sources.json`](source/font_sources.json)), SIL OFL 1.1 license texts ([`source/licenses/`](source/licenses/)), and upstream brand kit checksum manifest ([`source/upstream_kit_manifest.json`](source/upstream_kit_manifest.json)). Note: `NotoSerifCJKsc-Bold.otf` (`25,521,436` bytes, `SHA-256: 7ed2e04e7b96734dd67599be8ff3e625aba23c5c550a489070deb8a38a422568`) is referenced by checksum rather than checked into ordinary Git source; compiled SVG glyph paths are verified in Rust via `crates/asset-pipeline`.
