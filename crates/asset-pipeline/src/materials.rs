//! Orthographic 2.5D procedural mineral/wood/stone shader and straight-alpha sRGB PNG baker.
//!
//! Renders deterministic `@1x` (`96×96`) and `@2x` (`192×192`) piece sprites, unengraved
//! material base discs, contact shadows, and seamless `256×256` board surface grain tiles.
//! All lighting is computed in IEC 61966-2-1 linear RGB and encoded to straight-alpha 8-bit
//! sRGB PNGs with zero premultiplication edge halos.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use tiny_skia::{FillRule, Paint, Pixmap, Transform};

use crate::PipelineError;
use crate::glyphs::{
    GlyphEntry, GlyphManifest, GlyphSetId, PieceRole, PieceSide, parse_svg_path_d,
};
use crate::noise::{boxwood_grain, ivory_stone_vein, nephrite_cloud, periodic_fbm_2d, splitmix64};

pub const LOGICAL_SPRITE_SIZE: u32 = 96;
pub const SPRITE_SIZE_1X: u32 = 96;
pub const SPRITE_SIZE_2X: u32 = 192;
pub const TILE_SIZE: u32 = 256;

/// Disc outer radius as a fraction of sprite canvas width (`diameter = 86%` of cell spacing `s`).
pub const DISC_RADIUS_FRAC: f64 = 0.43;

/// Studio key light azimuth (`-45°` = upper-left) and elevation (`52°`).
pub const KEY_LIGHT_AZIMUTH_DEG: f64 = -45.0;
pub const KEY_LIGHT_ELEVATION_DEG: f64 = 52.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaterialFamily {
    PaleJade,
    Boxwood,
    IvoryStone,
}

