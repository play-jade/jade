//! Xiangqi piece-face vector glyph extraction, optical normalization, SVG serialization,
//! and `tiny-skia` path reconstruction.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tiny_skia::{PathBuilder, Transform};
use ttf_parser::{Face, OutlineBuilder};

use crate::PipelineError;

pub const EXPECTED_FONT_SHA256: &str =
    "7ed2e04e7b96734dd67599be8ff3e625aba23c5c550a489070deb8a38a422568";
pub const FONT_SOURCE_NAME: &str = "Noto Serif CJK SC Bold 2.003";
pub const FONT_LICENSE_ID: &str = "SIL-OFL-1.1";
pub const GLYPH_REVIEW_STATUS: &str =
    "prototype-licensed-fallback-pending-native-calligraphy-review";
pub const VIEWBOX_SIZE: f64 = 1000.0;
pub const UNIFORM_EM_TARGET: f64 = 820.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlyphSetId {
    SimplifiedPrototypeV1,
    TraditionalPrototypeV1,
}

impl GlyphSetId {
    pub const ALL: [GlyphSetId; 2] = [
        GlyphSetId::SimplifiedPrototypeV1,
        GlyphSetId::TraditionalPrototypeV1,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            GlyphSetId::SimplifiedPrototypeV1 => "simplified-prototype-v1",
            GlyphSetId::TraditionalPrototypeV1 => "traditional-prototype-v1",
        }
    }

    pub fn short_dir(self) -> &'static str {
        match self {
            GlyphSetId::SimplifiedPrototypeV1 => "simplified",
            GlyphSetId::TraditionalPrototypeV1 => "traditional",
        }
    }

    pub fn locale_tag(self) -> &'static str {
        match self {
            GlyphSetId::SimplifiedPrototypeV1 => "zh-Hans",
            GlyphSetId::TraditionalPrototypeV1 => "zh-Hant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PieceSide {
    Red,
    Black,
}

impl PieceSide {
    pub const ALL: [PieceSide; 2] = [PieceSide::Red, PieceSide::Black];

