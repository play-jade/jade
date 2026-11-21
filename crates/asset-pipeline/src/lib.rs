//! `jade-asset-pipeline`: Deterministic glyph vector extraction, orthographic 2.5D material
//! baking, physical modal percussion audio synthesis, visual reference rendering, and manifest
//! verification for `play-jade/jade`.

pub mod audio;
pub mod gallery;
pub mod glyphs;
pub mod manifest;
pub mod materials;
pub mod noise;

use std::path::Path;

use thiserror::Error;

use crate::audio::generate_all_audio;
use crate::gallery::generate_visual_reference_gallery;
use crate::glyphs::{extract_glyphs_from_font, load_glyph_manifest, verify_glyph_manifest};
use crate::manifest::{AssetManifest, build_and_write_manifest, verify_repository_assets};
use crate::materials::generate_all_materials;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Asset validation failed: {0}")]
    Validation(String),
}

#[derive(Debug, Clone)]
pub struct BuildReport {
    pub glyph_count: usize,
    pub svg_count: usize,
    pub png_count: usize,
    pub audio_count: usize,
    pub plate_count: usize,
    pub total_manifest_records: usize,
    pub flagship_2x_bundle_bytes: u64,
    pub flagship_1x_bundle_bytes: u64,
}

/// Runs the complete deterministic generation pipeline and verifies the resulting repository state.
pub fn run_build_pipeline(
    repo_root: &Path,
    font_path: Option<&Path>,
) -> Result<BuildReport, PipelineError> {
    let glyph_manifest = match font_path {
        Some(fp) => extract_glyphs_from_font(fp, repo_root)?,
        None => load_glyph_manifest(repo_root)?,
    };
    verify_glyph_manifest(repo_root, &glyph_manifest)?;

    let pngs = generate_all_materials(repo_root, &glyph_manifest)?;
    let audio = generate_all_audio(repo_root)?;
    let plates = generate_visual_reference_gallery(repo_root, &glyph_manifest)?;
    let asset_manifest =
        build_and_write_manifest(repo_root, &glyph_manifest, &pngs, &audio, &plates)?;

    // Run full read-only verification pass to guarantee deterministic self-consistency
    let verified: AssetManifest = verify_repository_assets(repo_root)?;

    let svg_count = verified
        .assets
        .iter()
        .filter(|r| r.kind == "board-glyph-svg")
        .count();
    let flagship_2x = verified
        .bundles
        .iter()
        .find(|b| b.bundle_id == "pale-jade.simplified.2x")
        .map(|b| b.total_bytes)
        .unwrap_or(0);
    let flagship_1x = verified
        .bundles
        .iter()
        .find(|b| b.bundle_id == "pale-jade.simplified.1x")
        .map(|b| b.total_bytes)
        .unwrap_or(0);

    Ok(BuildReport {
        glyph_count: glyph_manifest.glyphs.len(),
        svg_count,
        png_count: pngs.len(),
        audio_count: audio.len(),
        plate_count: plates.len(),
        total_manifest_records: asset_manifest.assets.len(),
        flagship_2x_bundle_bytes: flagship_2x,
        flagship_1x_bundle_bytes: flagship_1x,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{
        AudioFamily, SAMPLE_RATE, SoundEvent, encode_wav_mono_16bit, synthesize_samples,
        verify_wav_bytes,
    };
    use crate::glyphs::{GlyphSetId, PieceRole, PieceSide, face_character};
    use crate::materials::{
        MaterialFamily, linear_to_srgb_u8, render_contact_shadow, render_piece_sprite,
        render_surface_tile, srgb_u8_to_linear,
    };
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root")
            .to_path_buf()
    }

    #[test]
    fn curated_xiangqi_character_sets_match_specification() {
        // Simplified set checks
        assert_eq!(
            face_character(
                GlyphSetId::SimplifiedPrototypeV1,
                PieceSide::Red,
                PieceRole::General
            ),
            '帅'
        );
        assert_eq!(
            face_character(
                GlyphSetId::SimplifiedPrototypeV1,
                PieceSide::Black,
                PieceRole::General
            ),
            '将'
        );
        assert_eq!(
            face_character(
                GlyphSetId::SimplifiedPrototypeV1,
                PieceSide::Red,
                PieceRole::Chariot
            ),
            '车'
        );
        assert_eq!(
            face_character(
                GlyphSetId::SimplifiedPrototypeV1,
                PieceSide::Red,
                PieceRole::Horse
            ),
            '马'
        );

        // Traditional curated set checks (explicitly 將 U+5C07 and 砲 U+7832 for Black)
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Red,
                PieceRole::General
            ),
            '帥'
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Black,
                PieceRole::General
            ),
            '將'
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Black,
                PieceRole::General
            ) as u32,
            0x5C07
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Red,
                PieceRole::Cannon
            ),
            '炮'
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Black,
                PieceRole::Cannon
            ),
            '砲'
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Black,
                PieceRole::Chariot
            ),
            '車'
        );
        assert_eq!(
            face_character(
                GlyphSetId::TraditionalPrototypeV1,
                PieceSide::Black,
                PieceRole::Horse
            ),
            '馬'
        );
    }

    #[test]
    fn srgb_linear_roundtrip_is_exact_for_all_8bit_values() {
        for v in 0u8..=255u8 {
            let lin = srgb_u8_to_linear(v);
            let back = linear_to_srgb_u8(lin);
            assert_eq!(v, back, "sRGB -> linear -> sRGB drifted at {v}");
        }
    }

    #[test]
    fn procedural_sprites_and_shadows_are_deterministic_and_halo_free() {
        let a = render_piece_sprite(MaterialFamily::PaleJade, None, 96).expect("render 1");
        let b = render_piece_sprite(MaterialFamily::PaleJade, None, 96).expect("render 2");
        assert_eq!(
            a, b,
            "procedural material baking must be bit-identical across runs"
        );
        assert_eq!(&a[0..4], &[0, 0, 0, 0], "outer corners must be transparent");

        // Every partially transparent boundary pixel must carry true stone RGB (never black [0,0,0])
        for px in a.as_chunks::<4>().0 {
            let alpha = px[3];
            if alpha > 0 && alpha < 255 {
                assert!(
                    px[0] > 90 && px[1] > 100 && px[2] > 85,
                    "fractional-alpha edge pixel {:?} darkened by premultiplication halo",
                    px
                );
            }
        }

        let sh = render_contact_shadow(96);
        assert_eq!(
            &sh[0..4],
            &[0, 0, 0, 0],
            "shadow must not clip at canvas border"
        );
        for px in sh.as_chunks::<4>().0 {
            if px[3] > 0 {
                assert_eq!(
                    &px[0..3],
                    &[0, 0, 0],
                    "shadow straight RGB must be pure black"
                );
            }
        }

        // Seamless tile periodicity check
        let tile = render_surface_tile(false, 64);
        assert_eq!(tile.len(), 64 * 64 * 4);
    }

    #[test]
    fn modal_percussion_audio_meets_duration_and_headroom_contracts() {
        for family in AudioFamily::ALL {
            for event in SoundEvent::ALL {
                let (pcm1, peak1) = synthesize_samples(family, event);
                let (pcm2, _) = synthesize_samples(family, event);
                assert_eq!(pcm1, pcm2, "audio synthesis must be deterministic");
                assert!(peak1 > 0.5 && peak1 < 0.92, "unexpected peak {peak1}");

                let wav = encode_wav_mono_16bit(&pcm1, SAMPLE_RATE);
                let (sr, count, verified_peak) =
                    verify_wav_bytes(&wav, event.file_name()).expect("valid WAV");
                assert_eq!(sr, SAMPLE_RATE);
                assert_eq!(count, pcm1.len());
                assert!(verified_peak < 0.92);
            }
        }
    }

    #[test]
    fn committed_repository_assets_pass_offline_verification() {
        let root = workspace_root();
        if root.join("assets/manifest.json").exists() {
            let manifest = verify_repository_assets(&root)
                .expect("committed assets/manifest.json must pass strict offline verification");
            for b in &manifest.bundles {
                assert!(
                    b.total_bytes < 1_000_000,
                    "bundle {} exceeded 1 MB: {} bytes",
                    b.bundle_id,
                    b.total_bytes
                );
            }
        }
    }
}