impl MaterialFamily {
    pub const ALL: [MaterialFamily; 3] = [
        MaterialFamily::PaleJade,
        MaterialFamily::Boxwood,
        MaterialFamily::IvoryStone,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MaterialFamily::PaleJade => "pale-jade",
            MaterialFamily::Boxwood => "boxwood",
            MaterialFamily::IvoryStone => "ivory-stone",
        }
    }

    pub fn base_hex(self) -> &'static str {
        match self {
            MaterialFamily::PaleJade => "#DCE3CE",
            MaterialFamily::Boxwood => "#DFC496",
            MaterialFamily::IvoryStone => "#EDE6D6",
        }
    }

    pub fn red_ink_hex(self) -> &'static str {
        match self {
            MaterialFamily::PaleJade => "#A54837",
            MaterialFamily::Boxwood => "#99392A",
            MaterialFamily::IvoryStone => "#A54837",
        }
    }

    pub fn black_ink_hex(self) -> &'static str {
        "#252A25"
    }

    pub fn seed_salt(self) -> u64 {
        match self {
            MaterialFamily::PaleJade => 0x5ADE_0001_2026_0001,
            MaterialFamily::Boxwood => 0xB080_0002_2026_0002,
            MaterialFamily::IvoryStone => 0x1709_0003_2026_0003,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedPng {
    pub asset_id: String,
    pub kind: &'static str,
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub scale_factor: u32,
    pub material_family: Option<MaterialFamily>,
    pub glyph: Option<GlyphEntry>,
    pub has_engraving: bool,
    pub is_shadow: bool,
    pub is_surface_tile: bool,
}

#[inline]
pub fn srgb_u8_to_linear(c: u8) -> f64 {
    let u = (c as f64) / 255.0;
    if u <= 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn linear_to_srgb_u8(l: f64) -> u8 {
    let clamped = l.clamp(0.0, 1.0);
    let u = if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (u * 255.0).round().clamp(0.0, 255.0) as u8
}

#[inline]
pub fn hex_to_linear_rgb(hex_str: &str) -> [f64; 3] {
    let s = hex_str.trim_start_matches('#');
    let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0);
    [
        srgb_u8_to_linear(r),
        srgb_u8_to_linear(g),
        srgb_u8_to_linear(b),
    ]
}

#[inline]
fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

#[inline]
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if (edge1 - edge0).abs() < 1e-12 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Encodes straight-alpha RGBA8 pixels into a deterministic sRGB PNG.
pub fn write_rgba_png(
    path: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), PipelineError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    encoder.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut png_writer = encoder
        .write_header()
        .map_err(|e| PipelineError::Validation(format!("PNG header write failed: {e}")))?;
    png_writer
        .write_image_data(rgba)
        .map_err(|e| PipelineError::Validation(format!("PNG data write failed: {e}")))?;
    Ok(())
}

struct EngravingFields {
    ink: Vec<f64>,
    carve: Vec<f64>,
    wall_shadow: Vec<f64>,
}

/// Rasterizes a `0..1000` normalized SVG glyph path onto a `size × size` floating-point
/// ink coverage grid using 2× supersampling via `tiny-skia`, plus a Gaussian-smoothed
/// carve-groove heightfield and upper-left inner-wall shadow mask.
fn build_engraving_fields(svg_path_d: &str, size: u32) -> Result<EngravingFields, PipelineError> {
    let ss = 2u32;
    let hi = size * ss;
    let mut pixmap = Pixmap::new(hi, hi).ok_or_else(|| {
        PipelineError::Validation("failed to allocate glyph supersample pixmap".to_string())
    })?;

    let disc_diameter = 2.0 * DISC_RADIUS_FRAC * (hi as f64);
    // Map `0..1000` glyph box (whose `820`-unit em span contains CJK glyphs) to `0.83 * disc_diameter`
    // so ink spans `~0.65` of disc diameter, matching the brand signet and leaving generous bevel margin.
    let glyph_box = 0.83 * disc_diameter;
    let scale = (glyph_box / 1000.0) as f32;
    let offset = (0.5 * ((hi as f64) - glyph_box)) as f32;
    let transform = Transform::from_scale(scale, scale).post_translate(offset, offset);

    let path = parse_svg_path_d(svg_path_d, transform)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    pixmap.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    let n = (size * size) as usize;
    let mut ink = vec![0.0_f64; n];
    let hi_data = pixmap.data();
    let max_ink_r = 0.78 * DISC_RADIUS_FRAC * (size as f64);
    let center = 0.5 * (size as f64);

    for y in 0..size {
        for x in 0..size {
            let mut sum = 0.0;
            for sy in 0..ss {
                for sx in 0..ss {
                    let hx = x * ss + sx;
                    let hy = y * ss + sy;
                    let idx = ((hy * hi + hx) * 4 + 3) as usize;
                    sum += (hi_data[idx] as f64) / 255.0;
                }
            }
            let dx = (x as f64 + 0.5) - center;
            let dy = (y as f64 + 0.5) - center;
            let r = (dx * dx + dy * dy).sqrt();
            let clip = 1.0 - smoothstep(max_ink_r * 0.96, max_ink_r, r);
            ink[(y * size + x) as usize] = (sum / ((ss * ss) as f64)) * clip;
        }
    }

    // Separable Gaussian blur (`sigma` scaled with resolution) for carved V/U-groove wall profile
    let sigma = (0.0115 * (size as f64)).max(0.85);
    let carve = gaussian_blur_2d(&ink, size as usize, size as usize, sigma);

    // Inner cut-wall ambient shadow: upper-left stone rim overhangs the recessed groove
    let shift = 0.0085 * (size as f64);
    let mut wall_shadow = vec![0.0_f64; n];
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) as usize;
            let cov_here = ink[idx];
            if cov_here > 0.001 {
                let outside_ul = 1.0
                    - sample_bilinear(
                        &ink,
                        size as usize,
                        size as usize,
                        (x as f64) - shift,
                        (y as f64) - shift,
                    );
                wall_shadow[idx] = (cov_here * outside_ul).clamp(0.0, 1.0);
            }
        }
    }
    let wall_shadow_soft =
        gaussian_blur_2d(&wall_shadow, size as usize, size as usize, sigma * 0.65);

    Ok(EngravingFields {
        ink,
        carve,
        wall_shadow: wall_shadow_soft,
    })
}

fn sample_bilinear(grid: &[f64], w: usize, h: usize, x: f64, y: f64) -> f64 {
    let cx = x.clamp(0.0, (w - 1) as f64);
    let cy = y.clamp(0.0, (h - 1) as f64);
    let x0 = cx.floor() as usize;
    let y0 = cy.floor() as usize;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = cx - (x0 as f64);
    let fy = cy - (y0 as f64);

    let v00 = grid[y0 * w + x0];
    let v10 = grid[y0 * w + x1];
    let v01 = grid[y1 * w + x0];
    let v11 = grid[y1 * w + x1];
    let vx0 = v00 + (v10 - v00) * fx;
    let vx1 = v01 + (v11 - v01) * fx;
    vx0 + (vx1 - vx0) * fy
}

