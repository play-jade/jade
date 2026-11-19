# Jade Xiangqi Search Engines & Platform Adapters (`engines/`)

Search engines in `play-jade/jade` **advise** on candidate continuations and positional evaluations; **`crates/xiangqi-core` is the sole referee and rules authority** (`program-2023.11-v1`, Pikafish Chinese Chess Program Competition Rules rev. 2023-11-22). Every search adapter normalizes scores, stamps deterministic evaluation manifests, and validates that emitted principal-variation (`pv`) moves are strictly legal before updating any board or study state.

---

## Directory Layout

| Path | License | Responsibility |
| --- | --- | --- |
| `engines/mobile-bridge/` | `Apache-2.0` | Dependency-free C++26 (`-std=c++26`) Xiangqi search engine, standalone UCI binary (`jade-uci-engine` / `jade-ReferenceUCI`), narrow typed C-ABI linked library (`include/jade_engine.h`, `libjade_engine.dylib`/`.so`, `libjade_engine.a`), Android NDK JNI bridge (`src/jni_bridge.cpp`), and concurrency/cancellation test suite (`tests/engine_bridge_test.cpp`). |
| `engines/wasm-worker/` | `Apache-2.0` | Browser Web Worker search adapter (`protocol.ts`, `xiangqi-search-wasm.ts`, `worker.ts`, `client.ts`, `index.ts`) with an inline WebAssembly search kernel (`JADE_SEARCH_KERNEL_WASM_BYTES`), on-demand loading progress events, `SharedArrayBuffer` / `crossOriginIsolated` detection, and stale-update suppression. |
| `engines/uci-host/` | `Apache-2.0` | Rust supervised UCI subprocess host (`jade-uci-host`) managing desktop and bounded worker UCI engines over `stdin`/`stdout` pipes, canonical continuation verification via `xiangqi-core`, and evaluation manifest generation. |

---

## Search Adapter Contract (Section 12)

Every execution host implements the same logical five-operation lifecycle (`init` / `create`, `configure`, `search`, `cancel`, `dispose`) with correlation identifiers so rapid move scrubbing never paints stale analysis onto a newer board state:

1. **`init` / `create`**:
   - Allocates an isolated engine instance (no shared mutable transposition or killer/history state across instances).
   - Browser workers load on demand, emitting `progress` events (`wasm_fetch` → `wasm_compile` → `ready`) and reporting runtime capabilities (`cross_origin_isolated`, `shared_array_buffer`, `wasm_threads`, `execution_mode`).
2. **`configure`**:
   - Applies bounded engine options (`hash_mb`, `threads`, `multi_pv`, `move_overhead_ms`, `rules_profile`, `engine_id`, `network_hash`) after safely stopping any in-flight search, and recomputes the 64-hex SHA-256 `config_hash`.
3. **`search`**:
   - Accepts `{ search_id, state_id, fen, moves, limits }` where `limits` supports `depth`, `movetime_ms`, `nodes`, `infinite`, and `multi_pv`.
   - Never blocks UI/caller threads: runs on a dedicated Web Worker (`wasm-worker`), an internal per-handle bounded `std::thread` (`mobile-bridge`), or a supervised child process (`uci-host`).
   - Repeats `search_id` and `state_id` on every streaming `info` frame and terminal `complete` frame.
   - Superseding an active search with a new `search_id` atomically aborts the earlier search within 256 nodes (< 5 ms), suppresses further `info` frames for the superseded search, and marks any superseded completion frame with `"superseded": true` and `"stale": true`.
4. **`cancel`**:
   - Signals an atomic stop checked every 256 nodes (< 5 ms) and resolves cleanly with the best verified legal move found so far. Safe to invoke from the UI thread or re-entrantly from inside a streaming callback without deadlock.
5. **`dispose`**:
   - Cancels active work, joins or terminates the underlying worker/process, and frees all memory associated with the handle.

### Normalized Score & Evaluation Manifest Envelope

Every `info` and `complete` payload includes normalized score and provenance metadata:

```json
{
  "type": "complete",
  "status": "completed",
  "cancelled": false,
  "superseded": false,
  "stale": false,
  "search_id": "search-001",
  "state_id": "state-pos-01",
  "depth": 6,
  "seldepth": 11,
  "nodes": 28410,
  "nps": 1894000,
  "time_ms": 15,
  "hashfull": 4,
  "side_to_move": "red",
  "score": {
    "kind": "cp",
    "value": 34,
    "red_value": 34,
    "mate": null,
    "perspective": "side_to_move"
  },
  "best_move": "h2e2",
  "pv": ["h2e2", "h9g7", "b0c2", "i9h9"],
  "engine_id": "jade-reference-cpp-0.1.0",
  "binary_hash": "<64-char-sha256>",
  "network_hash": "<64-char-sha256>",
  "rules_profile": "program-2023.11-v1",
  "config_hash": "<64-char-sha256>",
  "manifest": {
    "engine_id": "jade-reference-cpp-0.1.0",
    "binary_hash": "<64-char-sha256>",
    "network_hash": "<64-char-sha256>",
    "rules_profile": "program-2023.11-v1",
    "config_hash": "<64-char-sha256>"
  }
}
```

---

## Platform Engine Hosts

### 1. Browser Web Worker (`engines/wasm-worker/`)

