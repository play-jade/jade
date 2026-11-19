// SPDX-License-Identifier: Apache-2.0

import {
  BrowserWorkerSearchAdapter,
  JADE_SEARCH_KERNEL_BINARY_HASH,
  JADE_SEARCH_KERNEL_WASM_BYTES,
  XiangqiPosition,
  type WorkerProgressEvent,
  type WorkerSearchInfoEvent,
} from "../index.ts";

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) {
    throw new Error(`Assertion failed: ${msg}`);
  }
}

export async function runWasmWorkerContractSuite(): Promise<void> {
  // 1. Validate inline WebAssembly binary magic & WebAssembly.validate().
  assert(
    JADE_SEARCH_KERNEL_WASM_BYTES.byteLength > 64,
    "inline Wasm kernel bytes must be non-empty",
  );
  assert(
    WebAssembly.validate(JADE_SEARCH_KERNEL_WASM_BYTES as BufferSource),
    "JADE_SEARCH_KERNEL_WASM_BYTES must pass WebAssembly.validate()",
  );
  assert(
    JADE_SEARCH_KERNEL_BINARY_HASH.length === 64,
    "binary_hash must be a 64-character SHA-256 hex digest",
  );

  // 2. Verify Xiangqi startpos legal move count == 44 and flying general pin.
  const startPos = new XiangqiPosition();
  assert(
    startPos.generateLegal(false).length === 44,
    "Xiangqi startpos must have 44 legal moves",
  );
  const pinnedPos = new XiangqiPosition();
  const fgRes = pinnedPos.setFen("4k4/9/9/9/9/4N4/9/9/9/4K4 w - - 0 1");
  assert(fgRes.ok, "flying general pin FEN must be valid");
  assert(
    pinnedPos.generateLegal(false).length === 3,
    "pinned horse on e4 between generals must have 0 legal moves (only 3 general moves)",
  );

  // 3. Test SearchAdapter.init() progress events, capabilities report, and manifest.
  const adapter = new BrowserWorkerSearchAdapter();
  const progressPhases: string[] = [];
  const ready = await adapter.init({
    configure: { hash_mb: 4, multi_pv: 2 },
    onProgress: (ev: WorkerProgressEvent) => {
      progressPhases.push(ev.phase);
    },
  });

  assert(
    progressPhases.includes("wasm_fetch") &&
      progressPhases.includes("wasm_compile") &&
      progressPhases.includes("ready"),
    `expected progress phases wasm_fetch/wasm_compile/ready, got ${progressPhases.join(",")}`,
  );
  assert(
    typeof ready.capabilities.cross_origin_isolated === "boolean",
    "capabilities must report cross_origin_isolated",
  );
  assert(
    ready.capabilities.execution_mode === "shared-memory-worker" ||
      ready.capabilities.execution_mode === "single-thread-worker",
    "capabilities must report execution_mode",
  );
  assert(ready.manifest.rules_profile === "program-2023.11-v1", "rules_profile mismatch");
  assert(ready.manifest.binary_hash.length === 64, "manifest.binary_hash must be 64 hex chars");
  assert(ready.manifest.config_hash.length === 64, "manifest.config_hash must be 64 hex chars");
  assert(ready.manifest.network_hash.length === 64, "manifest.network_hash must be 64 hex chars");

  // 4. Mate-in-1 tactical search & PV legality verification.
  const infoFrames: WorkerSearchInfoEvent[] = [];
  const mateComplete = await adapter.search({
    search_id: "wasm-mate-1",
    state_id: "board-mate-1",
    fen: "3ka4/R1P1a4/9/9/9/9/9/9/9/4K4 w - - 0 1",
    limits: { depth: 3 },
    onInfo: (ev) => infoFrames.push(ev),
  });

  assert(mateComplete.status === "completed", "mate search must complete normally");
  assert(mateComplete.search_id === "wasm-mate-1", "search_id must match");
  assert(mateComplete.state_id === "board-mate-1", "state_id must match");
  assert(mateComplete.best_move === "a8a9", `expected best_move a8a9, got ${mateComplete.best_move}`);
  assert(mateComplete.score.kind === "mate" && mateComplete.score.value === 1, "expected mate in 1");
  assert(infoFrames.length >= 2, "expected streaming info events");
  for (const inf of infoFrames) {
    assert(inf.search_id === "wasm-mate-1" && inf.state_id === "board-mate-1", "info ids mismatch");
    assert(inf.binary_hash === ready.manifest.binary_hash, "top-level manifest hash mismatch");
  }

  // 5. Superseding an active infinite search without stale board updates.
  const staleInfosAfterSupersede: WorkerSearchInfoEvent[] = [];
  let supersededTriggered = false;
  const oldSearchPromise = adapter.search({
    search_id: "wasm-infinite-old",
    state_id: "board-old",
    fen: "startpos",
    limits: { infinite: true },
    onInfo: (ev) => {
      if (supersededTriggered) {
        staleInfosAfterSupersede.push(ev);
      }
    },
  });

  // Allow at least one slice of the old search to run, then supersede with a fresh search_id.
  await new Promise((r) => setTimeout(r, 15));
  supersededTriggered = true;

  const newSearchPromise = adapter.search({
    search_id: "wasm-bounded-new",
    state_id: "board-new",
    fen: "startpos",
    moves: ["h2e2", "h9g7"],
    limits: { depth: 3 },
  });

  const [oldResult, newResult] = await Promise.all([oldSearchPromise, newSearchPromise]);
  assert(
    staleInfosAfterSupersede.length === 0,
    "adapter must suppress stale info updates after superseding search starts",
  );
  assert(oldResult.cancelled === true, "superseded search must resolve as cancelled");
  assert(newResult.status === "completed", "superseding search must complete cleanly");
  assert(newResult.search_id === "wasm-bounded-new", "new search_id mismatch");
  assert(newResult.state_id === "board-new", "new state_id mismatch");
  assert(newResult.best_move !== null && newResult.pv.length > 0, "new search must yield legal PV");

  // Verify every move in newResult.pv is strictly legal on the board.
  const verifyPos = new XiangqiPosition();
  verifyPos.setFen("startpos");
  verifyPos.applyUciMove("h2e2");
  verifyPos.applyUciMove("h9g7");
  for (const mv of newResult.pv) {
    const step = verifyPos.applyUciMove(mv);
    assert(step.ok, `PV move ${mv} must be legal`);
  }

  // 6. Explicit live cancellation (`infinite` -> `cancel()`).
  const cancelPromise = adapter.search({
    search_id: "wasm-cancel-1",
    state_id: "board-cancel-1",
    fen: "startpos",
    limits: { infinite: true },
  });
  await new Promise((r) => setTimeout(r, 15));
  await adapter.cancel("wasm-cancel-1");
  const cancelledRes = await cancelPromise;
  assert(cancelledRes.cancelled === true, "explicitly cancelled search must mark cancelled=true");
  assert(cancelledRes.best_move !== null, "cancelled search must still return legal fallback/best move");

  await adapter.dispose();
}

// Auto-run when invoked directly in Node 24+ or Bun.
const proc = (globalThis as { process?: { argv?: string[] } }).process;
if (proc?.argv?.[1]?.endsWith("worker_contract.test.ts")) {
  runWasmWorkerContractSuite().then(
    () => {
      console.log("[wasm-worker] ALL CONTRACT TESTS PASSED.");
    },
    (err) => {
      console.error("[wasm-worker] TEST FAILURE:", err);
      throw err;
    },
  );
}