fn gaussian_blur_2d(src: &[f64], w: usize, h: usize, sigma: f64) -> Vec<f64> {
    let radius = (sigma * 3.0).ceil() as isize;
    let mut kernel = Vec::with_capacity((2 * radius + 1) as usize);
    let mut ksum = 0.0;
    for k in -radius..=radius {
        let wgt = (-((k as f64) * (k as f64)) / (2.0 * sigma * sigma)).exp();
        kernel.push(wgt);
        ksum += wgt;
    }
    for wgt in &mut kernel {
        *wgt /= ksum;
    }

    let mut tmp = vec![0.0_f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (ki, k) in (-radius..=radius).enumerate() {
                let sx = (x as isize + k).clamp(0, (w - 1) as isize) as usize;
                acc += src[y * w + sx] * kernel[ki];
            }
            tmp[y * w + x] = acc;
        }
    }

    let mut dst = vec![0.0_f64; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (ki, k) in (-radius..=radius).enumerate() {
                let sy = (y as isize + k).clamp(0, (h - 1) as isize) as usize;
                acc += tmp[sy * w + x] * kernel[ki];
            }
            dst[y * w + x] = acc;
        }
    }
    dst
}

/// Evaluates normalized disc profile height `H(r_norm)` for `r_norm = r / r_outer` in `[0.0, 1.0]`.
#[inline]
fn disc_profile_height(material: MaterialFamily, r_norm: f64) -> f64 {
    let rn = r_norm.clamp(0.0, 1.0);
    // Smooth museum-style rounded peripheral bevel from `0.82..1.00`, gently domed inner table
    let bevel_start = 0.82;
    let main_body = if rn <= bevel_start {
        let u = rn / bevel_start;
        // Subtle convex dome (`+0.04` at center -> `0.0` at shoulder)
        0.26 + 0.035 * (1.0 - u * u)
    } else {
        let t = (rn - bevel_start) / (1.0 - bevel_start);
        // Quarter-elliptical smooth bevel roll-off
        0.26 * (1.0 - t * t).max(0.0).sqrt()
    };

    // Subtle turned lathe accent groove on hardwood boxwood discs (strictly outside glyph area)
    let lathe_groove = if material == MaterialFamily::Boxwood {
        let d = (rn - 0.795) / 0.018;
        -0.016 * (-d * d).exp()
    } else {
        0.0
    };

    main_body + lathe_groove
}

