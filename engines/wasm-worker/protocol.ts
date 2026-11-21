// SPDX-License-Identifier: Apache-2.0

export type RulesProfile = "program-2023.11-v1" | (string & {});

export type XiangqiSide = "red" | "black";

export interface NormalizedScore {
  kind: "cp" | "mate";
  /** Score in centipawns or mate-in-N moves from `side_to_move` perspective. */
  value: number;
  /** Score normalized to Red's perspective (`+` favors Red, `-` favors Black). */
  red_value: number;
  /** Non-null only when `kind === "mate"`. */
  mate: number | null;
  perspective: "side_to_move";
}

export interface EvaluationManifest {
  engine_id: string;
  binary_hash: string;
  network_hash: string;
  rules_profile: RulesProfile;
  config_hash: string;
}

export interface WorkerRuntimeCapabilities {
  cross_origin_isolated: boolean;
  shared_array_buffer: boolean;
  wasm_threads: boolean;
  execution_mode: "shared-memory-worker" | "single-thread-worker";
  max_threads: number;
}

export interface EngineConfigureOptions {
  hash_mb?: number;
  threads?: number;
  multi_pv?: number;
  move_overhead_ms?: number;
  rules_profile?: RulesProfile;
  engine_id?: string;
  network_hash?: string;
}

export interface ResolvedEngineOptions {
  hash_mb: number;
  threads: number;
  multi_pv: number;
  move_overhead_ms: number;
  rules_profile: RulesProfile;
  engine_id: string;
  network_hash: string;
}

export interface SearchLimits {
  depth?: number;
  movetime_ms?: number;
  nodes?: number;
  infinite?: boolean;
  multi_pv?: number;
}

export interface SearchPvLine {
  multipv: number;
  depth: number;
  seldepth: number;
  score: NormalizedScore;
  best_move: string | null;
  pv: string[];
}

// ---------------------------------------------------------------------------
// Main Thread -> Worker Commands (Section 12 Search Adapter Contract)
// ---------------------------------------------------------------------------

export interface WorkerInitCommand {
  type: "init";
  request_id?: string;
  wasm_url?: string;
  network_url?: string;
  shared_control_buffer?: SharedArrayBuffer;
  options?: EngineConfigureOptions;
}

export interface WorkerConfigureCommand {
  type: "configure";
  request_id?: string;
  options: EngineConfigureOptions;
}

export interface WorkerSearchCommand {
  type: "search";
  search_id: string;
  state_id: string;
  fen?: string;
  moves?: string[];
  limits?: SearchLimits;
}

export interface WorkerCancelCommand {
  type: "cancel";
  request_id?: string;
  search_id?: string;
}

export interface WorkerDisposeCommand {
  type: "dispose";
  request_id?: string;
}

export type WorkerCommand =
  | WorkerInitCommand
  | WorkerConfigureCommand
  | WorkerSearchCommand
  | WorkerCancelCommand
  | WorkerDisposeCommand;

// ---------------------------------------------------------------------------
// Worker -> Main Thread Events
// ---------------------------------------------------------------------------

export type LoadPhase = "wasm_fetch" | "wasm_compile" | "network_load" | "ready";

export interface WorkerProgressEvent {
  type: "progress";
  request_id?: string;
  phase: LoadPhase;
  loaded_bytes: number;
  total_bytes: number;
  percent: number;
}

export interface WorkerReadyEvent {
  type: "ready";
  request_id?: string;
  capabilities: WorkerRuntimeCapabilities;
  options: ResolvedEngineOptions;
  manifest: EvaluationManifest;
}

export interface WorkerConfiguredEvent {
  type: "configured";
  request_id?: string;
  options: ResolvedEngineOptions;
  manifest: EvaluationManifest;
}

export interface WorkerSearchInfoEvent extends EvaluationManifest {
  type: "info";
  stale: boolean;
  superseded: boolean;
  search_id: string;
  state_id: string;
  depth: number;
  seldepth: number;
  multipv: number;
  nodes: number;
  nps: number;
  time_ms: number;
  hashfull: number;
  side_to_move: XiangqiSide;
  score: NormalizedScore;
  best_move: string | null;
  pv: string[];
  manifest: EvaluationManifest;
}

export interface WorkerSearchCompleteEvent extends EvaluationManifest {
  type: "complete";
  status: "completed" | "cancelled" | "superseded";
  cancelled: boolean;
  superseded: boolean;
  stale: boolean;
  search_id: string;
  state_id: string;
  depth: number;
  seldepth: number;
  nodes: number;
  nps: number;
  time_ms: number;
  hashfull: number;
  side_to_move: XiangqiSide;
  score: NormalizedScore;
  best_move: string | null;
  pv: string[];
  lines: SearchPvLine[];
  manifest: EvaluationManifest;
}

export interface WorkerDisposedEvent {
  type: "disposed";
  request_id?: string;
}

export interface WorkerErrorEvent {
  type: "error";
  request_id?: string;
  search_id?: string;
  state_id?: string;
  code:
    | "NOT_INITIALIZED"
    | "INVALID_OPTIONS"
    | "INVALID_POSITION"
    | "INVALID_REQUEST"
    | "DISPOSED"
    | "INTERNAL_ERROR";
  message: string;
}

export type WorkerEvent =
  | WorkerProgressEvent
  | WorkerReadyEvent
  | WorkerConfiguredEvent
  | WorkerSearchInfoEvent
  | WorkerSearchCompleteEvent
  | WorkerDisposedEvent
  | WorkerErrorEvent;

export interface SearchAdapterRequest {
  search_id: string;
  state_id: string;
  fen?: string;
  moves?: string[];
  limits?: SearchLimits;
  onInfo?: (event: WorkerSearchInfoEvent) => void;
}

export interface SearchAdapter {
  init(options?: {
    wasmUrl?: string;
    networkUrl?: string;
    configure?: EngineConfigureOptions;
    onProgress?: (progress: WorkerProgressEvent) => void;
  }): Promise<WorkerReadyEvent>;
  configure(options: EngineConfigureOptions): Promise<WorkerConfiguredEvent>;
  search(request: SearchAdapterRequest): Promise<WorkerSearchCompleteEvent>;
  cancel(searchId?: string): Promise<void>;
  dispose(): Promise<void>;
}
