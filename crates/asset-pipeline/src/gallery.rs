//! Visual reference proof-sheet renderer (`docs/visual-reference/*.png`).
//!
//! Generates:
//! 1. `glyph-contact-sheet.png` — All 14 side/role faces in Simplified (`zh-Hans`) and
//!    Traditional (`zh-Hant`) across `pale-jade`, `boxwood`, and `ivory-stone` on light & dark grounds.
//! 2. `board-opening-light-dark.png` — Full `9 × 10` Xiangqi opening board on the light (`#EDE5D6`)
//!    and dark (`#222B27`) board surfaces, both Red-below and Black-below orientations, verifying the
//!    upright-glyph and fixed-key-light orientation invariant.
//! 3. `board-crowded-phone-360px.png` — Crowded middlegame rendered at the smallest supported
//!    mobile viewport (`360 × 420` px) with selection ring, legal-move dots, capture rings,
//!    last-move brackets, and dashed check ring to verify stroke legibility and non-color-only state cues.
//!
//! Board surfaces and state markers use the values in `docs/rendering.md` §4, which the web and Apple
//! renderers draw, so these plates show what players see.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapMut, Rect, Stroke,
    StrokeDash, Transform,
};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::PipelineError;
use crate::glyphs::{GlyphManifest, GlyphSetId, PieceRole, PieceSide};
use crate::materials::{
    MaterialFamily, TILE_SIZE, decode_png_info, render_surface_tile, write_rgba_png,
};