/// Renders a single piece or unengraved disc sprite at `size × size` in straight-alpha RGBA8.
pub fn render_piece_sprite(
    material: MaterialFamily,
    glyph: Option<&GlyphEntry>,
    size: u32,
) -> Result<Vec<u8>, PipelineError> {
    let w = size as usize;
    let h = size as usize;
    let center = 0.5 * (size as f64);
    let r_outer = DISC_RADIUS_FRAC * (size as f64);

    let seed = match glyph {
        Some(g) => splitmix64(
            material.seed_salt()
                ^ (g.role.seed_index() * 0x9E37_79B9)
                ^ ((g.side as u64 + 1) * 0x517C_C1B7),
        ),
        None => splitmix64(material.seed_salt() ^ 0xD15C_BA5E),
    };

    let EngravingFields {
        ink: ink_field,
        carve: carve_field,
        wall_shadow: wall_shadow_field,
    } = match glyph {
        Some(g) => build_engraving_fields(&g.svg_path, size)?,
        None => EngravingFields {
            ink: vec![0.0; w * h],
            carve: vec![0.0; w * h],
            wall_shadow: vec![0.0; w * h],
        },
    };

    // Precompute heightfield on `w × h` grid (clamping radial distance to `r_outer` outside edge
    // so central-difference normals at boundary pixels stay clean and halo-free).
    let mut height_grid = vec![0.0_f64; w * h];
    let carve_depth = 0.085;
    for y in 0..h {
        for x in 0..w {
            let px = (x as f64 + 0.5) - center;
            let py = (y as f64 + 0.5) - center;
            let r_norm = ((px * px + py * py).sqrt() / r_outer).min(1.0);
            let nx = px / r_outer;
            let ny = py / r_outer;

            let micro_bump = match material {
                MaterialFamily::PaleJade => 0.0045 * nephrite_cloud(nx, ny, seed),
                MaterialFamily::Boxwood => 0.0065 * boxwood_grain(nx, ny, seed),
                MaterialFamily::IvoryStone => 0.0040 * ivory_stone_vein(nx, ny, seed),
            };

            let h_val = disc_profile_height(material, r_norm)
                - carve_depth * carve_field[y * w + x]
                + micro_bump * (1.0 - 0.6 * ink_field[y * w + x]);
            height_grid[y * w + x] = h_val;
        }
    }

    // Studio key light vector: upper-left (`azimuth = -45°`, `elevation = 52°`) in `(x_right, y_down, z_out)`
    let elev = KEY_LIGHT_ELEVATION_DEG.to_radians();
    let lx = -std::f64::consts::FRAC_1_SQRT_2 * elev.cos();
    let ly = -std::f64::consts::FRAC_1_SQRT_2 * elev.cos();
    let lz = elev.sin();
    // Half-vector `H = normalize(L + V)` with orthographic view `V = (0, 0, 1)`
    let h_raw = [lx, ly, lz + 1.0];
    let h_len = (h_raw[0] * h_raw[0] + h_raw[1] * h_raw[1] + h_raw[2] * h_raw[2]).sqrt();
    let half_vec = [h_raw[0] / h_len, h_raw[1] / h_len, h_raw[2] / h_len];
    let n_flat_dot_l = lz; // `dot((0, 0, 1), L)` used to anchor flat-surface albedo to exact palette token

    let base_rgb = hex_to_linear_rgb(material.base_hex());
    let deep_jade_rgb = hex_to_linear_rgb("#415447");
    let boxwood_dark_rgb = hex_to_linear_rgb("#9E7747");
    let ivory_shade_rgb = hex_to_linear_rgb("#B6AD9B");
    let red_ink_rgb = hex_to_linear_rgb(material.red_ink_hex());
    let black_ink_rgb = hex_to_linear_rgb(material.black_ink_hex());

    let ink_target_rgb = match glyph.map(|g| g.side) {
        Some(PieceSide::Red) => red_ink_rgb,
        Some(PieceSide::Black) | None => black_ink_rgb,
    };

    let (shininess, spec_strength) = match material {
        MaterialFamily::PaleJade => (28.0, 0.145),
        MaterialFamily::Boxwood => (22.0, 0.105),
        MaterialFamily::IvoryStone => (14.0, 0.055),
    };

    let normal_xy_scale = 0.5 * r_outer;
    let mut rgba = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            // 4×4 supersampled analytical disc coverage for crisp anti-aliased silhouette
            let mut cov_hits = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let sub_x = (x as f64) + ((sx as f64) + 0.5) * 0.25 - center;
                    let sub_y = (y as f64) + ((sy as f64) + 0.5) * 0.25 - center;
                    if sub_x * sub_x + sub_y * sub_y <= r_outer * r_outer {
                        cov_hits += 1;
                    }
                }
            }
            if cov_hits == 0 {
                continue;
            }
            let alpha = (cov_hits as f64) / 16.0;

            let px = (x as f64 + 0.5) - center;
            let py = (y as f64 + 0.5) - center;
            let r_norm = ((px * px + py * py).sqrt() / r_outer).min(1.0);
            let nx_coord = px / r_outer;
            let ny_coord = py / r_outer;

            // Central-difference surface normal from heightfield
            let xm = x.saturating_sub(1);
            let xp = (x + 1).min(w - 1);
            let ym = y.saturating_sub(1);
            let yp = (y + 1).min(h - 1);
            let dh_dx =
                (height_grid[y * w + xp] - height_grid[y * w + xm]) / ((xp - xm).max(1) as f64);
            let dh_dy =
                (height_grid[yp * w + x] - height_grid[ym * w + x]) / ((yp - ym).max(1) as f64);

            let raw_nx = -dh_dx * normal_xy_scale;
            let raw_ny = -dh_dy * normal_xy_scale;
            let raw_nz = 1.0;
            let n_len = (raw_nx * raw_nx + raw_ny * raw_ny + raw_nz * raw_nz).sqrt();
            let n = [raw_nx / n_len, raw_ny / n_len, raw_nz / n_len];

            // Procedural material body albedo in linear RGB
            let stone_albedo = match material {
                MaterialFamily::PaleJade => {
                    let cloud = nephrite_cloud(nx_coord, ny_coord, seed);
                    // Cloudy celadon mottling + subtle translucent thin-edge warmth on upper-left bevel
                    let mottled = [
                        (base_rgb[0] * (1.0 + 0.095 * cloud)).clamp(0.0, 1.0),
                        (base_rgb[1] * (1.0 + 0.082 * cloud)).clamp(0.0, 1.0),
                        (base_rgb[2] * (1.0 + 0.110 * cloud)).clamp(0.0, 1.0),
                    ];
                    let vein_tint = ((-cloud).max(0.0)) * 0.075;
                    let veined = lerp3(mottled, deep_jade_rgb, vein_tint);
                    let rim_t = smoothstep(0.78, 1.0, r_norm);
                    let lr_shade = ((nx_coord + ny_coord) * 0.5).clamp(0.0, 1.0);
                    lerp3(veined, deep_jade_rgb, rim_t * (0.11 + 0.17 * lr_shade))
                }
                MaterialFamily::Boxwood => {
                    let grain = boxwood_grain(nx_coord, ny_coord, seed);
                    let t = (0.5 + 0.5 * grain).clamp(0.0, 1.0);
                    let body = lerp3(base_rgb, boxwood_dark_rgb, 0.22 * t);
                    let rim_t = smoothstep(0.80, 1.0, r_norm);
                    lerp3(body, boxwood_dark_rgb, 0.16 * rim_t)
                }
                MaterialFamily::IvoryStone => {
                    let vein = ivory_stone_vein(nx_coord, ny_coord, seed);
                    let body = [
                        (base_rgb[0] * (1.0 + 0.042 * vein)).clamp(0.0, 1.0),
                        (base_rgb[1] * (1.0 + 0.044 * vein)).clamp(0.0, 1.0),
                        (base_rgb[2] * (1.0 + 0.050 * vein)).clamp(0.0, 1.0),
                    ];
                    let rim_t = smoothstep(0.80, 1.0, r_norm);
                    lerp3(body, ivory_shade_rgb, 0.15 * rim_t)
                }
            };

            let idx = y * w + x;
            let ink_cov = ink_field[idx];
            let wall_ao = wall_shadow_field[idx];

            // Lambertian + hemisphere studio illumination anchored so flat center matches token hex
            let n_dot_l = (n[0] * lx + n[1] * ly + n[2] * lz).max(0.0);
            let ambient = 0.46;
            let diffuse_gain = (1.0 - ambient) / n_flat_dot_l;
            let studio_light = ambient + diffuse_gain * n_dot_l;

            // Blinn-Phong satin sheen (suppressed inside matte pigment engraving)
            let n_dot_h = (n[0] * half_vec[0] + n[1] * half_vec[1] + n[2] * half_vec[2]).max(0.0);
            let spec = spec_strength * (1.0 - 0.88 * ink_cov) * n_dot_h.powf(shininess);

            //Subtle subsurface scattering warmth on upper-left pale-jade bevel crest
            let sss = if material == MaterialFamily::PaleJade {
                let ul = (-(nx_coord + ny_coord) * 0.5).clamp(0.0, 1.0);
                0.032 * smoothstep(0.72, 0.94, r_norm) * ul * (1.0 - ink_cov)
            } else {
                0.0
            };

            let lit_stone = [
                stone_albedo[0] * studio_light + spec + sss,
                stone_albedo[1] * studio_light + spec + sss * 1.04,
                stone_albedo[2] * studio_light + spec * 0.96 + sss * 0.88,
            ];

            // Matte pigment infill inside recessed engraving with inner cut-wall occlusion
            let ink_micro =
                1.0 + 0.035 * nephrite_cloud(nx_coord * 3.0, ny_coord * 3.0, seed ^ 0x7777);
            let ink_shade =
                (0.94 - 0.28 * wall_ao + 0.08 * (n_dot_l - n_flat_dot_l)).clamp(0.58, 1.05);
            let lit_ink = [
                ink_target_rgb[0] * ink_shade * ink_micro,
                ink_target_rgb[1] * ink_shade * ink_micro,
                ink_target_rgb[2] * ink_shade * ink_micro,
            ];

            let final_lin = lerp3(lit_stone, lit_ink, ink_cov);

            let out_idx = idx * 4;
            rgba[out_idx] = linear_to_srgb_u8(final_lin[0]);
            rgba[out_idx + 1] = linear_to_srgb_u8(final_lin[1]);
            rgba[out_idx + 2] = linear_to_srgb_u8(final_lin[2]);
            rgba[out_idx + 3] = (alpha * 255.0).round().clamp(1.0, 255.0) as u8;
        }
    }

    Ok(rgba)
}

