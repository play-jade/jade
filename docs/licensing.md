# Licensing and Artifact Terms

`play-jade/jade` separates code, font, art, audio, engine, and data licenses per package rather than marking the entire monorepo `Apache-2.0` by implication.

## 1. Reusable Public Core, Binding, and Contract Packages (`Apache-2.0`)

The following independently written packages are licensed under the **Apache License, Version 2.0** (each carrying its own `LICENSE` file):

- [`crates/xiangqi-core/`](../crates/xiangqi-core/LICENSE) — Deterministic Xiangqi coordinates, rules, notation, `PositionKey` v1, `BoardView` geometry, and `program-2023.11-v1` adjudication.
- [`crates/archive-client/`](../crates/archive-client/LICENSE) — Versioned public archive, game, study, and privacy contracts plus synthetic/sample fixture provider.
- [`crates/inference/`](../crates/inference/LICENSE) — Observation/action encoding (`8,100` labels) and released-model inference contracts.
- [`crates/core-wasm/`](../crates/core-wasm/LICENSE) — `wasm-bindgen` browser bindings.
- [`crates/core-ffi/`](../crates/core-ffi/LICENSE) — `UniFFI` Swift/Kotlin bindings and narrow C ABI for `.NET LibraryImport`.
- [`engines/uci-host/`](../engines/uci-host/Cargo.toml) & `engines/mobile-bridge/` — Original UCI process supervisor and linked C/C++ search adapter.
- [`protocol/`](../protocol/LICENSE) — Shared JSON schemas and TypeScript protocol definitions.

## 2. Application and Server Packages

- `apps/web/`, `apps/macos/`, `apps/ios/`, `packages/apple/`, `apps/android/`, `apps/windows/`, `apps/linux/`, `crates/match-server/`, and `crates/asset-pipeline/` remain first-party Jade application/service packages owned by `play-jade`. Final distribution license terms and compatible third-party engine bundling decisions are recorded per release target prior to binary store upload or redistribution.

## 3. External Reference Engines (`GPL-3.0`)

- External reference engines such as **Official Pikafish** and **Fairy-Stockfish WASM** are licensed under **GNU GPLv3**, and linked NNUE network files carry their own upstream distribution terms.
- No GPLv3 code is linked into `xiangqi-core`, `core-wasm`, `core-ffi`, or `match-server`. External UCI binaries run out-of-process or as separately packaged optional downloads with full source, hash, and license notices recorded in their evaluation/release manifests.

## 4. Fonts, Brand Kit, Baked Materials, and Audio

- **Fonts (`SIL Open Font License 1.1`)**:
  - `Noto Serif Display Regular 2.003` (`© 2016 Google Inc.`) — used to outline the titlecase `Jade` wordmark.
  - `Noto Serif CJK SC Bold 2.003` (`© 2017–2024 Adobe`) — used for the independent `象` flat signet prototype and prototype gameplay glyph outlines pending native-calligraphy review.
  - `Noto Sans Regular 2.004` (`© 2015 Google LLC`) — used for proof-sheet labels.
  - Full copyright and license notices live in [`assets/brand/source/licenses/`](../assets/brand/source/licenses/) and [`assets/brand/source/font_sources.json`](../assets/brand/source/font_sources.json).
- **Brand, Material, and Audio Assets**:
  - Provenance, SHA-256 checksums, generation records, and review status are recorded in [`assets/brand/README.md`](../assets/brand/README.md) and `assets/manifest.json`.

## 5. Sample Fixtures vs. Historical Collections

- Synthetic and normalized test fixtures under `data/fixtures/` are public test artifacts for `play-jade/jade`.
- Historical corpus observations, source-specific adapters, and dataset releases remain governed by `play-jade/cinnabar` source manifests and explicit per-collection release policies.