#[derive(Debug, Clone)]
pub struct GeneratedPlate {
    pub asset_id: String,
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub description: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SpriteKey {
    material: MaterialFamily,
    set_id: GlyphSetId,
    side: PieceSide,
    role: PieceRole,
}

struct SpriteImage {
    width: u32,
    height: u32,
    /// Premultiplied RGBA floats in `[0.0, 1.0]` for exact area-integrated downsampling without edge halos.
    premul: Vec<[f32; 4]>,
}

impl SpriteImage {
    fn from_png_path(path: &Path) -> Result<Self, PipelineError> {
        let (w, h, color_type, raw) = decode_png_info(path)?;
        let mut premul = Vec::with_capacity((w * h) as usize);
        match color_type {
            png::ColorType::Rgba => {
                for px in raw.as_chunks::<4>().0 {
                    let a = (px[3] as f32) / 255.0;
                    premul.push([
                        ((px[0] as f32) / 255.0) * a,
                        ((px[1] as f32) / 255.0) * a,
                        ((px[2] as f32) / 255.0) * a,
                        a,
                    ]);
                }
            }
            png::ColorType::Rgb => {
                for px in raw.as_chunks::<3>().0 {
                    premul.push([
                        (px[0] as f32) / 255.0,
                        (px[1] as f32) / 255.0,
                        (px[2] as f32) / 255.0,
                        1.0,
                    ]);
                }
            }
            other => {
                return Err(PipelineError::Validation(format!(
                    "{}: unsupported color type {other:?}",
                    path.display()
                )));
            }
        }
        Ok(Self {
            width: w,
            height: h,
            premul,
        })
    }
}

/// Area-averaging (box-filter) resampler & Porter-Duff `source-over` compositor onto a `tiny-skia::Pixmap`.
/// Converts straight-alpha source sprites to premultiplied alpha prior to filtering per Section 4.
fn blit_sprite_area(
    dst: &mut PixmapMut<'_>,
    src: &SpriteImage,
    center_x: f32,
    center_y: f32,
    box_size: f32,
    opacity: f32,
) {
    let half = 0.5 * box_size;
    let dst_x0 = (center_x - half).floor().max(0.0) as i32;
    let dst_y0 = (center_y - half).floor().max(0.0) as i32;
    let dst_x1 = (center_x + half).ceil().min(dst.width() as f32) as i32;
    let dst_y1 = (center_y + half).ceil().min(dst.height() as f32) as i32;

    if dst_x1 <= dst_x0 || dst_y1 <= dst_y0 || box_size <= 0.5 {
        return;
    }

    let scale_x = (src.width as f32) / box_size;
    let scale_y = (src.height as f32) / box_size;
    let sw = src.width as i32;
    let sh = src.height as i32;
    let dst_w = dst.width() as i32;
    let dst_data = dst.data_mut();

    for dy in dst_y0..dst_y1 {
        let sy_start = ((dy as f32) - (center_y - half)) * scale_y;
        let sy_end = (((dy + 1) as f32) - (center_y - half)) * scale_y;
        let iy0 = sy_start.floor().max(0.0) as i32;
        let iy1 = sy_end.ceil().min(sh as f32) as i32;

        for dx in dst_x0..dst_x1 {
            let sx_start = ((dx as f32) - (center_x - half)) * scale_x;
            let sx_end = (((dx + 1) as f32) - (center_x - half)) * scale_x;
            let ix0 = sx_start.floor().max(0.0) as i32;
            let ix1 = sx_end.ceil().min(sw as f32) as i32;

            let mut acc = [0.0_f32; 4];
            let mut w_sum = 0.0_f32;

            for sy in iy0..iy1 {
                let wy = (sy_end.min((sy + 1) as f32) - sy_start.max(sy as f32)).max(0.0);
                if wy <= 0.0 {
                    continue;
                }
                let row_off = (sy * sw) as usize;
                for sx in ix0..ix1 {
                    let wx = (sx_end.min((sx + 1) as f32) - sx_start.max(sx as f32)).max(0.0);
                    let w = wx * wy;
                    if w <= 0.0 {
                        continue;
                    }
                    let p = src.premul[row_off + (sx as usize)];
                    acc[0] += p[0] * w;
                    acc[1] += p[1] * w;
                    acc[2] += p[2] * w;
                    acc[3] += p[3] * w;
                    w_sum += w;
                }
            }

            if w_sum <= 0.0 {
                continue;
            }
            let inv_w = opacity / w_sum;
            let sr = (acc[0] * inv_w).clamp(0.0, 1.0);
            let sg = (acc[1] * inv_w).clamp(0.0, 1.0);
            let sb = (acc[2] * inv_w).clamp(0.0, 1.0);
            let sa = (acc[3] * inv_w).clamp(0.0, 1.0);
            if sa <= 1.0 / 512.0 {
                continue;
            }

            let didx = ((dy * dst_w + dx) * 4) as usize;
            let dr = (dst_data[didx] as f32) / 255.0;
            let dg = (dst_data[didx + 1] as f32) / 255.0;
            let db = (dst_data[didx + 2] as f32) / 255.0;
            let da = (dst_data[didx + 3] as f32) / 255.0;

            let inv_sa = 1.0 - sa;
            dst_data[didx] = ((sr + dr * inv_sa) * 255.0).round().clamp(0.0, 255.0) as u8;
            dst_data[didx + 1] = ((sg + dg * inv_sa) * 255.0).round().clamp(0.0, 255.0) as u8;
            dst_data[didx + 2] = ((sb + db * inv_sa) * 255.0).round().clamp(0.0, 255.0) as u8;
            dst_data[didx + 3] = ((sa + da * inv_sa) * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Fills a rectangular region of `dst` with seamless tiled pixels from a `TILE_SIZE × TILE_SIZE` RGBA buffer.
fn fill_rect_tiled(
    dst: &mut PixmapMut<'_>,
    tile_rgba: &[u8],
    tile_size: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
) {
    let x1 = (x0 + w).min(dst.width());
    let y1 = (y0 + h).min(dst.height());
    let dst_w = dst.width();
    let dst_data = dst.data_mut();
    for y in y0..y1 {
        let ty = y % tile_size;
        for x in x0..x1 {
            let tx = x % tile_size;
            let sidx = ((ty * tile_size + tx) * 4) as usize;
            let didx = ((y * dst_w + x) * 4) as usize;
            dst_data[didx..didx + 4].copy_from_slice(&tile_rgba[sidx..sidx + 4]);
        }
    }
}

/// Light board surface (`docs/rendering.md` §4, Layer 1): `#EDE5D6` under the paper grain at 32%.
const LIGHT_SURFACE: ([u8; 3], f32) = ([0xED, 0xE5, 0xD6], 0.32);
/// Dark board surface: `#222B27` under the dark grain at 24%.
const DARK_SURFACE: ([u8; 3], f32) = ([0x22, 0x2B, 0x27], 0.24);

/// Fills a rectangle with a board surface: the solid base color with the grain tile laid over it
/// at `surface.1` opacity, as the web and Apple renderers draw it.
#[allow(clippy::too_many_arguments)]
fn fill_rect_surface(
    dst: &mut PixmapMut<'_>,
    tile_rgba: &[u8],
    tile_size: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    surface: ([u8; 3], f32),
) {
    let (base, alpha) = surface;
    let x1 = (x0 + w).min(dst.width());
    let y1 = (y0 + h).min(dst.height());
    let dst_w = dst.width();
    let dst_data = dst.data_mut();
    for y in y0..y1 {
        let ty = y % tile_size;
        for x in x0..x1 {
            let tx = x % tile_size;
            let sidx = ((ty * tile_size + tx) * 4) as usize;
            let didx = ((y * dst_w + x) * 4) as usize;
            for c in 0..3 {
                let grain = f32::from(tile_rgba[sidx + c]);
                let mixed = f32::from(base[c]) * (1.0 - alpha) + grain * alpha;
                dst_data[didx + c] = mixed.round().clamp(0.0, 255.0) as u8;
            }
            dst_data[didx + 3] = 255;
        }
    }
}

/// Converts a `tiny-skia::Pixmap` (premultiplied RGBA8) to straight-alpha RGBA8 and saves as PNG.
fn save_pixmap_png(pixmap: &Pixmap, path: &Path) -> Result<(), PipelineError> {
    let src = pixmap.data();
    let mut straight = vec![0u8; src.len()];
    for (s, d) in src
        .as_chunks::<4>()
        .0
        .iter()
        .zip(straight.as_chunks_mut::<4>().0)
    {
        let a = s[3];
        if a == 0 {
            d.copy_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            d.copy_from_slice(s);
        } else {
            let af = (a as f32) / 255.0;
            d[0] = (((s[0] as f32) / af).round().clamp(0.0, 255.0)) as u8;
            d[1] = (((s[1] as f32) / af).round().clamp(0.0, 255.0)) as u8;
            d[2] = (((s[2] as f32) / af).round().clamp(0.0, 255.0)) as u8;
            d[3] = a;
        }
    }
    write_rgba_png(path, pixmap.width(), pixmap.height(), &straight)
}

struct SkiaGlyphOutline {
    pb: PathBuilder,
    scale: f32,
    origin_x: f32,
    baseline_y: f32,
}

impl OutlineBuilder for SkiaGlyphOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(
            self.origin_x + x * self.scale,
            self.baseline_y - y * self.scale,
        );
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(
            self.origin_x + x * self.scale,
            self.baseline_y - y * self.scale,
        );
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(
            self.origin_x + x1 * self.scale,
            self.baseline_y - y1 * self.scale,
            self.origin_x + x * self.scale,
            self.baseline_y - y * self.scale,
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.origin_x + x1 * self.scale,
            self.baseline_y - y1 * self.scale,
            self.origin_x + x2 * self.scale,
            self.baseline_y - y2 * self.scale,
            self.origin_x + x * self.scale,
            self.baseline_y - y * self.scale,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

fn measure_text(face: &Face<'_>, text: &str, px_size: f32) -> f32 {
    let scale = px_size / (face.units_per_em() as f32);
    let mut width = 0.0_f32;
    for ch in text.chars() {
        let gid = face.glyph_index(ch).unwrap_or(GlyphId(0));
        if let Some(adv) = face.glyph_hor_advance(gid) {
            width += (adv as f32) * scale;
        }
    }
    width
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    dst: &mut PixmapMut<'_>,
    face: &Face<'_>,
    text: &str,
    x: f32,
    baseline_y: f32,
    px_size: f32,
    color: Color,
    centered: bool,
) {
    let scale = px_size / (face.units_per_em() as f32);
    let mut pen_x = if centered {
        x - 0.5 * measure_text(face, text, px_size)
    } else {
        x
    };

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    for ch in text.chars() {
        let gid = face.glyph_index(ch).unwrap_or(GlyphId(0));
        let mut builder = SkiaGlyphOutline {
            pb: PathBuilder::new(),
            scale,
            origin_x: pen_x,
            baseline_y,
        };
        if face.outline_glyph(gid, &mut builder).is_some()
            && let Some(path) = builder.pb.finish()
        {
            dst.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        if let Some(adv) = face.glyph_hor_advance(gid) {
            pen_x += (adv as f32) * scale;
        }
    }
}

/// Exact Section 4 viewport layout (`W, H, p >= 0`).
#[derive(Clone, Copy)]
struct BoardGeometry {
    s: f32,
    ox: f32,
    oy: f32,
    red_below: bool,
}

impl BoardGeometry {
    fn from_viewport(vp_x: f32, vp_y: f32, w: f32, h: f32, pad: f32, red_below: bool) -> Self {
        let s = ((w - 2.0 * pad) / 9.0).min((h - 2.0 * pad) / 10.0);
        let ox = vp_x + 0.5 * (w - 8.0 * s);
        let oy = vp_y + 0.5 * (h - 9.0 * s);
        Self {
            s,
            ox,
            oy,
            red_below,
        }
    }

    #[inline]
    fn screen_xy(self, file_x: u8, rank_y: u8) -> (f32, f32) {
        let x = file_x as f32;
        let y = rank_y as f32;
        if self.red_below {
            (self.ox + self.s * x, self.oy + self.s * (9.0 - y))
        } else {
            (self.ox + self.s * (8.0 - x), self.oy + self.s * y)
        }
    }
}

fn color_hex(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// Draws the traditional `9 × 10` Xiangqi grid, outer frame, palace diagonals, and L-shaped fiducial ticks.
fn draw_xiangqi_grid(dst: &mut PixmapMut<'_>, geom: BoardGeometry, is_dark: bool) {
    let line_col = if is_dark {
        color_hex(0xDC, 0xE3, 0xCE, 196) // Pale-jade crisp linework on dark slate
    } else {
        color_hex(0x25, 0x2A, 0x25, 224) // Charcoal ink linework on warm paper
    };
    let accent_frame_col = if is_dark {
        color_hex(0xDC, 0xE3, 0xCE, 128)
    } else {
        color_hex(0x41, 0x54, 0x47, 185)
    };

    let mut paint = Paint::default();
    paint.set_color(line_col);
    paint.anti_alias = true;

    let mut frame_paint = Paint::default();
    frame_paint.set_color(accent_frame_col);
    frame_paint.anti_alias = true;

    let lw = (geom.s * 0.032).clamp(1.0, 2.2);
    let mut stroke = Stroke {
        width: lw,
        line_cap: LineCap::Square,
        line_join: LineJoin::Miter,
        ..Stroke::default()
    };

    let (x0, y0) = (geom.ox, geom.oy);
    let (x8, y9) = (geom.ox + 8.0 * geom.s, geom.oy + 9.0 * geom.s);

    // Outer decorative frame slightly outside the grid
    let frame_pad = (geom.s * 0.11).max(3.5);
    let mut outer_stroke = stroke.clone();
    outer_stroke.width = (lw * 1.45).clamp(1.4, 3.0);
    let mut pb_frame = PathBuilder::new();
    if let Some(rect) = Rect::from_ltrb(
        x0 - frame_pad,
        y0 - frame_pad,
        x8 + frame_pad,
        y9 + frame_pad,
    ) {
        pb_frame.push_rect(rect);
        if let Some(path) = pb_frame.finish() {
            dst.stroke_path(
                &path,
                &frame_paint,
                &outer_stroke,
                Transform::identity(),
                None,
            );
        }
    }

    let mut pb = PathBuilder::new();
    // 10 horizontal rank lines
    for r in 0..=9 {
        let y = y0 + (r as f32) * geom.s;
        pb.move_to(x0, y);
        pb.line_to(x8, y);
    }
    // Outer vertical boundary files (`c = 0` and `c = 8` span full `0..9`, crossing the river)
    for c in [0, 8] {
        let x = x0 + (c as f32) * geom.s;
        pb.move_to(x, y0);
        pb.line_to(x, y9);
    }
    // Interior files (`c = 1..7`) split cleanly across the central river (`r = 0..4` and `r = 5..9`)
    for c in 1..=7 {
        let x = x0 + (c as f32) * geom.s;
        pb.move_to(x, y0);
        pb.line_to(x, y0 + 4.0 * geom.s);
        pb.move_to(x, y0 + 5.0 * geom.s);
        pb.line_to(x, y9);
    }

    // Palace diagonals (canonical `(3,0)-(5,2)`, `(5,0)-(3,2)` and `(3,7)-(5,9)`, `(5,7)-(3,9)`)
    for ((fx0, fy0), (fx1, fy1)) in [
        ((3, 0), (5, 2)),
        ((5, 0), (3, 2)),
        ((3, 7), (5, 9)),
        ((5, 7), (3, 9)),
    ] {
        let (sx0, sy0) = geom.screen_xy(fx0, fy0);
        let (sx1, sy1) = geom.screen_xy(fx1, fy1);
        pb.move_to(sx0, sy0);
        pb.line_to(sx1, sy1);
    }

    // L-shaped fiducial ticks for Cannons and Soldiers
    let fiducials: [(u8, u8); 14] = [
        (1, 2),
        (7, 2),
        (1, 7),
        (7, 7),
        (0, 3),
        (2, 3),
        (4, 3),
        (6, 3),
        (8, 3),
        (0, 6),
        (2, 6),
        (4, 6),
        (6, 6),
        (8, 6),
    ];
    let gap = (geom.s * 0.085).max(2.2);
    let arm = (geom.s * 0.16).max(3.8);
    for (fx, fy) in fiducials {
        let (cx, cy) = geom.screen_xy(fx, fy);
        // Determine which screen-space quadrants stay inside `[x0..x8]`
        for qx in [-1.0_f32, 1.0_f32] {
            let test_x = cx + qx * (gap + arm);
            if test_x < x0 - 0.5 || test_x > x8 + 0.5 {
                continue;
            }
            for qy in [-1.0_f32, 1.0_f32] {
                pb.move_to(cx + qx * gap, cy + qy * (gap + arm));
                pb.line_to(cx + qx * gap, cy + qy * gap);
                pb.line_to(cx + qx * (gap + arm), cy + qy * gap);
            }
        }
    }

    if let Some(path) = pb.finish() {
        stroke.line_cap = LineCap::Butt;
        dst.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn opening_pieces() -> Vec<(u8, u8, PieceSide, PieceRole)> {
    let back_rank = [
        PieceRole::Chariot,
        PieceRole::Horse,
        PieceRole::Elephant,
        PieceRole::Advisor,
        PieceRole::General,
        PieceRole::Advisor,
        PieceRole::Elephant,
        PieceRole::Horse,
        PieceRole::Chariot,
    ];
    let mut pieces = Vec::with_capacity(32);
    for (x, role) in back_rank.into_iter().enumerate() {
        pieces.push((x as u8, 0, PieceSide::Red, role));
        pieces.push((x as u8, 9, PieceSide::Black, role));
    }
    for x in [1u8, 7u8] {
        pieces.push((x, 2, PieceSide::Red, PieceRole::Cannon));
        pieces.push((x, 7, PieceSide::Black, PieceRole::Cannon));
    }
    for x in [0u8, 2, 4, 6, 8] {
        pieces.push((x, 3, PieceSide::Red, PieceRole::Soldier));
        pieces.push((x, 6, PieceSide::Black, PieceRole::Soldier));
    }
    pieces
}

fn load_sprite_bank(
    repo_root: &Path,
) -> Result<(BTreeMap<SpriteKey, SpriteImage>, SpriteImage), PipelineError> {
    let mut bank = BTreeMap::new();
    for material in MaterialFamily::ALL {
        for set_id in GlyphSetId::ALL {
            for side in PieceSide::ALL {
                for role in PieceRole::ALL {
                    let path = repo_root.join(format!(
                        "assets/board/materials/{}/{}/{}-{}@2x.png",
                        material.as_str(),
                        set_id.short_dir(),
                        side.as_str(),
                        role.as_str()
                    ));
                    let img = SpriteImage::from_png_path(&path)?;
                    bank.insert(
                        SpriteKey {
                            material,
                            set_id,
                            side,
                            role,
                        },
                        img,
                    );
                }
            }
        }
    }
    let shadow = SpriteImage::from_png_path(
        &repo_root.join("assets/board/materials/contact-shadow@2x.png"),
    )?;
    Ok((bank, shadow))
}

fn render_contact_sheet(
    repo_root: &Path,
    sans_face: &Face<'_>,
    glyph_manifest: &GlyphManifest,
    sprites: &BTreeMap<SpriteKey, SpriteImage>,
    shadow: &SpriteImage,
    paper_tile: &[u8],
    dark_tile: &[u8],
) -> Result<GeneratedPlate, PipelineError> {
    let width = 1640u32;
    let height = 1320u32;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
        PipelineError::Validation("failed to allocate contact sheet pixmap".to_string())
    })?;

    {
        let mut pm = pixmap.as_mut();
        fill_rect_tiled(&mut pm, paper_tile, TILE_SIZE, 0, 0, width, height);

        let charcoal = color_hex(0x25, 0x2A, 0x25, 255);
        let deep_jade = color_hex(0x41, 0x54, 0x47, 255);
        let warm_paper = color_hex(0xF3, 0xEE, 0xE4, 255);

        draw_text(
            &mut pm,
            sans_face,
            "Jade Xiangqi — Orthographic 2.5D Piece & Glyph Contact Sheet (Simplified & Traditional)",
            48.0,
            52.0,
            24.0,
            charcoal,
            false,
        );
        draw_text(
            &mut pm,
            sans_face,
            "Source: Noto Serif CJK SC Bold 2.003 (SIL OFL 1.1) | Top-left key light (-45 deg az, 52 deg el) | Prototype pending native calligraphy review",
            48.0,
            80.0,
            14.5,
            deep_jade,
            false,
        );

        let col_x0 = 260.0_f32;
        let col_step = 96.0_f32;
        let roles = PieceRole::ALL;

        // Column headers (`Red` 0..6, `Black` 7..13)
        for (side_idx, side) in PieceSide::ALL.into_iter().enumerate() {
            for (r_idx, role) in roles.into_iter().enumerate() {
                let col = side_idx * 7 + r_idx;
                let cx = col_x0 + (col as f32) * col_step;
                let label = format!(
                    "{} {}",
                    if side == PieceSide::Red { "R" } else { "B" },
                    role.as_str()
                );
                draw_text(&mut pm, sans_face, &label, cx, 122.0, 12.5, charcoal, true);
            }
        }

        // Six main rows: 3 materials × 2 script sets (first 4 on light paper, last 2 + dark comparison band on dark slate)
        let rows: [(MaterialFamily, GlyphSetId, bool, &str); 8] = [
            (
                MaterialFamily::PaleJade,
                GlyphSetId::SimplifiedPrototypeV1,
                false,
                "Pale Jade / Simplified",
            ),
            (
                MaterialFamily::PaleJade,
                GlyphSetId::TraditionalPrototypeV1,
                false,
                "Pale Jade / Traditional",
            ),
            (
                MaterialFamily::Boxwood,
                GlyphSetId::SimplifiedPrototypeV1,
                false,
                "Boxwood / Simplified",
            ),
            (
                MaterialFamily::Boxwood,
                GlyphSetId::TraditionalPrototypeV1,
                false,
                "Boxwood / Traditional",
            ),
            (
                MaterialFamily::IvoryStone,
                GlyphSetId::SimplifiedPrototypeV1,
                false,
                "Ivory Stone / Simplified",
            ),
            (
                MaterialFamily::IvoryStone,
                GlyphSetId::TraditionalPrototypeV1,
                false,
                "Ivory Stone / Traditional",
            ),
            (
                MaterialFamily::PaleJade,
                GlyphSetId::SimplifiedPrototypeV1,
                true,
                "Pale Jade (Dark Ground)",
            ),
            (
                MaterialFamily::PaleJade,
                GlyphSetId::TraditionalPrototypeV1,
                true,
                "Pale Jade Trad (Dark)",
            ),
        ];

        // Dark ground backdrop band covering rows 6 & 7
        fill_rect_tiled(&mut pm, dark_tile, TILE_SIZE, 28, 962, width - 56, 326);

        for (row_idx, (mat, set_id, on_dark, title)) in rows.into_iter().enumerate() {
            let cy = 194.0 + (row_idx as f32) * 138.0 + if row_idx >= 6 { 18.0 } else { 0.0 };
            let label_col = if on_dark { warm_paper } else { charcoal };
            let sub_col = if on_dark {
                color_hex(0xDC, 0xE3, 0xCE, 210)
            } else {
                deep_jade
            };

            draw_text(
                &mut pm,
                sans_face,
                title,
                44.0,
                cy - 4.0,
                15.0,
                label_col,
                false,
            );
            draw_text(
                &mut pm,
                sans_face,
                set_id.locale_tag(),
                44.0,
                cy + 16.0,
                12.5,
                sub_col,
                false,
            );

            for (side_idx, side) in PieceSide::ALL.into_iter().enumerate() {
                for (r_idx, role) in roles.into_iter().enumerate() {
                    let col = side_idx * 7 + r_idx;
                    let cx = col_x0 + (col as f32) * col_step;
                    let sprite = sprites
                        .get(&SpriteKey {
                            material: mat,
                            set_id,
                            side,
                            role,
                        })
                        .expect("sprite present");
                    blit_sprite_area(&mut pm, shadow, cx, cy, 88.0, 1.0);
                    blit_sprite_area(&mut pm, sprite, cx, cy, 88.0, 1.0);

                    if let Some(g) = glyph_manifest.find(set_id, side, role) {
                        draw_text(
                            &mut pm,
                            sans_face,
                            &g.codepoint,
                            cx,
                            cy + 56.0,
                            11.0,
                            sub_col,
                            true,
                        );
                    }
                }
            }
        }
    }

    let rel_path = "docs/visual-reference/glyph-contact-sheet.png".to_string();
    let abs_path = repo_root.join(&rel_path);
    save_pixmap_png(&pixmap, &abs_path)?;
    Ok(GeneratedPlate {
        asset_id: "docs.visual-reference.glyph-contact-sheet".to_string(),
        rel_path,
        abs_path,
        width,
        height,
        description: "All 14 Xiangqi piece faces in Simplified and Traditional across Pale Jade, Boxwood, and Ivory Stone on light and dark grounds.".to_string(),
    })
}

fn render_opening_boards(
    repo_root: &Path,
    sans_face: &Face<'_>,
    sprites: &BTreeMap<SpriteKey, SpriteImage>,
    shadow: &SpriteImage,
    paper_tile: &[u8],
    dark_tile: &[u8],
) -> Result<GeneratedPlate, PipelineError> {
    let width = 1560u32;
    let height = 1720u32;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
        PipelineError::Validation("failed to allocate opening board pixmap".to_string())
    })?;

    {
        let mut pm = pixmap.as_mut();
        // Left half: light board surface `#EDE5D6`; right half: dark board surface `#222B27`
        let half_w = width / 2;
        fill_rect_surface(
            &mut pm,
            paper_tile,
            TILE_SIZE,
            0,
            0,
            half_w,
            height,
            LIGHT_SURFACE,
        );
        fill_rect_surface(
            &mut pm,
            dark_tile,
            TILE_SIZE,
            half_w,
            0,
            width - half_w,
            height,
            DARK_SURFACE,
        );

        let charcoal = color_hex(0x25, 0x2A, 0x25, 255);
        let deep_jade = color_hex(0x41, 0x54, 0x47, 255);
        let warm_paper = color_hex(0xF3, 0xEE, 0xE4, 255);
        let pale_jade_txt = color_hex(0xDC, 0xE3, 0xCE, 220);

        struct OpeningPanel<'a> {
            px: u32,
            py: u32,
            is_dark: bool,
            red_below: bool,
            set_id: GlyphSetId,
            material: MaterialFamily,
            title: &'a str,
            subtitle: &'a str,
        }

        let panels = [
            OpeningPanel {
                px: 0,
                py: 0,
                is_dark: false,
                red_below: true,
                set_id: GlyphSetId::SimplifiedPrototypeV1,
                material: MaterialFamily::PaleJade,
                title: "Light (#EDE5D6) — Red Below (Simplified, Pale Jade)",
                subtitle: "Canonical y=0 at bottom; top-left key light & upright glyphs",
            },
            OpeningPanel {
                px: half_w,
                py: 0,
                is_dark: true,
                red_below: true,
                set_id: GlyphSetId::SimplifiedPrototypeV1,
                material: MaterialFamily::PaleJade,
                title: "Dark (#222B27) — Red Below (Simplified, Pale Jade)",
                subtitle: "High-contrast pale-jade linework & shared contact shadow",
            },
            OpeningPanel {
                px: 0,
                py: height / 2,
                is_dark: false,
                red_below: false,
                set_id: GlyphSetId::TraditionalPrototypeV1,
                material: MaterialFamily::PaleJade,
                title: "Light (#EDE5D6) — Black Below (Traditional, Flipped)",
                subtitle: "Flipped orientation: Black y=9 at bottom; glyphs & light stay upright",
            },
            OpeningPanel {
                px: half_w,
                py: height / 2,
                is_dark: true,
                red_below: false,
                set_id: GlyphSetId::TraditionalPrototypeV1,
                material: MaterialFamily::Boxwood,
                title: "Dark (#222B27) — Black Below (Traditional, Boxwood)",
                subtitle: "Alternative Boxwood material set under flipped orientation",
            },
        ];

        let pieces = opening_pieces();

        for panel in panels {
            let OpeningPanel {
                px,
                py,
                is_dark,
                red_below,
                set_id,
                material,
                title,
                subtitle,
            } = panel;
            let title_col = if is_dark { warm_paper } else { charcoal };
            let sub_col = if is_dark { pale_jade_txt } else { deep_jade };
            draw_text(
                &mut pm,
                sans_face,
                title,
                (px as f32) + 42.0,
                (py as f32) + 42.0,
                18.0,
                title_col,
                false,
            );
            draw_text(
                &mut pm,
                sans_face,
                subtitle,
                (px as f32) + 42.0,
                (py as f32) + 66.0,
                13.5,
                sub_col,
                false,
            );

            let geom = BoardGeometry::from_viewport(
                px as f32,
                (py as f32) + 78.0,
                half_w as f32,
                (height / 2) as f32 - 86.0,
                36.0,
                red_below,
            );

            // Layer 1: Board grid & fiducials
            draw_xiangqi_grid(&mut pm, geom, is_dark);

            // Layer 3: Contact shadows (all pieces first so shadow never overlaps an adjacent piece top)
            for &(fx, fy, _, _) in &pieces {
                let (sx, sy) = geom.screen_xy(fx, fy);
                blit_sprite_area(&mut pm, shadow, sx, sy, geom.s, 1.0);
            }

            // Layer 4: Stationary pieces (upright regardless of `red_below`!)
            for &(fx, fy, side, role) in &pieces {
                let (sx, sy) = geom.screen_xy(fx, fy);
                let sprite = sprites
                    .get(&SpriteKey {
                        material,
                        set_id,
                        side,
                        role,
                    })
                    .expect("sprite present");
                blit_sprite_area(&mut pm, sprite, sx, sy, geom.s, 1.0);
            }
        }
    }

    let rel_path = "docs/visual-reference/board-opening-light-dark.png".to_string();
    let abs_path = repo_root.join(&rel_path);
    save_pixmap_png(&pixmap, &abs_path)?;
    Ok(GeneratedPlate {
        asset_id: "docs.visual-reference.board-opening-light-dark".to_string(),
        rel_path,
        abs_path,
        width,
        height,
        description: "Full 9x10 Xiangqi opening position on the light (#EDE5D6) and dark (#222B27) board surfaces and Red-below / Black-below orientations.".to_string(),
    })
}

fn stroke_circle(dst: &mut PixmapMut<'_>, cx: f32, cy: f32, r: f32, width: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        dst.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn fill_circle(dst: &mut PixmapMut<'_>, cx: f32, cy: f32, r: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        dst.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn draw_corner_brackets(
    dst: &mut PixmapMut<'_>,
    cx: f32,
    cy: f32,
    half: f32,
    arm: f32,
    width: f32,
    color: Color,
) {
    let mut pb = PathBuilder::new();
    for sx in [-1.0_f32, 1.0_f32] {
        for sy in [-1.0_f32, 1.0_f32] {
            pb.move_to(cx + sx * half, cy + sy * (half - arm));
            pb.line_to(cx + sx * half, cy + sy * half);
            pb.line_to(cx + sx * (half - arm), cy + sy * half);
        }
    }
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        dst.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

/// Draws the check indicator: a dashed vermilion ring around the checked General, told from the
/// solid selection ring by its dashes (`docs/rendering.md` §4 and §8).
fn draw_check_ring(dst: &mut PixmapMut<'_>, cx: f32, cy: f32, r: f32) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, r);
    if let Some(path) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(color_hex(0xA5, 0x48, 0x37, 255));
        paint.anti_alias = true;
        let stroke = Stroke {
            width: 2.6,
            dash: StrokeDash::new(vec![5.0, 3.0], 0.0),
            ..Stroke::default()
        };
        dst.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn render_crowded_phone_360(
    repo_root: &Path,
    sans_face: &Face<'_>,
    sprites: &BTreeMap<SpriteKey, SpriteImage>,
    shadow: &SpriteImage,
    paper_tile: &[u8],
) -> Result<GeneratedPlate, PipelineError> {
    // Smallest supported mobile board viewport (`360 × 420` logical px)
    let width = 360u32;
    let height = 420u32;
    let mut pixmap = Pixmap::new(width, height).ok_or_else(|| {
        PipelineError::Validation("failed to allocate 360x420 pixmap".to_string())
    })?;

    {
        let mut pm = pixmap.as_mut();
        fill_rect_surface(
            &mut pm,
            paper_tile,
            TILE_SIZE,
            0,
            0,
            width,
            height,
            LIGHT_SURFACE,
        );

        let charcoal = color_hex(0x25, 0x2A, 0x25, 255);
        let deep_jade = color_hex(0x41, 0x54, 0x47, 255);
        let vermilion = color_hex(0xA5, 0x48, 0x37, 255);

        draw_text(
            &mut pm,
            sans_face,
            "360x420 Mobile Viewport — Crowded Middlegame & State Cues",
            180.0,
            15.0,
            10.5,
            charcoal,
            true,
        );

        // Exact Section 4 formula inside `[0, 20]..[360, 404]` (`W = 360, H = 384, p = 12`)
        let geom = BoardGeometry::from_viewport(0.0, 20.0, 360.0, 384.0, 12.0, true);

        // Layer 1: Board grid
        draw_xiangqi_grid(&mut pm, geom, false);

        // Crowded middlegame obeying exact Xiangqi rules:
        // Black Cannon moved `(1,8) -> (4,8)`, checking Red General `(4,0)` over sole screen
        // Red Chariot `(4,4)`. Selecting Red Chariot `(4,4)` yields legal horizontal unscreening
        // moves `(3,4), (2,4), (5,4), (6,4)` and legal captures `(1,4), (7,4), (4,8)`!
        let pieces: &[(u8, u8, PieceSide, PieceRole)] = &[
            // Red pieces
            (4, 0, PieceSide::Red, PieceRole::General), // Checked General!
            (3, 0, PieceSide::Red, PieceRole::Advisor),
            (5, 1, PieceSide::Red, PieceRole::Advisor),
            (2, 0, PieceSide::Red, PieceRole::Elephant),
            (6, 2, PieceSide::Red, PieceRole::Elephant),
            (4, 4, PieceSide::Red, PieceRole::Chariot), // Selected screen piece!
            (7, 1, PieceSide::Red, PieceRole::Chariot),
            (2, 3, PieceSide::Red, PieceRole::Horse),
            (6, 3, PieceSide::Red, PieceRole::Horse),
            (2, 2, PieceSide::Red, PieceRole::Cannon),
            (7, 5, PieceSide::Red, PieceRole::Cannon),
            (0, 3, PieceSide::Red, PieceRole::Soldier),
            (3, 5, PieceSide::Red, PieceRole::Soldier),
            (6, 5, PieceSide::Red, PieceRole::Soldier),
            // Black pieces
            (4, 9, PieceSide::Black, PieceRole::General),
            (3, 9, PieceSide::Black, PieceRole::Advisor),
            (5, 8, PieceSide::Black, PieceRole::Advisor),
            (2, 9, PieceSide::Black, PieceRole::Elephant),
            (6, 7, PieceSide::Black, PieceRole::Elephant),
            (0, 8, PieceSide::Black, PieceRole::Chariot),
            (6, 8, PieceSide::Black, PieceRole::Chariot),
            (2, 6, PieceSide::Black, PieceRole::Horse),
            (7, 4, PieceSide::Black, PieceRole::Horse), // Legal capture target!
            (4, 8, PieceSide::Black, PieceRole::Cannon), // Last-move `(1,8) -> (4,8)` checking General; also legal capture!
            (7, 7, PieceSide::Black, PieceRole::Cannon),
            (1, 4, PieceSide::Black, PieceRole::Soldier), // Legal capture target!
            (3, 6, PieceSide::Black, PieceRole::Soldier),
            (5, 6, PieceSide::Black, PieceRole::Soldier),
        ];

        // Layer 2: Under-piece state indicators, at the shared values of `docs/rendering.md` §4
        // (`d = 0.88 * s` is the piece diameter).
        let d = geom.s * 0.88;
        // 2a. Last-move corner brackets at the origin `(1, 8)` and the destination `(4, 8)`
        for (lx, ly) in [(1u8, 8u8), (4, 8)] {
            let (bx, by) = geom.screen_xy(lx, ly);
            draw_corner_brackets(
                &mut pm,
                bx,
                by,
                d * 0.52,
                (geom.s * 0.16).max(5.0),
                2.0,
                deep_jade,
            );
        }

        // 2b. Dashed check ring around the Red General `(4, 0)`
        let (chk_x, chk_y) = geom.screen_xy(4, 0);
        draw_check_ring(&mut pm, chk_x, chk_y, d * 0.58);

        // Layer 3: Contact shadows
        for &(fx, fy, _, _) in pieces {
            let (sx, sy) = geom.screen_xy(fx, fy);
            blit_sprite_area(&mut pm, shadow, sx, sy, geom.s, 1.0);
        }

        // Layer 4: Stationary pieces (`pale-jade`, `simplified-prototype-v1`)
        for &(fx, fy, side, role) in pieces {
            let (sx, sy) = geom.screen_xy(fx, fy);
            let sprite = sprites
                .get(&SpriteKey {
                    material: MaterialFamily::PaleJade,
                    set_id: GlyphSetId::SimplifiedPrototypeV1,
                    side,
                    role,
                })
                .expect("sprite present");
            blit_sprite_area(&mut pm, sprite, sx, sy, geom.s, 1.0);
        }

        // Layer 6: Foreground selection, legal-move dots, and legal-capture rings
        // 6a. Selected piece `(4, 4)`: solid deep-jade ring
        let (sel_x, sel_y) = geom.screen_xy(4, 4);
        stroke_circle(&mut pm, sel_x, sel_y, d * 0.54, 2.5, deep_jade);

        // 6b. Exact legal quiet destinations for screen Chariot `(4, 4)` (moving off file 4 resolves Cannon check)
        for (qx, qy) in [(3u8, 4u8), (2, 4), (5, 4), (6, 4)] {
            let (dx, dy) = geom.screen_xy(qx, qy);
            fill_circle(
                &mut pm,
                dx,
                dy,
                (geom.s * 0.13).max(4.5),
                color_hex(0x41, 0x54, 0x47, 199),
            );
        }

        // 6c. Exact legal capture targets `(1, 4)` (Soldier), `(7, 4)` (Horse), & `(4, 8)` (checking Cannon)
        for (cx_file, cy_rank) in [(1u8, 4u8), (7, 4), (4, 8)] {
            let (tx, ty) = geom.screen_xy(cx_file, cy_rank);
            stroke_circle(&mut pm, tx, ty, d * 0.53, 2.5, vermilion);
        }

        draw_text(
            &mut pm,
            sans_face,
            "Ring: selected | Dots: quiet moves | Red rings: captures | Dashed ring: check",
            180.0,
            413.0,
            8.0,
            deep_jade,
            true,
        );
    }

    let rel_path = "docs/visual-reference/board-crowded-phone-360px.png".to_string();
    let abs_path = repo_root.join(&rel_path);
    save_pixmap_png(&pixmap, &abs_path)?;
    Ok(GeneratedPlate {
        asset_id: "docs.visual-reference.board-crowded-phone-360px".to_string(),
        rel_path,
        abs_path,
        width,
        height,
        description: "Crowded middlegame at smallest supported mobile viewport (360x420 px) verifying glyph legibility and geometric non-color-only state markers.".to_string(),
    })
}

pub fn generate_visual_reference_gallery(
    repo_root: &Path,
    glyph_manifest: &GlyphManifest,
) -> Result<Vec<GeneratedPlate>, PipelineError> {
    let gallery_dir = repo_root.join("docs/visual-reference");
    fs::create_dir_all(&gallery_dir)?;

    let sans_path = repo_root.join("assets/brand/source/fonts/NotoSans-Regular.ttf");
    let sans_bytes = fs::read(&sans_path).map_err(|e| {
        PipelineError::Validation(format!(
            "failed to read committed plate label font {}: {e}",
            sans_path.display()
        ))
    })?;
    let sans_face = Face::parse(&sans_bytes, 0).map_err(|e| {
        PipelineError::Validation(format!("failed to parse NotoSans-Regular: {e:?}"))
    })?;

    let (sprites, shadow) = load_sprite_bank(repo_root)?;
    let paper_tile = render_surface_tile(false, TILE_SIZE);
    let dark_tile = render_surface_tile(true, TILE_SIZE);

    let p1 = render_contact_sheet(
        repo_root,
        &sans_face,
        glyph_manifest,
        &sprites,
        &shadow,
        &paper_tile,
        &dark_tile,
    )?;
    let p2 = render_opening_boards(
        repo_root,
        &sans_face,
        &sprites,
        &shadow,
        &paper_tile,
        &dark_tile,
    )?;
    let p3 = render_crowded_phone_360(repo_root, &sans_face, &sprites, &shadow, &paper_tile)?;

    Ok(vec![p1, p2, p3])
}