/// Renders the shared soft orthographic contact shadow (`contact-shadow.png` / `@2x.png`)
/// shifted slightly toward lower-right (`azimuth = -45°` key light) with straight RGB `(0, 0, 0)`.
pub fn render_contact_shadow(size: u32) -> Vec<u8> {
    let w = size as usize;
    let h = size as usize;
    let center_x = (0.5 + 0.021) * (size as f64);
    let center_y = (0.5 + 0.029) * (size as f64);
    let r_disc = DISC_RADIUS_FRAC * (size as f64);
    let r_max = 0.468 * (size as f64);

    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let dx = (x as f64 + 0.5) - center_x;
            let dy = (y as f64 + 0.5) - center_y;
            let r = (dx * dx + dy * dy).sqrt();
            if r >= r_max {
                continue;
            }
            // Soft penumbra peaking under the piece rim and tapering smoothly to exact 0 before `r_max`
            let core = 1.0 - smoothstep(r_disc * 0.72, r_max, r);
            let edge_taper = 1.0 - smoothstep(r_max * 0.88, r_max, r);
            let alpha = (0.34 * core * edge_taper).clamp(0.0, 1.0);
            let a_u8 = (alpha * 255.0).round() as u8;
            if a_u8 > 0 {
                let idx = (y * w + x) * 4;
                // Straight RGB = pure black (0, 0, 0) so no halo occurs on either warm paper or dark slate
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = a_u8;
            }
        }
    }
    rgba
}

