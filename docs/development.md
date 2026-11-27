# Developing Jade

Everything in `play-jade/jade` builds, tests, plays offline, hosts a local match and explores the sample archive from a fresh public checkout, with no private credentials, private repositories or upstream data sources.

## Tools

The repository pins its tools in [`mise.toml`](../mise.toml) (Rust, Node, pnpm, Task and Bosun). With [mise](https://mise.jdx.dev) installed:

```sh
mise install
pnpm install
```

The macOS and iOS clients also need the Xcode command-line tools.

## Run Jade locally

```sh
./dev/build-wasm.sh              # the rules core for the browser
cargo run -p jade-match-server   # match server on http://127.0.0.1:4180
pnpm --dir apps/web dev          # web app on http://localhost:3000
./dev/run-macos.sh               # the native macOS app
```

The web app plays offline without the match server; invitations to a friend need it.

## Test

```sh
cargo test --workspace                                 # rules, notation, geometry and server
cargo run --release -p jade-asset-pipeline -- --verify # board art, glyphs, audio and manifest
./dev/build-engines.sh                                 # native UCI engine and mobile bridge
pnpm --dir apps/web build && pnpm --dir apps/web exec playwright test
```

Changes go through [Bosun](../bosun.yaml); see [`AGENTS.md`](../AGENTS.md) for the evidence each change carries.

## Repository layout

| Path | Responsibility |
| :--- | :--- |
| `crates/xiangqi-core/` | Deterministic coordinates (`0..8 × 0..9`), legal moves, Chinese/WXF/ICCS/UCI notation, `PositionKey` v1, Section 4 `BoardView` geometry, and `program-2023.11-v1` terminal adjudication (`Apache-2.0`). |
| `crates/archive-client/`, `protocol/`, `data/fixtures/` | Versioned public archive/game/study/privacy contracts, standalone sample fixture provider, and shared rules/geometry fixtures (`Apache-2.0`). |
| `crates/core-wasm/`, `crates/core-ffi/` | `wasm-bindgen` browser module, `UniFFI` Swift/Kotlin bindings, and panic-safe C ABI for Windows `.NET LibraryImport` (`Apache-2.0`). |
| `crates/inference/`, `models/releases/` | Observation/action encoding (`from * 90 + to` = `8,100` labels) and released-model inference contracts (`Apache-2.0`). |
| `crates/match-server/`, `db/game/` | Authoritative `Axum`/`Tokio` match, invitation, `10+5`/untimed clock, reconnection, and private-by-default study service. Its schema (`db/game/`) is applied at startup on SQLite or PostgreSQL, and counters added since are added to existing databases. |
| `engines/` | Search adapter contracts, supervised desktop UCI host (`engines/uci-host`), linked C/C++ mobile bridge (`engines/mobile-bridge`), and browser Web Worker (`engines/wasm-worker`). |
| `crates/asset-pipeline/`, `assets/` | Reproducible Rust vector glyph extraction, orthographic 2.5D baked material pipeline, synthesized piece acoustics, brand kit (`assets/brand/`), and `assets/manifest.json`. |
| `apps/web/` | Server-rendered Nuxt 4 / Vue 3 / TypeScript web interface with Canvas 2D materials, SVG/DOM WCAG 2.2 semantics, IndexedDB offline play, and Web Worker analysis. |
| `packages/apple/`, `apps/macos/`, `apps/ios/` | Shared Swift/UniFFI package, native macOS (`SwiftUI + AppKit`), and native iPhone/iPad (`SwiftUI + UIKit`) applications with Core Graphics rendering. |
| `apps/android/` | Native `Kotlin + Jetpack Compose` application with `UniFFI` Kotlin bindings, explicit Compose accessibility semantics, and `SoundPool` audio. |
| `apps/windows/` | Native `C# + WinUI 3 + Win2D` application using `.NET LibraryImport` + `SafeHandle` over `core-ffi`. |
| `apps/linux/` | Native `Rust + GTK 4 + libadwaita + Cairo` application linking `xiangqi-core` directly. |
| `docs/`, `dev/` | Architecture, rules (`program-2023.11-v1`), rendering contract (`docs/rendering.md`), visual reference gallery, licensing, and local build tasks. |

Data acquisition, historical entity resolution, corpus indexing, the hosted archive API and Rust/Burn self-play training live in the private companion repository, `play-jade/cinnabar`, which consumes versioned crates from this repository across a one-way dependency boundary. [`architecture.md`](architecture.md) describes that boundary and how each client integrates the core.