- Exports `BrowserWorkerSearchAdapter` (`client.ts`) and `XiangqiWorkerRuntime` (`worker.ts`) backed by `WasmXiangqiEngine` (`xiangqi-search-wasm.ts`).
- Compiles and instantiates an inline WebAssembly search kernel (`JADE_SEARCH_KERNEL_WASM_BYTES`).
- Automatically detects `globalThis.crossOriginIsolated` and `SharedArrayBuffer`:
  - **`shared-memory-worker`** (`crossOriginIsolated === true`): allocates shared `WebAssembly.Memory` + `SharedArrayBuffer` control words so `Atomics.store` stops in-flight searches immediately across threads without waiting for the Worker event queue.
  - **`single-thread-worker`** (`crossOriginIsolated === false`): uses unshared `WebAssembly.Memory` with cooperative slice yielding so `cancel` and superseding `search` messages remain responsive (< 5 ms) without COOP/COEP headers.

### 2. iOS & macOS C-ABI Linked Library (`engines/mobile-bridge/`)

- Compiled with `-std=c++26` into `libjade_engine.a` and `libjade_engine.dylib`.
- Swift (`packages/apple/`) imports the pure C11-compatible header [`jade_engine.h`](file:///Users/andrew/workspace/play-jade/jade/.claude/worktrees/slice-a/engines/mobile-bridge/include/jade_engine.h):
  - `jade_engine_create(JadeEngineHandle** out_handle) -> int32_t`
  - `jade_engine_configure(JadeEngineHandle* handle, const char* options_json_utf8, size_t len) -> int32_t`
  - `jade_engine_search(JadeEngineHandle* handle, const char* request_json_utf8, size_t len, JadeEngineCallback on_info, void* user_data) -> int32_t`
  - `jade_engine_cancel(JadeEngineHandle* handle) -> int32_t`
  - `jade_engine_dispose(JadeEngineHandle* handle) -> void`
  - `jade_engine_last_error(const JadeEngineHandle* handle, char* buf, size_t buf_cap) -> size_t`
- Every `extern "C"` entrypoint traps all C++ exceptions (`try { ... } catch (...)`) and records human-readable diagnostics in the per-handle mutex-guarded error buffer.

### 3. Android NDK / JNI Linked Library (`engines/mobile-bridge/src/jni_bridge.cpp`)

- Exposes `Java_play_jade_app_engine_NativeEngineBridge_nativeCreate`, `nativeConfigure`, `nativeSearch`, `nativeCancel`, `nativeDispose`, and `nativeLastError`.
- Routes all operations strictly through the narrow `jade_engine_*` C ABI while managing `JavaVM::AttachCurrentThread`, `GlobalRef` callback lifetimes, `DeleteLocalRef` per streaming event, and `ExceptionClear` guards.

### 4. Desktop Supervised UCI Process (`engines/uci-host/` + `jade-uci-engine`)

- Desktop clients and local integration tests supervise a standalone UCI child process over pipes.
- Out of the box, `dev/build-engines.sh` builds `jade-uci-engine` (`Apache-2.0`), which speaks standard Xiangqi UCI (`uci`, `isready`, `ucinewgame`, `setoption name Hash/Threads/MultiPV/Move Overhead/EvalFile/UCI_Variant`, `position startpos|fen ... [moves ...]`, `go depth|movetime|nodes|infinite|wtime|btime`, `stop`, `quit`).
- Setting `JADE_UCI_ENGINE_PATH` points `jade-uci-host` at either `jade-uci-engine` or an external UCI engine binary such as Official `Pikafish` or `Fairy-Stockfish`.

### 5. Corpus & Analysis Bounded Workers

- Batch analysis workers drive supervised UCI engine pools with explicit `nodes`, `depth`, and `movetime` caps, persisting the full SHA-256 `EvaluationManifest` (`engine_id`, `binary_hash`, `network_hash`, `rules_profile`, `config_hash`) beside every stored evaluation.

---

## GPLv3 Isolation Boundary

- All code committed under `engines/mobile-bridge/`, `engines/wasm-worker/`, and `engines/uci-host/` is independently written **`Apache-2.0`** software containing zero GPLv3 source files, headers, or static/dynamic library links.
- External GPLv3 engines (`Pikafish`, `Fairy-Stockfish`) and third-party `.nnue` weights are **never** linked in-process into `play-jade/jade` crates, web bundles, or iOS/Android linked libraries.
- When a desktop user or private `cinnabar` corpus worker supplies an external `Pikafish` or `Fairy-Stockfish` binary via `JADE_UCI_ENGINE_PATH` (and optional `EvalFile`), interaction occurs strictly across an operating-system process boundary using standard plain-text UCI over `stdin`/`stdout`, and its `binary_hash` and `network_hash` are recorded explicitly in the resulting evaluation manifest.

---

## Build & Verification Commands

### Build & Test Native C++26 Engine, Libraries, and UCI Binary

```bash
# Default build into engines/mobile-bridge/build/ (gitignored) + full test suite
./dev/build-engines.sh

# Build into an explicit scratch directory (e.g., /tmp/jade-engine-build)
JADE_ENGINE_BUILD_DIR=/tmp/jade-engine-build ./dev/build-engines.sh
```

### Interactive UCI Smoke Check

```bash
printf "uci\nisready\nposition startpos moves h2e2 h9g7\ngo depth 5\nquit\n" \
  | ./engines/mobile-bridge/build/jade-uci-engine
```

### CMake / Android NDK Build (`engines/mobile-bridge/CMakeLists.txt`)

```bash
cmake -S engines/mobile-bridge -B engines/mobile-bridge/build -DCMAKE_BUILD_TYPE=Release
cmake --build engines/mobile-bridge/build -j
ctest --test-dir engines/mobile-bridge/build --output-on-failure
```

### Browser Web Worker Contract Suite (`engines/wasm-worker/`)

```bash
# Node 24+ native TypeScript test runner
cd engines/wasm-worker && node --test tests/worker_contract.test.ts
```