/// Renders a seamless `256 × 256` tileable board surface grain texture (`paper-grain` or `dark-grain`).
pub fn render_surface_tile(is_dark: bool, size: u32) -> Vec<u8> {
    let w = size as usize;
    let h = size as usize;
    let base = if is_dark {
        hex_to_linear_rgb("#1D2421")
    } else {
        hex_to_linear_rgb("#F3EEE4")
    };
    let seed = if is_dark {
        0xDA24_1D24_2026
    } else {
        0xF3EE_E400_2026
    };
    let mut rgba = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            let u = (x as f64) / (w as f64);
            let v = (y as f64) / (h as f64);
            let macro_grain = periodic_fbm_2d(u, v, 6, 4, 0.52, seed);
            let fine_fiber = periodic_fbm_2d(u, v, 24, 3, 0.50, seed ^ 0x55AA);
            let delta = if is_dark {
                0.035 * macro_grain + 0.020 * fine_fiber
            } else {
                0.022 * macro_grain + 0.014 * fine_fiber
            };

            let rgb = [
                (base[0] * (1.0 + delta)).clamp(0.0, 1.0),
                (base[1] * (1.0 + delta)).clamp(0.0, 1.0),
                (base[2] * (1.0 + delta * 1.05)).clamp(0.0, 1.0),
            ];
            let idx = (y * w + x) * 4;
            rgba[idx] = linear_to_srgb_u8(rgb[0]);
            rgba[idx + 1] = linear_to_srgb_u8(rgb[1]);
            rgba[idx + 2] = linear_to_srgb_u8(rgb[2]);
            rgba[idx + 3] = 255;
        }
    }
    rgba
}