    pub fn as_str(self) -> &'static str {
        match self {
            PieceSide::Red => "red",
            PieceSide::Black => "black",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PieceRole {
    General,
    Advisor,
    Elephant,
    Chariot,
    Horse,
    Cannon,
    Soldier,
}

impl PieceRole {
    pub const ALL: [PieceRole; 7] = [
        PieceRole::General,
        PieceRole::Advisor,
        PieceRole::Elephant,
        PieceRole::Chariot,
        PieceRole::Horse,
        PieceRole::Cannon,
        PieceRole::Soldier,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            PieceRole::General => "general",
            PieceRole::Advisor => "advisor",
            PieceRole::Elephant => "elephant",
            PieceRole::Chariot => "chariot",
            PieceRole::Horse => "horse",
            PieceRole::Cannon => "cannon",
            PieceRole::Soldier => "soldier",
        }
    }

    pub fn seed_index(self) -> u64 {
        match self {
            PieceRole::General => 1,
            PieceRole::Advisor => 2,
            PieceRole::Elephant => 3,
            PieceRole::Chariot => 4,
            PieceRole::Horse => 5,
            PieceRole::Cannon => 6,
            PieceRole::Soldier => 7,
        }
    }
}

/// Returns the curated Xiangqi character for `(set_id, side, role)`.
///
/// Traditional faces are curated per traditional Xiangqi set conventions
/// (e.g. Black General `將` `U+5C07`, Black Cannon `砲` `U+7832` vs Red Cannon `炮` `U+70AE`),
/// never produced by mechanical character-table conversion.
pub fn face_character(set_id: GlyphSetId, side: PieceSide, role: PieceRole) -> char {
    match (set_id, side, role) {
        // Simplified (`zh-Hans`)
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::General) => '帅',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Advisor) => '仕',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Elephant) => '相',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Chariot) => '车',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Horse) => '马',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Cannon) => '炮',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Red, PieceRole::Soldier) => '兵',

        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::General) => '将',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Advisor) => '士',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Elephant) => '象',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Chariot) => '车',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Horse) => '马',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Cannon) => '炮',
        (GlyphSetId::SimplifiedPrototypeV1, PieceSide::Black, PieceRole::Soldier) => '卒',

        // Traditional (`zh-Hant`)
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::General) => '帥',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Advisor) => '仕',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Elephant) => '相',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Chariot) => '車',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Horse) => '馬',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Cannon) => '炮',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Red, PieceRole::Soldier) => '兵',

        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::General) => '將',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Advisor) => '士',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Elephant) => '象',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Chariot) => '車',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Horse) => '馬',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Cannon) => '砲',
        (GlyphSetId::TraditionalPrototypeV1, PieceSide::Black, PieceRole::Soldier) => '卒',
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphBounds {
    pub view_box: [f64; 4],
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
    pub width: f64,
    pub height: f64,
    pub font_units_per_em: u16,
    pub font_bbox: [i16; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontSourceMeta {
    pub name: String,
    pub license: String,
    pub copyright: String,
    pub font_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphEntry {
    pub glyph_id: String,
    pub set_id: String,
    pub locale: String,
    pub side: PieceSide,
    pub role: PieceRole,
    pub character: String,
    pub codepoint: String,
    pub svg_path: String,
    pub svg_file: String,
    pub bounds: GlyphBounds,
    pub font_source: FontSourceMeta,
    pub review_status: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentedVariant {
    pub variant_id: String,
    pub applies_to_set: String,
    pub side: PieceSide,
    pub role: PieceRole,
    pub character: String,
    pub codepoint: String,
    pub svg_file: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphManifest {
    pub schema_version: String,
    pub view_box: [u32; 4],
    pub y_axis: String,
    pub optical_centering: String,
    pub sets: Vec<String>,
    pub documented_variants: Vec<DocumentedVariant>,
    pub glyphs: Vec<GlyphEntry>,
}

impl GlyphManifest {
    pub fn find(
        &self,
        set_id: GlyphSetId,
        side: PieceSide,
        role: PieceRole,
    ) -> Option<&GlyphEntry> {
        let target = set_id.as_str();
        self.glyphs
            .iter()
            .find(|g| g.set_id == target && g.side == side && g.role == role)
    }
}

#[derive(Debug, Clone, Copy)]
enum RawCmd {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    QuadTo(f64, f64, f64, f64),
    CurveTo(f64, f64, f64, f64, f64, f64),
    Close,
}

#[derive(Default)]
struct RawOutlineCollector {
    cmds: Vec<RawCmd>,
}

impl OutlineBuilder for RawOutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cmds.push(RawCmd::MoveTo(x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.cmds.push(RawCmd::LineTo(x as f64, y as f64));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.cmds
            .push(RawCmd::QuadTo(x1 as f64, y1 as f64, x as f64, y as f64));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.cmds.push(RawCmd::CurveTo(
            x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64, y as f64,
        ));
    }

    fn close(&mut self) {
        self.cmds.push(RawCmd::Close);
    }
}

#[inline]
fn round2(v: f64) -> f64 {
    let r = (v * 100.0).round() / 100.0;
    if r == -0.0 { 0.0 } else { r }
}

fn fmt_coord(v: f64) -> String {
    format!("{:.2}", round2(v))
}

struct ExtractedCodepoint {
    codepoint_str: String,
    svg_rel_path: String,
    svg_path_d: String,
    svg_sha256: String,
    bounds: GlyphBounds,
}

fn render_glyph_svg(ch: char, codepoint_str: &str, path_d: &str) -> String {
    format!(
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1000 1000\" width=\"1000\" height=\"1000\" role=\"img\" aria-label=\"{ch} ({cp})\">\n",
            "  <title>Xiangqi piece glyph {ch} ({cp})</title>\n",
            "  <desc>Normalized Y-down vector outline extracted from {font} ({lic}); prototype fallback pending native calligraphy review.</desc>\n",
            "  <path fill=\"#252A25\" d=\"{d}\"/>\n",
            "</svg>\n"
        ),
        ch = ch,
        cp = codepoint_str,
        font = FONT_SOURCE_NAME,
        lic = FONT_LICENSE_ID,
        d = path_d,
    )
}

pub fn extract_glyphs_from_font(
    font_path: &Path,
    repo_root: &Path,
) -> Result<GlyphManifest, PipelineError> {
    let font_bytes = fs::read(font_path).map_err(|e| {
        PipelineError::Validation(format!("failed to read font {}: {e}", font_path.display()))
    })?;

    let actual_font_sha = hex::encode(Sha256::digest(&font_bytes));
    if actual_font_sha != EXPECTED_FONT_SHA256 {
        return Err(PipelineError::Validation(format!(
            "font SHA-256 mismatch for {}: expected {EXPECTED_FONT_SHA256}, got {actual_font_sha}",
            font_path.display()
        )));
    }

    let face = Face::parse(&font_bytes, 0).map_err(|e| {
        PipelineError::Validation(format!("ttf-parser failed to parse font: {e:?}"))
    })?;
    let units_per_em = face.units_per_em();
    let scale = UNIFORM_EM_TARGET / (units_per_em as f64);

    let glyphs_dir = repo_root.join("assets/board/glyphs");
    fs::create_dir_all(&glyphs_dir)?;

    let mut extracted_by_char: BTreeMap<char, ExtractedCodepoint> = BTreeMap::new();

    for set_id in GlyphSetId::ALL {
        for side in PieceSide::ALL {
            for role in PieceRole::ALL {
                let ch = face_character(set_id, side, role);
                if extracted_by_char.contains_key(&ch) {
                    continue;
                }

                let gid = face.glyph_index(ch).ok_or_else(|| {
                    PipelineError::Validation(format!(
                        "character '{ch}' (U+{:04X}) missing from font",
                        ch as u32
                    ))
                })?;

                let mut collector = RawOutlineCollector::default();
                let rect = face.outline_glyph(gid, &mut collector).ok_or_else(|| {
                    PipelineError::Validation(format!(
                        "character '{ch}' (U+{:04X}) produced empty outline",
                        ch as u32
                    ))
                })?;

                if collector.cmds.is_empty() {
                    return Err(PipelineError::Validation(format!(
                        "character '{ch}' outline commands empty"
                    )));
                }

                let cx = 0.5 * ((rect.x_min as f64) + (rect.x_max as f64));
                let cy = 0.5 * ((rect.y_min as f64) + (rect.y_max as f64));

                let tx = |fx: f64| -> f64 { round2(500.0 + (fx - cx) * scale) };
                // Font Y+ points up; SVG/screen Y+ points down.
                let ty = |fy: f64| -> f64 { round2(500.0 - (fy - cy) * scale) };

                let mut d = String::with_capacity(collector.cmds.len() * 20);
                for (idx, cmd) in collector.cmds.iter().enumerate() {
                    if idx > 0 {
                        d.push(' ');
                    }
                    match *cmd {
                        RawCmd::MoveTo(x, y) => {
                            let _ = write!(d, "M{} {}", fmt_coord(tx(x)), fmt_coord(ty(y)));
                        }
                        RawCmd::LineTo(x, y) => {
                            let _ = write!(d, "L{} {}", fmt_coord(tx(x)), fmt_coord(ty(y)));
                        }
                        RawCmd::QuadTo(x1, y1, x, y) => {
                            let _ = write!(
                                d,
                                "Q{} {} {} {}",
                                fmt_coord(tx(x1)),
                                fmt_coord(ty(y1)),
                                fmt_coord(tx(x)),
                                fmt_coord(ty(y))
                            );
                        }
                        RawCmd::CurveTo(x1, y1, x2, y2, x, y) => {
                            let _ = write!(
                                d,
                                "C{} {} {} {} {} {}",
                                fmt_coord(tx(x1)),
                                fmt_coord(ty(y1)),
                                fmt_coord(tx(x2)),
                                fmt_coord(ty(y2)),
                                fmt_coord(tx(x)),
                                fmt_coord(ty(y))
                            );
                        }
                        RawCmd::Close => {
                            d.push('Z');
                        }
                    }
                }

                let x_min = round2(500.0 + ((rect.x_min as f64) - cx) * scale);
                let x_max = round2(500.0 + ((rect.x_max as f64) - cx) * scale);
                let y_min = round2(500.0 - ((rect.y_max as f64) - cy) * scale);
                let y_max = round2(500.0 - ((rect.y_min as f64) - cy) * scale);

                let bounds = GlyphBounds {
                    view_box: [0.0, 0.0, VIEWBOX_SIZE, VIEWBOX_SIZE],
                    x_min,
                    y_min,
                    x_max,
                    y_max,
                    width: round2(x_max - x_min),
                    height: round2(y_max - y_min),
                    font_units_per_em: units_per_em,
                    font_bbox: [rect.x_min, rect.y_min, rect.x_max, rect.y_max],
                };

                let cp_hex = format!("{:04X}", ch as u32);
                let codepoint_str = format!("U+{cp_hex}");
                let file_name = format!("u{}.svg", cp_hex.to_ascii_lowercase());
                let svg_rel_path = format!("assets/board/glyphs/{file_name}");
                let svg_abs_path = glyphs_dir.join(&file_name);

                let svg_xml = render_glyph_svg(ch, &codepoint_str, &d);
                let svg_sha256 = hex::encode(Sha256::digest(svg_xml.as_bytes()));
                fs::write(&svg_abs_path, svg_xml.as_bytes())?;

                extracted_by_char.insert(
                    ch,
                    ExtractedCodepoint {
                        codepoint_str,
                        svg_rel_path,
                        svg_path_d: d,
                        svg_sha256,
                        bounds,
                    },
                );
            }
        }
    }

    let font_meta = FontSourceMeta {
        name: FONT_SOURCE_NAME.to_string(),
        license: FONT_LICENSE_ID.to_string(),
        copyright: "© 2017-2024 Adobe (http://www.adobe.com/).".to_string(),
        font_sha256: EXPECTED_FONT_SHA256.to_string(),
    };

    let mut glyphs = Vec::with_capacity(28);
    for set_id in GlyphSetId::ALL {
        for side in PieceSide::ALL {
            for role in PieceRole::ALL {
                let ch = face_character(set_id, side, role);
                let extracted = extracted_by_char.get(&ch).expect("extracted above");
                glyphs.push(GlyphEntry {
                    glyph_id: format!("{}.{}.{}", set_id.as_str(), side.as_str(), role.as_str()),
                    set_id: set_id.as_str().to_string(),
                    locale: set_id.locale_tag().to_string(),
                    side,
                    role,
                    character: ch.to_string(),
                    codepoint: extracted.codepoint_str.clone(),
                    svg_path: extracted.svg_path_d.clone(),
                    svg_file: extracted.svg_rel_path.clone(),
                    bounds: extracted.bounds.clone(),
                    font_source: font_meta.clone(),
                    review_status: GLYPH_REVIEW_STATUS.to_string(),
                    sha256: extracted.svg_sha256.clone(),
                });
            }
        }
    }

    let documented_variants = vec![
        DocumentedVariant {
            variant_id: "traditional-black-cannon-stone-radical".to_string(),
            applies_to_set: GlyphSetId::TraditionalPrototypeV1.as_str().to_string(),
            side: PieceSide::Black,
            role: PieceRole::Cannon,
            character: "砲".to_string(),
            codepoint: "U+7832".to_string(),
            svg_file: "assets/board/glyphs/u7832.svg".to_string(),
            notes: "Default in traditional-prototype-v1: pairs stone-radical 砲 (Black) with fire-radical 炮 (Red) per classic carved Xiangqi set convention.".to_string(),
        },
        DocumentedVariant {
            variant_id: "simplified-shared-cannon-fire-radical".to_string(),
            applies_to_set: GlyphSetId::SimplifiedPrototypeV1.as_str().to_string(),
            side: PieceSide::Black,
            role: PieceRole::Cannon,
            character: "炮".to_string(),
            codepoint: "U+70AE".to_string(),
            svg_file: "assets/board/glyphs/u70ae.svg".to_string(),
            notes: "Default in simplified-prototype-v1: uses 炮 (U+70AE) on both sides with vermilion/charcoal engraving and optional u7832.svg stone-radical override.".to_string(),
        },
    ];

    let manifest = GlyphManifest {
        schema_version: "1.0.0".to_string(),
        view_box: [0, 0, 1000, 1000],
        y_axis: "down".to_string(),
        optical_centering: "ink-bounding-box-center-at-500-500-uniform-820-em-scale".to_string(),
        sets: vec![
            GlyphSetId::SimplifiedPrototypeV1.as_str().to_string(),
            GlyphSetId::TraditionalPrototypeV1.as_str().to_string(),
        ],
        documented_variants,
        glyphs,
    };

    let json_bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut out = json_bytes;
    out.push(b'\n');
    let manifest_path = repo_root.join("assets/board/glyphs.json");
    fs::write(manifest_path, out)?;

    Ok(manifest)
}

pub fn load_glyph_manifest(repo_root: &Path) -> Result<GlyphManifest, PipelineError> {
    let path = repo_root.join("assets/board/glyphs.json");
    let bytes = fs::read(&path).map_err(|e| {
        PipelineError::Validation(format!("missing or unreadable {}: {e}", path.display()))
    })?;
    let manifest: GlyphManifest = serde_json::from_slice(&bytes)?;
    Ok(manifest)
}

/// Parses our canonical normalized SVG `d` string (`M`, `L`, `Q`, `C`, `Z`) into a `tiny_skia::Path`,
/// mapped via `transform` from `0..1000` glyph space into target pixel coordinates.
pub fn parse_svg_path_d(d: &str, transform: Transform) -> Result<tiny_skia::Path, PipelineError> {
    let mut pb = PathBuilder::new();
    let bytes = d.as_bytes();
    let mut i = 0;

    let skip_ws_commas = |idx: &mut usize| {
        while *idx < bytes.len() && (bytes[*idx].is_ascii_whitespace() || bytes[*idx] == b',') {
            *idx += 1;
        }
    };

    let parse_f32 = |idx: &mut usize| -> Result<f32, PipelineError> {
        skip_ws_commas(idx);
        let start = *idx;
        if *idx < bytes.len() && (bytes[*idx] == b'+' || bytes[*idx] == b'-') {
            *idx += 1;
        }
        while *idx < bytes.len()
            && (bytes[*idx].is_ascii_digit()
                || bytes[*idx] == b'.'
                || bytes[*idx] == b'e'
                || bytes[*idx] == b'E')
        {
            *idx += 1;
        }
        if *idx == start {
            return Err(PipelineError::Validation(format!(
                "expected float in SVG path at byte {start}"
            )));
        }
        let token = std::str::from_utf8(&bytes[start..*idx])
            .map_err(|e| PipelineError::Validation(format!("invalid UTF-8 in SVG path: {e}")))?;
        token.parse::<f32>().map_err(|e| {
            PipelineError::Validation(format!("invalid float '{token}' in SVG path: {e}"))
        })
    };

    while i < bytes.len() {
        skip_ws_commas(&mut i);
        if i >= bytes.len() {
            break;
        }
        let cmd = bytes[i] as char;
        i += 1;
        match cmd {
            'M' => {
                let x = parse_f32(&mut i)?;
                let y = parse_f32(&mut i)?;
                pb.move_to(x, y);
            }
            'L' => {
                let x = parse_f32(&mut i)?;
                let y = parse_f32(&mut i)?;
                pb.line_to(x, y);
            }
            'Q' => {
                let x1 = parse_f32(&mut i)?;
                let y1 = parse_f32(&mut i)?;
                let x = parse_f32(&mut i)?;
                let y = parse_f32(&mut i)?;
                pb.quad_to(x1, y1, x, y);
            }
            'C' => {
                let x1 = parse_f32(&mut i)?;
                let y1 = parse_f32(&mut i)?;
                let x2 = parse_f32(&mut i)?;
                let y2 = parse_f32(&mut i)?;
                let x = parse_f32(&mut i)?;
                let y = parse_f32(&mut i)?;
                pb.cubic_to(x1, y1, x2, y2, x, y);
            }
            'Z' | 'z' => {
                pb.close();
            }
            other => {
                return Err(PipelineError::Validation(format!(
                    "unsupported SVG command '{other}' in canonical glyph path"
                )));
            }
        }
    }

    let raw_path = pb
        .finish()
        .ok_or_else(|| PipelineError::Validation("empty or degenerate SVG path".to_string()))?;
    raw_path
        .transform(transform)
        .ok_or_else(|| PipelineError::Validation("failed to transform SVG path".to_string()))
}

pub fn verify_glyph_manifest(
    repo_root: &Path,
    manifest: &GlyphManifest,
) -> Result<(), PipelineError> {
    if manifest.glyphs.len() != 28 {
        return Err(PipelineError::Validation(format!(
            "expected 28 glyph entries (2 sets × 14 faces), found {}",
            manifest.glyphs.len()
        )));
    }

    for set_id in GlyphSetId::ALL {
        for side in PieceSide::ALL {
            for role in PieceRole::ALL {
                let expected_char = face_character(set_id, side, role);
                let entry = manifest.find(set_id, side, role).ok_or_else(|| {
                    PipelineError::Validation(format!(
                        "missing glyph entry for {}.{}.{}",
                        set_id.as_str(),
                        side.as_str(),
                        role.as_str()
                    ))
                })?;

                if entry.character != expected_char.to_string() {
                    return Err(PipelineError::Validation(format!(
                        "{}: expected character '{}', found '{}'",
                        entry.glyph_id, expected_char, entry.character
                    )));
                }
                let expected_cp = format!("U+{:04X}", expected_char as u32);
                if entry.codepoint != expected_cp {
                    return Err(PipelineError::Validation(format!(
                        "{}: expected codepoint {}, found {}",
                        entry.glyph_id, expected_cp, entry.codepoint
                    )));
                }
                if entry.review_status != GLYPH_REVIEW_STATUS {
                    return Err(PipelineError::Validation(format!(
                        "{}: unexpected review_status '{}'",
                        entry.glyph_id, entry.review_status
                    )));
                }
                if entry.font_source.font_sha256 != EXPECTED_FONT_SHA256 {
                    return Err(PipelineError::Validation(format!(
                        "{}: unexpected font_sha256 '{}'",
                        entry.glyph_id, entry.font_source.font_sha256
                    )));
                }

                // Verify SVG file exists, matches recorded SHA-256, and contains `svg_path`
                let svg_abs = repo_root.join(&entry.svg_file);
                let svg_bytes = fs::read(&svg_abs).map_err(|e| {
                    PipelineError::Validation(format!(
                        "{}: failed to read {}: {e}",
                        entry.glyph_id,
                        svg_abs.display()
                    ))
                })?;
                let actual_sha = hex::encode(Sha256::digest(&svg_bytes));
                if actual_sha != entry.sha256 {
                    return Err(PipelineError::Validation(format!(
                        "{}: SVG SHA-256 mismatch for {}: expected {}, got {}",
                        entry.glyph_id, entry.svg_file, entry.sha256, actual_sha
                    )));
                }
                let svg_text = std::str::from_utf8(&svg_bytes).map_err(|e| {
                    PipelineError::Validation(format!("{}: invalid UTF-8 SVG: {e}", entry.glyph_id))
                })?;
                if !svg_text.contains(&entry.svg_path) {
                    return Err(PipelineError::Validation(format!(
                        "{}: SVG file {} does not embed canonical svg_path",
                        entry.glyph_id, entry.svg_file
                    )));
                }

                // Verify path parses cleanly into non-empty geometry centered near (500, 500)
                let path = parse_svg_path_d(&entry.svg_path, Transform::identity())?;
                let pb = path.bounds();
                if pb.width() < 200.0 || pb.height() < 200.0 {
                    return Err(PipelineError::Validation(format!(
                        "{}: glyph bounds suspiciously small: {:?}",
                        entry.glyph_id, pb
                    )));
                }
                let cx = 0.5 * (entry.bounds.x_min + entry.bounds.x_max);
                let cy = 0.5 * (entry.bounds.y_min + entry.bounds.y_max);
                if (cx - 500.0).abs() > 0.5 || (cy - 500.0).abs() > 0.5 {
                    return Err(PipelineError::Validation(format!(
                        "{}: glyph optical center ({cx:.2}, {cy:.2}) deviates from (500, 500)",
                        entry.glyph_id
                    )));
                }
            }
        }
    }

    Ok(())
}
