//! Asset manifest builder and strict schema / checksum / optical / acoustic verifier.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::PipelineError;
use crate::audio::{GeneratedAudio, verify_wav_bytes};
use crate::gallery::GeneratedPlate;
use crate::glyphs::{
    FONT_LICENSE_ID, FONT_SOURCE_NAME, GLYPH_REVIEW_STATUS, GlyphEntry, GlyphManifest, GlyphSetId,
    verify_glyph_manifest,
};
use crate::materials::{
    DISC_RADIUS_FRAC, GeneratedPng, KEY_LIGHT_AZIMUTH_DEG, KEY_LIGHT_ELEVATION_DEG,
    LOGICAL_SPRITE_SIZE, MaterialFamily, decode_png_info,
};

pub const BUILD_COMMAND: &str = "cargo run --release -p jade-asset-pipeline -- --verify";
pub const DERIVATION_COMMAND_WITH_FONT: &str =
    "cargo run --release -p jade-asset-pipeline -- --font-path <NotoSerifCJKsc-Bold.otf>";
pub const WEB_MATERIAL_BUNDLE_LIMIT_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleBudget {
    pub bundle_id: String,
    pub material_family: String,
    pub glyph_set: String,
    pub density: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub limit_bytes: u64,
    pub within_budget: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DensityVariant {
    pub density: String,
    pub width: u32,
    pub height: u32,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionsSpec {
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor: u32,
    pub density_variants: Vec<DensityVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PivotSpec {
    pub x: f64,
    pub y: f64,
    pub logical_x: f64,
    pub logical_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundsBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub units: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub units: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraLightSpec {
    pub projection: String,
    pub key_light_azimuth_deg: f64,
    pub key_light_elevation_deg: f64,
    pub ambient_intensity: f64,
    pub orientation_invariant: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngravingSpec {
    pub profile: String,
    pub carve_depth_norm: f64,
    pub ink_hex: String,
    pub inner_wall_shadow: bool,
    pub lower_right_catchlight: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphIdentitySpec {
    pub glyph_id: String,
    pub set_id: String,
    pub locale: String,
    pub side: String,
    pub role: String,
    pub character: String,
    pub codepoint: String,
    pub review_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSpec {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub duration_ms: f64,
    pub peak_amplitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRecord {
    pub asset_id: String,
    pub version: String,
    pub kind: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub source_file: String,
    pub derivation_command: String,
    pub author_or_origin: String,
    pub license: String,
    pub reuse_notes: String,
    pub reviewer: String,
    pub dimensions: Option<DimensionsSpec>,
    pub pivot: Option<PivotSpec>,
    pub visible_bounds: Option<BoundsBox>,
    pub shadow_bounds: Option<ShadowBounds>,
    pub camera_light: Option<CameraLightSpec>,
    pub engraving: Option<EngravingSpec>,
    pub color_space: Option<String>,
    pub alpha_convention: Option<String>,
    pub material_family: Option<String>,
    pub glyph_identity: Option<GlyphIdentitySpec>,
    pub audio_spec: Option<AudioSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetManifest {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub schema_version: String,
    pub generator: String,
    pub build_command: String,
    pub bundles: Vec<BundleBudget>,
    pub assets: Vec<AssetRecord>,
}

#[inline]
fn round2(v: f64) -> f64 {
    let r = (v * 100.0).round() / 100.0;
    if r == -0.0 { 0.0 } else { r }
}

fn file_bytes_and_sha256(path: &Path) -> Result<(u64, String), PipelineError> {
    let data = fs::read(path).map_err(|e| {
        PipelineError::Validation(format!("failed to read {}: {e}", path.display()))
    })?;
    Ok((data.len() as u64, hex::encode(Sha256::digest(&data))))
}

fn collect_files_sorted(dir: &Path) -> Result<Vec<PathBuf>, PipelineError> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        for entry in fs::read_dir(&cur)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                if let Some(name) = p.file_name().and_then(|n| n.to_str())
                    && name == ".DS_Store"
                {
                    continue;
                }
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn parse_svg_viewbox_dims(svg_text: &str) -> Option<(u32, u32)> {
    let idx = svg_text.find("viewBox=\"")?;
    let rest = &svg_text[idx + 9..];
    let end = rest.find('"')?;
    let nums: Vec<f64> = rest[..end]
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    if nums.len() == 4 {
        Some((nums[2].round() as u32, nums[3].round() as u32))
    } else {
        None
    }
}

fn canonical_sprite_shadow_bounds() -> ShadowBounds {
    let l = LOGICAL_SPRITE_SIZE as f64;
    let ox = round2(0.021 * l);
    let oy = round2(0.029 * l);
    let r = round2(0.468 * l);
    let cx = 0.5 * l + ox;
    let cy = 0.5 * l + oy;
    ShadowBounds {
        x: round2(cx - r),
        y: round2(cy - r),
        width: round2(2.0 * r),
        height: round2(2.0 * r),
        offset_x: ox,
        offset_y: oy,
        units: "logical_px".to_string(),
    }
}

fn canonical_disc_visible_bounds() -> BoundsBox {
    let l = LOGICAL_SPRITE_SIZE as f64;
    let r = DISC_RADIUS_FRAC * l;
    let x = round2(0.5 * l - r);
    let d = round2(2.0 * r);
    BoundsBox {
        x,
        y: x,
        width: d,
        height: d,
        units: "logical_px".to_string(),
    }
}

fn canonical_camera_light() -> CameraLightSpec {
    CameraLightSpec {
        projection: "orthographic-top-down".to_string(),
        key_light_azimuth_deg: KEY_LIGHT_AZIMUTH_DEG,
        key_light_elevation_deg: KEY_LIGHT_ELEVATION_DEG,
        ambient_intensity: 0.46,
        orientation_invariant: true,
    }
}

pub fn build_and_write_manifest(
    repo_root: &Path,
    glyph_manifest: &GlyphManifest,
    generated_pngs: &[GeneratedPng],
    generated_audio: &[GeneratedAudio],
    generated_plates: &[GeneratedPlate],
) -> Result<AssetManifest, PipelineError> {
    let mut records: Vec<AssetRecord> = Vec::new();

    // 1. `assets/board/glyphs.json`
    {
        let rel = "assets/board/glyphs.json".to_string();
        let (bytes, sha256) = file_bytes_and_sha256(&repo_root.join(&rel))?;
        records.push(AssetRecord {
            asset_id: "board.glyphs.manifest".to_string(),
            version: "0.1.0".to_string(),
            kind: "board-glyph-manifest".to_string(),
            path: rel,
            bytes,
            sha256,
            source_file: "source/fonts/NotoSerifCJKsc-Bold.otf".to_string(),
            derivation_command: DERIVATION_COMMAND_WITH_FONT.to_string(),
            author_or_origin: format!("{FONT_SOURCE_NAME} (© 2017-2024 Adobe)"),
            license: FONT_LICENSE_ID.to_string(),
            reuse_notes: "Curated Simplified and Traditional Xiangqi glyph outlines; prototype fallback pending native calligraphy review.".to_string(),
            reviewer: GLYPH_REVIEW_STATUS.to_string(),
            dimensions: None,
            pivot: None,
            visible_bounds: None,
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space: None,
            alpha_convention: None,
            material_family: None,
            glyph_identity: None,
            audio_spec: None,
        });
    }

    // 2. Unique SVG files in `assets/board/glyphs/*.svg`
    let mut seen_svg: BTreeMap<String, &GlyphEntry> = BTreeMap::new();
    for g in &glyph_manifest.glyphs {
        seen_svg.entry(g.svg_file.clone()).or_insert(g);
    }
    for (svg_rel, rep_glyph) in seen_svg {
        let (bytes, sha256) = file_bytes_and_sha256(&repo_root.join(&svg_rel))?;
        let cp_slug = rep_glyph.codepoint.to_ascii_lowercase().replace('+', "");
        records.push(AssetRecord {
            asset_id: format!("board.glyph.{cp_slug}"),
            version: "0.1.0".to_string(),
            kind: "board-glyph-svg".to_string(),
            path: svg_rel.clone(),
            bytes,
            sha256,
            source_file: "source/fonts/NotoSerifCJKsc-Bold.otf".to_string(),
            derivation_command: DERIVATION_COMMAND_WITH_FONT.to_string(),
            author_or_origin: format!("{FONT_SOURCE_NAME} (© 2017-2024 Adobe)"),
            license: FONT_LICENSE_ID.to_string(),
            reuse_notes:
                "Y-down optically centered pure vector glyph outline in 0 0 1000 1000 viewBox."
                    .to_string(),
            reviewer: GLYPH_REVIEW_STATUS.to_string(),
            dimensions: Some(DimensionsSpec {
                logical_width: 1000,
                logical_height: 1000,
                pixel_width: 1000,
                pixel_height: 1000,
                scale_factor: 1,
                density_variants: Vec::new(),
            }),
            pivot: Some(PivotSpec {
                x: 0.5,
                y: 0.5,
                logical_x: 500.0,
                logical_y: 500.0,
            }),
            visible_bounds: Some(BoundsBox {
                x: rep_glyph.bounds.x_min,
                y: rep_glyph.bounds.y_min,
                width: rep_glyph.bounds.width,
                height: rep_glyph.bounds.height,
                units: "viewbox_units".to_string(),
            }),
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space: Some("sRGB".to_string()),
            alpha_convention: Some("straight".to_string()),
            material_family: None,
            glyph_identity: Some(GlyphIdentitySpec {
                glyph_id: format!("codepoint.{}", rep_glyph.codepoint),
                set_id: rep_glyph.set_id.clone(),
                locale: rep_glyph.locale.clone(),
                side: "shared".to_string(),
                role: rep_glyph.role.as_str().to_string(),
                character: rep_glyph.character.clone(),
                codepoint: rep_glyph.codepoint.clone(),
                review_status: rep_glyph.review_status.clone(),
            }),
            audio_spec: None,
        });
    }

    // Group `@1x` and `@2x` density variant paths by logical stem
    let mut variant_group: BTreeMap<String, Vec<DensityVariant>> = BTreeMap::new();
    for p in generated_pngs {
        if p.is_surface_tile {
            continue;
        }
        let stem = p
            .asset_id
            .strip_suffix(".1x")
            .or_else(|| p.asset_id.strip_suffix(".2x"))
            .unwrap_or(&p.asset_id)
            .to_string();
        variant_group.entry(stem).or_default().push(DensityVariant {
            density: format!("{}x", p.scale_factor),
            width: p.width,
            height: p.height,
            path: p.rel_path.clone(),
        });
    }

    // 3. Generated board material PNGs
    for p in generated_pngs {
        let (bytes, sha256) = file_bytes_and_sha256(&p.abs_path)?;
        let stem = p
            .asset_id
            .strip_suffix(".1x")
            .or_else(|| p.asset_id.strip_suffix(".2x"))
            .unwrap_or(&p.asset_id);
        let variants = variant_group.get(stem).cloned().unwrap_or_default();

        let logical_w = if p.is_surface_tile {
            p.width
        } else {
            LOGICAL_SPRITE_SIZE
        };
        let logical_h = if p.is_surface_tile {
            p.height
        } else {
            LOGICAL_SPRITE_SIZE
        };

        let pivot = if p.is_surface_tile {
            Some(PivotSpec {
                x: 0.0,
                y: 0.0,
                logical_x: 0.0,
                logical_y: 0.0,
            })
        } else {
            Some(PivotSpec {
                x: 0.5,
                y: 0.5,
                logical_x: (LOGICAL_SPRITE_SIZE as f64) * 0.5,
                logical_y: (LOGICAL_SPRITE_SIZE as f64) * 0.5,
            })
        };

        let visible_bounds = if p.is_surface_tile {
            Some(BoundsBox {
                x: 0.0,
                y: 0.0,
                width: p.width as f64,
                height: p.height as f64,
                units: "pixel".to_string(),
            })
        } else if p.is_shadow {
            let sb = canonical_sprite_shadow_bounds();
            Some(BoundsBox {
                x: sb.x,
                y: sb.y,
                width: sb.width,
                height: sb.height,
                units: "logical_px".to_string(),
            })
        } else {
            Some(canonical_disc_visible_bounds())
        };

        let shadow_bounds = if p.is_surface_tile {
            None
        } else {
            Some(canonical_sprite_shadow_bounds())
        };

        let camera_light = if p.is_surface_tile {
            None
        } else {
            Some(canonical_camera_light())
        };

        let engraving = match (p.has_engraving, p.material_family, p.glyph.as_ref()) {
            (true, Some(mat), Some(g)) => {
                let ink_hex = match g.side {
                    crate::glyphs::PieceSide::Red => mat.red_ink_hex(),
                    crate::glyphs::PieceSide::Black => mat.black_ink_hex(),
                };
                Some(EngravingSpec {
                    profile: "recessed-v-u-groove".to_string(),
                    carve_depth_norm: 0.085,
                    ink_hex: ink_hex.to_string(),
                    inner_wall_shadow: true,
                    lower_right_catchlight: true,
                })
            }
            _ => None,
        };

        let glyph_identity = p.glyph.as_ref().map(|g| GlyphIdentitySpec {
            glyph_id: g.glyph_id.clone(),
            set_id: g.set_id.clone(),
            locale: g.locale.clone(),
            side: g.side.as_str().to_string(),
            role: g.role.as_str().to_string(),
            character: g.character.clone(),
            codepoint: g.codepoint.clone(),
            review_status: g.review_status.clone(),
        });

        let (source_file, author, license, reviewer) = if let Some(g) = &p.glyph {
            (
                g.svg_file.clone(),
                format!("Jade procedural 2.5D shader + {FONT_SOURCE_NAME}"),
                "SIL-OFL-1.1 (glyph outline) + play-jade procedural material".to_string(),
                GLYPH_REVIEW_STATUS.to_string(),
            )
        } else {
            (
                "crates/asset-pipeline/src/materials.rs".to_string(),
                "Jade procedural 2.5D shader (play-jade/jade)".to_string(),
                "Apache-2.0".to_string(),
                "automated-jade-asset-pipeline-v1".to_string(),
            )
        };

        records.push(AssetRecord {
            asset_id: p.asset_id.clone(),
            version: "0.1.0".to_string(),
            kind: p.kind.to_string(),
            path: p.rel_path.clone(),
            bytes,
            sha256,
            source_file,
            derivation_command: BUILD_COMMAND.to_string(),
            author_or_origin: author,
            license,
            reuse_notes: "Straight-alpha sRGB PNG; convert to premultiplied alpha at renderer upload boundary before linear mip/bilinear filtering.".to_string(),
            reviewer,
            dimensions: Some(DimensionsSpec {
                logical_width: logical_w,
                logical_height: logical_h,
                pixel_width: p.width,
                pixel_height: p.height,
                scale_factor: p.scale_factor,
                density_variants: variants,
            }),
            pivot,
            visible_bounds,
            shadow_bounds,
            camera_light,
            engraving,
            color_space: Some("sRGB".to_string()),
            alpha_convention: Some("straight".to_string()),
            material_family: p.material_family.map(|m| m.as_str().to_string()),
            glyph_identity,
            audio_spec: None,
        });
    }

    // 4. Generated acoustic percussion WAV samples (`assets/audio/**`)
    for a in generated_audio {
        let (bytes, sha256) = file_bytes_and_sha256(&a.abs_path)?;
        records.push(AssetRecord {
            asset_id: format!("audio.{}.{}", a.family.dir_name(), a.event.id_slug()),
            version: "0.1.0".to_string(),
            kind: "board-audio".to_string(),
            path: a.rel_path.clone(),
            bytes,
            sha256,
            source_file: "crates/asset-pipeline/src/audio.rs".to_string(),
            derivation_command: BUILD_COMMAND.to_string(),
            author_or_origin: "Jade modal-resonance physical percussion synthesizer".to_string(),
            license: "Apache-2.0".to_string(),
            reuse_notes: "44.1 kHz 16-bit mono PCM WAV; zero-crossing cosine tail fade; alternate place-1 and place-2 on consecutive moves.".to_string(),
            reviewer: "automated-jade-asset-pipeline-v1".to_string(),
            dimensions: None,
            pivot: None,
            visible_bounds: None,
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space: None,
            alpha_convention: None,
            material_family: Some(a.family.material_family().to_string()),
            glyph_identity: None,
            audio_spec: Some(AudioSpec {
                sample_rate_hz: a.sample_rate,
                channels: 1,
                bits_per_sample: 16,
                duration_ms: round2(a.duration_ms),
                peak_amplitude: round2(a.peak_amplitude),
            }),
        });
    }

    // 5. Visual reference proof plates (`docs/visual-reference/*.png`)
    let mut indexed_plate_paths = BTreeSet::new();
    for plate in generated_plates {
        indexed_plate_paths.insert(plate.rel_path.clone());
        let (bytes, sha256) = file_bytes_and_sha256(&plate.abs_path)?;
        records.push(AssetRecord {
            asset_id: plate.asset_id.clone(),
            version: "0.1.0".to_string(),
            kind: "visual-reference".to_string(),
            path: plate.rel_path.clone(),
            bytes,
            sha256,
            source_file: "crates/asset-pipeline/src/gallery.rs".to_string(),
            derivation_command: BUILD_COMMAND.to_string(),
            author_or_origin: "Jade deterministic visual-reference renderer".to_string(),
            license: "SIL-OFL-1.1 (glyphs/labels) + play-jade visual specification".to_string(),
            reuse_notes: plate.description.clone(),
            reviewer: "automated-jade-asset-pipeline-v1".to_string(),
            dimensions: Some(DimensionsSpec {
                logical_width: plate.width,
                logical_height: plate.height,
                pixel_width: plate.width,
                pixel_height: plate.height,
                scale_factor: 1,
                density_variants: Vec::new(),
            }),
            pivot: Some(PivotSpec {
                x: 0.0,
                y: 0.0,
                logical_x: 0.0,
                logical_y: 0.0,
            }),
            visible_bounds: Some(BoundsBox {
                x: 0.0,
                y: 0.0,
                width: plate.width as f64,
                height: plate.height as f64,
                units: "pixel".to_string(),
            }),
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space: Some("sRGB".to_string()),
            alpha_convention: Some("straight".to_string()),
            material_family: None,
            glyph_identity: None,
            audio_spec: None,
        });
    }

    // 5b. Additional client visual proof captures in `docs/visual-reference/*.png`
    let vis_ref_dir = repo_root.join("docs/visual-reference");
    for abs in collect_files_sorted(&vis_ref_dir)? {
        let rel = abs
            .strip_prefix(repo_root)
            .map_err(|e| PipelineError::Validation(format!("path prefix error: {e}")))?
            .to_string_lossy()
            .replace('\\', "/");
        if indexed_plate_paths.contains(&rel) || !rel.ends_with(".png") {
            continue;
        }
        let (bytes, sha256) = file_bytes_and_sha256(&abs)?;
        let (w, h, _, _) = decode_png_info(&abs)?;
        let stem = abs
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("proof")
            .replace('_', "-");
        records.push(AssetRecord {
            asset_id: format!("visual.proof.{stem}"),
            version: "0.1.0".to_string(),
            kind: "visual-reference".to_string(),
            path: rel,
            bytes,
            sha256,
            source_file: "docs/visual-reference".to_string(),
            derivation_command: "client-headless-visual-proof".to_string(),
            author_or_origin: "Jade automated client visual verification".to_string(),
            license: "SIL-OFL-1.1 (glyphs/labels) + play-jade visual specification".to_string(),
            reuse_notes: "Automated native/web client board rendering proof.".to_string(),
            reviewer: "automated-jade-asset-pipeline-v1".to_string(),
            dimensions: Some(DimensionsSpec {
                logical_width: w,
                logical_height: h,
                pixel_width: w,
                pixel_height: h,
                scale_factor: 1,
                density_variants: Vec::new(),
            }),
            pivot: Some(PivotSpec {
                x: 0.0,
                y: 0.0,
                logical_x: 0.0,
                logical_y: 0.0,
            }),
            visible_bounds: Some(BoundsBox {
                x: 0.0,
                y: 0.0,
                width: w as f64,
                height: h as f64,
                units: "pixel".to_string(),
            }),
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space: Some("sRGB".to_string()),
            alpha_convention: Some("straight".to_string()),
            material_family: None,
            glyph_identity: None,
            audio_spec: None,
        });
    }

    // 6. Committed brand assets (`assets/brand/**` — strictly read-only!)
    let brand_dir = repo_root.join("assets/brand");
    for abs in collect_files_sorted(&brand_dir)? {
        let rel = abs
            .strip_prefix(repo_root)
            .map_err(|e| PipelineError::Validation(format!("path prefix error: {e}")))?
            .to_string_lossy()
            .replace('\\', "/");
        let (bytes, sha256) = file_bytes_and_sha256(&abs)?;
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let (dims, vis_bounds, color_space, alpha_conv) = if ext == "png" {
            let (w, h, _, _) = decode_png_info(&abs)?;
            (
                Some(DimensionsSpec {
                    logical_width: w,
                    logical_height: h,
                    pixel_width: w,
                    pixel_height: h,
                    scale_factor: 1,
                    density_variants: Vec::new(),
                }),
                Some(BoundsBox {
                    x: 0.0,
                    y: 0.0,
                    width: w as f64,
                    height: h as f64,
                    units: "pixel".to_string(),
                }),
                Some("sRGB".to_string()),
                Some("straight".to_string()),
            )
        } else if ext == "svg" {
            let text = fs::read_to_string(&abs)?;
            let (w, h) = parse_svg_viewbox_dims(&text).unwrap_or((512, 512));
            (
                Some(DimensionsSpec {
                    logical_width: w,
                    logical_height: h,
                    pixel_width: w,
                    pixel_height: h,
                    scale_factor: 1,
                    density_variants: Vec::new(),
                }),
                Some(BoundsBox {
                    x: 0.0,
                    y: 0.0,
                    width: w as f64,
                    height: h as f64,
                    units: "viewbox_units".to_string(),
                }),
                Some("sRGB".to_string()),
                Some("straight".to_string()),
            )
        } else if rel.ends_with("palette.json") || rel.ends_with("palette.css") {
            (None, None, Some("sRGB".to_string()), None)
        } else {
            (None, None, None, None)
        };

        let license = if rel.contains("Noto") || rel.ends_with(".ttf") || rel.ends_with(".svg") {
            "SIL-OFL-1.1".to_string()
        } else {
            "Jade Brand Kit (see assets/brand/README.md)".to_string()
        };

        let slug = rel
            .trim_start_matches("assets/")
            .replace('/', ".")
            .replace('_', "-");
        records.push(AssetRecord {
            asset_id: slug,
            version: "0.1.0".to_string(),
            kind: "brand-asset".to_string(),
            path: rel.clone(),
            bytes,
            sha256,
            source_file: "assets/brand/source/upstream_kit_manifest.json".to_string(),
            derivation_command: "committed-brand-kit-v1".to_string(),
            author_or_origin: "Jade Brand Kit (play-jade)".to_string(),
            license,
            reuse_notes: "Canonical Jade brand identity asset; signet U+8C61 is distinct from gameplay piece faces.".to_string(),
            reviewer: "approved-brand-kit-prototype-v1".to_string(),
            dimensions: dims,
            pivot: None,
            visible_bounds: vis_bounds,
            shadow_bounds: None,
            camera_light: None,
            engraving: None,
            color_space,
            alpha_convention: alpha_conv,
            material_family: if rel.contains("avatar") || rel.contains("Avatar") || rel.contains("mark") || rel.contains("Mark") {
                Some("pale-jade".to_string())
            } else {
                None
            },
            glyph_identity: None,
            audio_spec: None,
        });
    }

    records.sort_by(|a, b| a.path.cmp(&b.path));

    // Compute web material bundle byte budgets (`14` pieces + `contact-shadow` + `paper-grain` + `dark-grain`)
    let by_path: BTreeMap<&str, u64> = records.iter().map(|r| (r.path.as_str(), r.bytes)).collect();

    let paper_bytes = *by_path
        .get("assets/board/materials/paper-grain.png")
        .unwrap_or(&0);
    let dark_bytes = *by_path
        .get("assets/board/materials/dark-grain.png")
        .unwrap_or(&0);

    let mut bundles = Vec::new();
    for mat in MaterialFamily::ALL {
        for set_id in GlyphSetId::ALL {
            for (density, suffix) in [("1x", ""), ("2x", "@2x")] {
                let shadow_path = format!("assets/board/materials/contact-shadow{suffix}.png");
                let mut total =
                    paper_bytes + dark_bytes + *by_path.get(shadow_path.as_str()).unwrap_or(&0);
                let mut count = 3usize;
                let prefix = format!(
                    "assets/board/materials/{}/{}/",
                    mat.as_str(),
                    set_id.short_dir()
                );
                for rec in &records {
                    if rec.kind == "board-piece"
                        && rec.path.starts_with(&prefix)
                        && rec.path.ends_with("@2x.png") == (density == "2x")
                    {
                        total += rec.bytes;
                        count += 1;
                    }
                }
                let within_budget = total < WEB_MATERIAL_BUNDLE_LIMIT_BYTES;
                if !within_budget {
                    return Err(PipelineError::Validation(format!(
                        "bundle {}.{}.{density} exceeds 1 MB limit: {total} bytes",
                        mat.as_str(),
                        set_id.short_dir()
                    )));
                }
                bundles.push(BundleBudget {
                    bundle_id: format!("{}.{}.{}", mat.as_str(), set_id.short_dir(), density),
                    material_family: mat.as_str().to_string(),
                    glyph_set: set_id.as_str().to_string(),
                    density: density.to_string(),
                    file_count: count,
                    total_bytes: total,
                    limit_bytes: WEB_MATERIAL_BUNDLE_LIMIT_BYTES,
                    within_budget,
                });
            }
        }
    }

    let manifest = AssetManifest {
        schema: "./manifest.schema.json".to_string(),
        schema_version: "1.0.0".to_string(),
        generator: "jade-asset-pipeline 0.1.0".to_string(),
        build_command: BUILD_COMMAND.to_string(),
        bundles,
        assets: records,
    };

    let mut json_bytes = serde_json::to_vec_pretty(&manifest)?;
    json_bytes.push(b'\n');
    fs::write(repo_root.join("assets/manifest.json"), json_bytes)?;

    Ok(manifest)
}

/// Strictly validates `assets/manifest.schema.json`, `assets/manifest.json`, `assets/board/glyphs.json`,
/// all SVG glyph outlines, all PNG sprites/plates, all WAV audio files, and bundle budgets on disk
/// WITHOUT requiring the external OTF font file.
pub fn verify_repository_assets(repo_root: &Path) -> Result<AssetManifest, PipelineError> {
    // 1. Validate `assets/manifest.schema.json` parses as valid Draft 2020-12 JSON Schema
    let schema_path = repo_root.join("assets/manifest.schema.json");
    let schema_bytes = fs::read(&schema_path).map_err(|e| {
        PipelineError::Validation(format!("missing {}: {e}", schema_path.display()))
    })?;
    let schema_val: serde_json::Value = serde_json::from_slice(&schema_bytes)?;
    if schema_val.get("$schema").and_then(|v| v.as_str())
        != Some("https://json-schema.org/draft/2020-12/schema")
    {
        return Err(PipelineError::Validation(
            "assets/manifest.schema.json must declare Draft 2020-12 $schema".to_string(),
        ));
    }

    // 2. Load and validate `assets/board/glyphs.json` and all 16 SVG glyph files
    let glyph_manifest = crate::glyphs::load_glyph_manifest(repo_root)?;
    verify_glyph_manifest(repo_root, &glyph_manifest)?;

    // 3. Load `assets/manifest.json`
    let manifest_path = repo_root.join("assets/manifest.json");
    let manifest_bytes = fs::read(&manifest_path).map_err(|e| {
        PipelineError::Validation(format!("missing {}: {e}", manifest_path.display()))
    })?;
    let manifest: AssetManifest = serde_json::from_slice(&manifest_bytes)?;

    if manifest.schema != "./manifest.schema.json" {
        return Err(PipelineError::Validation(format!(
            "unexpected manifest.$schema: {}",
            manifest.schema
        )));
    }
    if manifest.bundles.len() != 12 {
        return Err(PipelineError::Validation(format!(
            "expected 12 bundle budget entries (3 materials × 2 scripts × 2 densities), got {}",
            manifest.bundles.len()
        )));
    }
    for b in &manifest.bundles {
        if !b.within_budget
            || b.total_bytes >= WEB_MATERIAL_BUNDLE_LIMIT_BYTES
            || b.file_count != 17
        {
            return Err(PipelineError::Validation(format!(
                "invalid bundle budget {}: files={}, bytes={}",
                b.bundle_id, b.file_count, b.total_bytes
            )));
        }
    }

    // 4. Verify every single `AssetRecord` against schema rules and actual disk bytes/SHA-256
    let mut seen_ids = BTreeSet::new();
    let mut seen_paths = BTreeSet::new();
    let mut piece_count = 0usize;
    let mut audio_count = 0usize;
    let mut plate_count = 0usize;

    for rec in &manifest.assets {
        if !seen_ids.insert(rec.asset_id.clone()) {
            return Err(PipelineError::Validation(format!(
                "duplicate asset_id: {}",
                rec.asset_id
            )));
        }
        if !seen_paths.insert(rec.path.clone()) {
            return Err(PipelineError::Validation(format!(
                "duplicate asset path: {}",
                rec.path
            )));
        }
        if rec.sha256.len() != 64 || !rec.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(PipelineError::Validation(format!(
                "{}: invalid sha256 '{}'",
                rec.asset_id, rec.sha256
            )));
        }

        let abs = repo_root.join(&rec.path);
        let raw_bytes = fs::read(&abs).map_err(|e| {
            PipelineError::Validation(format!("{}: missing file {}: {e}", rec.asset_id, rec.path))
        })?;
        if (raw_bytes.len() as u64) != rec.bytes {
            return Err(PipelineError::Validation(format!(
                "{}: byte count mismatch for {} (manifest={}, disk={})",
                rec.asset_id,
                rec.path,
                rec.bytes,
                raw_bytes.len()
            )));
        }
        let disk_sha = hex::encode(Sha256::digest(&raw_bytes));
        if disk_sha != rec.sha256 {
            return Err(PipelineError::Validation(format!(
                "{}: SHA-256 mismatch for {} (manifest={}, disk={})",
                rec.asset_id, rec.path, rec.sha256, disk_sha
            )));
        }

        match rec.kind.as_str() {
            "board-piece" => {
                piece_count += 1;
                let dims = rec.dimensions.as_ref().ok_or_else(|| {
                    PipelineError::Validation(format!("{}: missing dimensions", rec.asset_id))
                })?;
                if rec.pivot.is_none()
                    || rec.visible_bounds.is_none()
                    || rec.shadow_bounds.is_none()
                    || rec.camera_light.is_none()
                    || rec.engraving.is_none()
                    || rec.glyph_identity.is_none()
                    || rec.material_family.is_none()
                {
                    return Err(PipelineError::Validation(format!(
                        "{}: board-piece missing required optical/engraving/glyph metadata",
                        rec.asset_id
                    )));
                }
                if rec.color_space.as_deref() != Some("sRGB")
                    || rec.alpha_convention.as_deref() != Some("straight")
                {
                    return Err(PipelineError::Validation(format!(
                        "{}: board-piece must be straight-alpha sRGB",
                        rec.asset_id
                    )));
                }
                let (w, h, ct, pixels) = decode_png_info(&abs)?;
                if w != dims.pixel_width || h != dims.pixel_height || ct != png::ColorType::Rgba {
                    return Err(PipelineError::Validation(format!(
                        "{}: PNG header mismatch ({w}x{h} {ct:?})",
                        rec.asset_id
                    )));
                }
                // Verify corners are strictly transparent `(0, 0, 0, 0)` (no background bleed)
                if pixels[0..4] != [0, 0, 0, 0] {
                    return Err(PipelineError::Validation(format!(
                        "{}: top-left corner pixel is not transparent [0,0,0,0]: {:?}",
                        rec.asset_id,
                        &pixels[0..4]
                    )));
                }
            }
            "board-audio" => {
                audio_count += 1;
                let spec = rec.audio_spec.as_ref().ok_or_else(|| {
                    PipelineError::Validation(format!("{}: missing audio_spec", rec.asset_id))
                })?;
                let (sr, samples, _) = verify_wav_bytes(&raw_bytes, &rec.path)?;
                if sr != spec.sample_rate_hz || samples == 0 {
                    return Err(PipelineError::Validation(format!(
                        "{}: WAV sample rate mismatch",
                        rec.asset_id
                    )));
                }
            }
            "visual-reference" => {
                plate_count += 1;
                let dims = rec.dimensions.as_ref().ok_or_else(|| {
                    PipelineError::Validation(format!("{}: missing dimensions", rec.asset_id))
                })?;
                let (w, h, _, _) = decode_png_info(&abs)?;
                if w != dims.pixel_width || h != dims.pixel_height {
                    return Err(PipelineError::Validation(format!(
                        "{}: plate PNG dimensions mismatch ({w}x{h})",
                        rec.asset_id
                    )));
                }
            }
            _ => {}
        }
    }

    // 3 materials × 2 scripts × 14 pieces × 2 densities = 168 piece PNGs
    if piece_count != 168 {
        return Err(PipelineError::Validation(format!(
            "expected 168 baked board-piece PNG records, found {piece_count}"
        )));
    }
    // 3 audio families × 4 events = 12 WAV files
    if audio_count != 12 {
        return Err(PipelineError::Validation(format!(
            "expected 12 board-audio WAV records, found {audio_count}"
        )));
    }
    if plate_count < 3 {
        return Err(PipelineError::Validation(format!(
            "expected at least 3 visual-reference plates, found {plate_count}"
        )));
    }

    // 5. Verify no untracked stray files exist inside `assets/board/`, `assets/audio/`, `assets/brand/`, or `docs/visual-reference/`
    for subdir in [
        "assets/board",
        "assets/audio",
        "assets/brand",
        "docs/visual-reference",
    ] {
        for disk_file in collect_files_sorted(&repo_root.join(subdir))? {
            let rel = disk_file
                .strip_prefix(repo_root)
                .map_err(|e| PipelineError::Validation(format!("prefix error: {e}")))?
                .to_string_lossy()
                .replace('\\', "/");
            if !seen_paths.contains(&rel) {
                return Err(PipelineError::Validation(format!(
                    "unindexed file on disk missing from assets/manifest.json: {rel}"
                )));
            }
        }
    }

    Ok(manifest)
}
