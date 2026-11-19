# Agents — `play-jade/jade` (Public Repository)

This repository (`play-jade/jade`) is the single public monorepo for the Jade Xiangqi game, shared Rust rules/replay core, versioned public contracts, web application, native clients, match server, and approved public art/brand assets.

## Repository Ownership and One-Way Boundary

1. **Public standalone build (`play-jade/jade`)**:
   - Owns `apps/web/`, `apps/ios/`, `apps/macos/`, `packages/apple/`, `apps/android/`, `apps/windows/`, `apps/linux/`, `crates/xiangqi-core/`, `crates/core-wasm/`, `crates/core-ffi/`, `crates/match-server/`, `crates/archive-client/`, `crates/inference/`, `crates/asset-pipeline/`, `db/game/`, `protocol/`, `data/fixtures/`, `engines/`, `models/releases/`, `assets/`, `docs/`, and `dev/`.
   - Every dependency, build, test, offline local game, locally hosted match, and sample archive study MUST succeed from a fresh public checkout with zero private credentials, zero private Git/registry dependencies, zero upstream source-site fetches, and zero production model downloads.
2. **Private companion (`play-jade/cinnabar`)**:
   - Owns source-specific acquisition (`ingest/`), historical entity resolution and correction queues (`identities/`), full corpus indexing and exports (`corpus/`, `exports/`), production archive API and bounded workers (`api/`, `workers/`), Rust/Burn self-play training (`research/`), private schemas/manifests (`db/`, `manifests/`), and deployment/recovery (`deploy/`, `operations/`).
   - `cinnabar` depends one-way on versioned/pinned public crates from `play-jade/jade`. Never copy private source fixtures, scraper selectors, raw HTML observations, credentials, training checkpoints, or internal handoff documents into `play-jade/jade`.

## Code, Language, and Licensing Rules

- **Languages**: Rust, TypeScript/Vue (Nuxt 4), Swift (SwiftUI + AppKit/UIKit), Kotlin (Jetpack Compose), C# (WinUI 3 / .NET), SQL, CSS, and shell/Taskfile automation. **Never commit Python (`.py`) files to the repository.**
- **Licensing**:
  - `Apache-2.0` applies explicitly to independently written reusable public crates: `crates/xiangqi-core`, `crates/core-wasm`, `crates/core-ffi`, `crates/archive-client`, and `protocol/`.
  - Do not apply `Apache-2.0` by implication to the entire monorepo; application, server, GPLv3 reference engine (`Pikafish` / `Fairy-Stockfish`), font (`SIL OFL 1.1`), and art/model artifacts carry package-specific terms recorded in their manifests.
- **Rules Authority**:
  - `crates/xiangqi-core` is the sole referee and rules authority (`program-2023.11-v1`, Pikafish Chinese Chess Program Competition Rules rev. 2023-11-22). Search engines advise; they never adjudicate games.
  - Placement keys (`PositionKey`) exclude internal piece tokens and history; terminal adjudication and repetition/chase resolution require full move/check/chase history.
- **Privacy and Honest State**:
  - Personal games and studies default to `private`. Sharing (`unlisted` / `public`) and research/training/dataset opt-ins are separate, explicit controls requiring both participants' consent for matches.
  - Never display invented Elo ratings, fake player profiles, synthetic corpus counts disguised as real archive totals, or unbuilt native clients advertised as released.

## Verification Evidence Requirements

- Run shared rules, notation, repetition/chase (`例1`–`例39`), and `BoardView` geometry/hit-testing fixtures across every active binding (`Rust`, `wasm-bindgen`, `UniFFI Swift`, `UniFFI Kotlin`, `.NET C ABI`, and `GTK`).
- Validate `assets/manifest.json` against `assets/manifest.schema.json` and keep `docs/rendering.md` and `docs/visual-reference/` synchronized with renderer changes.
- Record exact OS, SDK, device/simulator, and build mode for every test or screenshot claim.