pub fn generate_all_materials(
    repo_root: &Path,
    glyph_manifest: &GlyphManifest,
) -> Result<Vec<GeneratedPng>, PipelineError> {
    let mut out = Vec::new();
    let mat_root = repo_root.join("assets/board/materials");
    fs::create_dir_all(&mat_root)?;

    // 1. Shared contact shadow (`@1x` and `@2x`)
    for (scale, size, suffix) in [(1u32, SPRITE_SIZE_1X, ""), (2u32, SPRITE_SIZE_2X, "@2x")] {
        let fname = format!("contact-shadow{suffix}.png");
        let abs = mat_root.join(&fname);
        let rgba = render_contact_shadow(size);
        write_rgba_png(&abs, size, size, &rgba)?;
        out.push(GeneratedPng {
            asset_id: format!("board.shadow.contact.{}x", scale),
            kind: "board-shadow",
            rel_path: format!("assets/board/materials/{fname}"),
            abs_path: abs,
            width: size,
            height: size,
            scale_factor: scale,
            material_family: None,
            glyph: None,
            has_engraving: false,
            is_shadow: true,
            is_surface_tile: false,
        });
    }

    // 2. Seamless board surface tiles (`paper-grain.png` and `dark-grain.png`)
    for (slug, is_dark) in [("paper-grain", false), ("dark-grain", true)] {
        let fname = format!("{slug}.png");
        let abs = mat_root.join(&fname);
        let rgba = render_surface_tile(is_dark, TILE_SIZE);
        write_rgba_png(&abs, TILE_SIZE, TILE_SIZE, &rgba)?;
        out.push(GeneratedPng {
            asset_id: format!("board.surface.{slug}"),
            kind: "board-surface",
            rel_path: format!("assets/board/materials/{fname}"),
            abs_path: abs,
            width: TILE_SIZE,
            height: TILE_SIZE,
            scale_factor: 1,
            material_family: None,
            glyph: None,
            has_engraving: false,
            is_shadow: false,
            is_surface_tile: true,
        });
    }

    // 3. Per-material base discs + 14-piece engraved sprites across both Simplified and Traditional sets
    for material in MaterialFamily::ALL {
        let fam_dir = mat_root.join(material.as_str());
        fs::create_dir_all(&fam_dir)?;

        for (scale, size, suffix) in [(1u32, SPRITE_SIZE_1X, ""), (2u32, SPRITE_SIZE_2X, "@2x")] {
            let fname = format!("disc-base{suffix}.png");
            let abs = fam_dir.join(&fname);
            let rgba = render_piece_sprite(material, None, size)?;
            write_rgba_png(&abs, size, size, &rgba)?;
            out.push(GeneratedPng {
                asset_id: format!("board.material.{}.disc-base.{}x", material.as_str(), scale),
                kind: "board-disc",
                rel_path: format!("assets/board/materials/{}/{fname}", material.as_str()),
                abs_path: abs,
                width: size,
                height: size,
                scale_factor: scale,
                material_family: Some(material),
                glyph: None,
                has_engraving: false,
                is_shadow: false,
                is_surface_tile: false,
            });
        }

        for set_id in GlyphSetId::ALL {
            let script_dir = fam_dir.join(set_id.short_dir());
            fs::create_dir_all(&script_dir)?;
            for side in PieceSide::ALL {
                for role in PieceRole::ALL {
                    let glyph = glyph_manifest
                        .find(set_id, side, role)
                        .ok_or_else(|| {
                            PipelineError::Validation(format!(
                                "glyph missing for {}.{}.{}",
                                set_id.as_str(),
                                side.as_str(),
                                role.as_str()
                            ))
                        })?
                        .clone();

                    for (scale, size, suffix) in
                        [(1u32, SPRITE_SIZE_1X, ""), (2u32, SPRITE_SIZE_2X, "@2x")]
                    {
                        let fname = format!("{}-{}{suffix}.png", side.as_str(), role.as_str());
                        let abs = script_dir.join(&fname);
                        let rgba = render_piece_sprite(material, Some(&glyph), size)?;
                        write_rgba_png(&abs, size, size, &rgba)?;
                        out.push(GeneratedPng {
                            asset_id: format!(
                                "board.piece.{}.{}.{}.{}.{}x",
                                material.as_str(),
                                set_id.short_dir(),
                                side.as_str(),
                                role.as_str(),
                                scale
                            ),
                            kind: "board-piece",
                            rel_path: format!(
                                "assets/board/materials/{}/{}/{fname}",
                                material.as_str(),
                                set_id.short_dir()
                            ),
                            abs_path: abs,
                            width: size,
                            height: size,
                            scale_factor: scale,
                            material_family: Some(material),
                            glyph: Some(glyph.clone()),
                            has_engraving: true,
                            is_shadow: false,
                            is_surface_tile: false,
                        });
                    }
                }
            }
        }
    }

    Ok(out)
}

/// Decodes a PNG file from disk into `(width, height, color_type, rgba_or_rgb_bytes)`.
pub fn decode_png_info(path: &Path) -> Result<(u32, u32, png::ColorType, Vec<u8>), PipelineError> {
    let file = fs::File::open(path)?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().map_err(|e| {
        PipelineError::Validation(format!("{}: PNG decode failed: {e}", path.display()))
    })?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| {
        PipelineError::Validation(format!("{}: PNG frame failed: {e}", path.display()))
    })?;
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, info.color_type, buf))
}
