// SPDX-License-Identifier: Apache-2.0

export * from "./protocol.ts";
export {
  BUILTIN_NETWORK_HASH,
  JADE_SEARCH_KERNEL_BINARY_HASH,
  JADE_SEARCH_KERNEL_WASM_BYTES,
  STARTPOS_FEN,
  WasmXiangqiEngine,
  XiangqiPosition,
  buildSearchKernelWasmBytes,
  detectRuntimeCapabilities,
  moveToUci,
  normalizeScore,
  parseUciMove,
  sha256Hex,
} from "./xiangqi-search-wasm.ts";
export {
  XiangqiWorkerRuntime,
  attachDedicatedWorkerScope,
  type WorkerEndpoint,
} from "./worker.ts";
export {
  BrowserWorkerSearchAdapter,
  type WorkerFactory,
  type WorkerLike,
} from "./client.ts";
