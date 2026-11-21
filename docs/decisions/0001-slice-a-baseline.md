# ADR 0001: Slice A Platform Baseline, Rules Profile, Renderer, and Repository Boundary

- **Status**: Accepted
- **Date**: 2026-11-18
- **Repositories**: `play-jade/jade` (public) and `play-jade/cinnabar` (private)

## Context

Jade combines a public Xiangqi platform (friendly invitation play, offline native play, studies, archive exploration, and local engine analysis across web, iOS/iPadOS, Android, macOS, Windows, and Linux) with a private companion system (`play-jade/cinnabar`) for fresh corpus acquisition, reversible player-identity resolution, position-graph indexing, and `Rust/Burn` self-play research.

## Decisions

### 1. Identity and Repository Ownership

- **English identity**: `Jade` (titlecase serif wordmark from Noto Serif Display Regular 2.003; `象` signet prototype from Noto Serif CJK SC Bold 2.003).
- **Current Chinese display**: `玉枰象棋` (`Jade — Xiangqi / 玉枰象棋`), pending regional CN/TW/HK wording and native-calligraphy review.
- **Canonical GitHub organization**: `play-jade`, split into:
  - `play-jade/jade` (public monorepo): complete game core, bindings, match server, sample archive provider, web app, native clients, engine host adapters, and approved public art/brand pipeline.
  - `play-jade/cinnabar` (private companion): source-specific adapters, raw observations, entity resolution, full corpus graph/search API, bounded `FOR UPDATE SKIP LOCKED` workers, `Rust/Burn` training, and production deployment/recovery.
- **One-way dependency rule**: `play-jade/jade` builds, tests, plays offline, and runs local hosted matches with zero private dependencies, zero private credentials, and zero upstream source-site traffic. `play-jade/cinnabar` pins versioned crates and contracts from `play-jade/jade`.

### 2. Adopted Online Rules Profile and Clock Defaults

- **Rules profile**: `program-2023.11-v1`, implementing the Pikafish community *Chinese Chess Program Competition Rules* (revision 22 November 2023), verified against all 39 published repetition/chase examples (`例1`–`例39`), checkmate (`将死`), no-legal-move loss (`困毙`), insufficient attacking material, and the 120-halfmove natural limit with per-side 10-check caps.
- **Clock default**: `10 minutes + 5 seconds increment per move` for friendly human invitations, alongside untimed play. Hosted online matches remain server-authoritative; native clients also support independent offline play and save/resume without an account or server connection.

### 3. Shared Rust Core and Platform-Native Client Baseline

- **Core**: Pure deterministic Rust (`crates/xiangqi-core`, `rustc 1.98.1`, Rust 2024 edition) with zero UI, database, network, or tensor dependencies.
- **Bindings**:
  - Web: `wasm-bindgen` (`crates/core-wasm`) consumed by `Nuxt 4.6.4 / Vue 3 / TypeScript`.
  - Apple (`iOS`, `iPadOS`, `macOS`): `UniFFI` Swift bindings (`crates/core-ffi` -> `packages/apple`), paired with `SwiftUI + UIKit` on iOS/iPadOS and `SwiftUI + AppKit` on macOS.
  - Android: `UniFFI` Kotlin bindings (`crates/core-ffi` -> `apps/android`), paired with `Kotlin + Jetpack Compose`.
  - Windows: Narrow panic-safe C ABI (`crates/core-ffi`) consumed via `.NET LibraryImport` and `SafeHandle` in `C# + WinUI 3 / XAML + Win2D`.
  - Linux: Direct Rust crate linkage in `GTK 4 + libadwaita + Cairo` (`apps/linux`).

### 4. Rendering and Accessible Interaction

- **Treatment**: Orthographic 2.5D baked material pipeline (`pale-jade` flagship, `boxwood`, `ivory-stone`) with exact platform-rendered board markings, upright glyphs, fixed top-left studio key light preserved across board orientation flips, and exact Section 4 logical viewport/half-open cell hit-testing coordinates.
- **Accessibility**: Semantic DOM on web and host accessibility trees on native platforms (VoiceOver, TalkBack, Narrator, GTK Accessible), supporting keyboard focus/movement, tap-to-select + tap-to-move alternatives to dragging, 200% text scaling, muted audio, and reduced motion.

### 5. Engine Analysis, Research Track, Privacy, and Code Terms

- **Engine hosts**: Browser Web Worker WASM, mobile linked C/C++ library on bounded worker threads, and desktop/worker supervised UCI child processes. Engines advise; `xiangqi-core` adjudicates.
- **Privacy defaults**: Personal games and studies default to `private`. Explicit sharing (`unlisted` / `public`) and separate opt-ins for internal model training, published research, and downloadable datasets require both participants' consent for matches.
- **First research track**: Private `Rust/Burn` rules-only self-play (`from * 90 + to` = `8,100` action labels) evaluated against a frozen opponent pool under fixed budgets.
- **Reusable crate licensing**: `Apache-2.0` explicitly scoped to independently written public core, binding, interface, and contract crates (`xiangqi-core`, `archive-client`, `jade-inference`, `core-wasm`, `core-ffi`, `jade-uci-host`, `protocol`).
