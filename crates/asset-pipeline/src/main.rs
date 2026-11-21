use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use jade_asset_pipeline::manifest::verify_repository_assets;
use jade_asset_pipeline::run_build_pipeline;

fn default_repo_root() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("assets/brand").exists() && cwd.join("crates/asset-pipeline").exists() {
        return cwd;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .unwrap_or(cwd)
}

fn print_usage() {
    eprintln!(
        "Usage: jade-asset-pipeline [--repo-root <DIR>] [--font-path <OTF>] [--verify]\n\
         \n\
         Options:\n\
           --repo-root <DIR>   Repository root (defaults to play-jade/jade workspace root)\n\
           --font-path <OTF>   Extract SVG glyph outlines from NotoSerifCJKsc-Bold.otf and bake all assets\n\
           --verify            Verify committed SVGs, PNGs, WAVs, and SHA-256 hashes in assets/manifest.json"
    );
}

fn main() -> ExitCode {
    let mut repo_root: Option<PathBuf> = None;
    let mut font_path: Option<PathBuf> = None;
    let mut verify_only = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo-root" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --repo-root requires a directory path");
                    return ExitCode::FAILURE;
                };
                repo_root = Some(PathBuf::from(val));
            }
            "--font-path" => {
                let Some(val) = args.next() else {
                    eprintln!("error: --font-path requires a path to NotoSerifCJKsc-Bold.otf");
                    return ExitCode::FAILURE;
                };
                font_path = Some(PathBuf::from(val));
            }
            "--verify" => {
                verify_only = true;
            }
            "-h" | "--help" => {
                print_usage();
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("error: unrecognized argument '{other}'");
                print_usage();
                return ExitCode::FAILURE;
            }
        }
    }

    let root = repo_root.unwrap_or_else(default_repo_root);

    if verify_only && font_path.is_none() {
        match verify_repository_assets(&root) {
            Ok(manifest) => {
                println!(
                    "Verified {} assets and {} material bundles in {} (all SHA-256 checksums, SVG paths, PNG headers, and WAV samples valid).",
                    manifest.assets.len(),
                    manifest.bundles.len(),
                    root.display()
                );
                for b in &manifest.bundles {
                    if b.density == "2x" {
                        println!(
                            "  bundle {:<28} {:>7} bytes (< 1,000,000 limit)",
                            b.bundle_id, b.total_bytes
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("Asset verification failed: {err}");
                ExitCode::FAILURE
            }
        }
    } else {
        match run_build_pipeline(&root, font_path.as_deref()) {
            Ok(report) => {
                println!(
                    "Generated and verified Jade assets in {}:\n\
                     - Curated glyph entries: {} ({} unique SVG files)\n\
                     - Baked material PNGs:   {}\n\
                     - Acoustic WAV samples:  {}\n\
                     - Reference plates:      {}\n\
                     - Total manifest items:  {}\n\
                     - Flagship pale-jade @1x bundle: {} bytes\n\
                     - Flagship pale-jade @2x bundle: {} bytes",
                    root.display(),
                    report.glyph_count,
                    report.svg_count,
                    report.png_count,
                    report.audio_count,
                    report.plate_count,
                    report.total_manifest_records,
                    report.flagship_1x_bundle_bytes,
                    report.flagship_2x_bundle_bytes,
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("Asset pipeline failed: {err}");
                ExitCode::FAILURE
            }
        }
    }
}
