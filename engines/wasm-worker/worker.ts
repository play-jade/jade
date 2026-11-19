// SPDX-License-Identifier: Apache-2.0

import type {
  WorkerCommand,
  WorkerEvent,
  WorkerSearchCommand,
  WorkerSearchCompleteEvent,
  WorkerSearchInfoEvent,
} from "./protocol.ts";
import { WasmXiangqiEngine, XiangqiPosition } from "./xiangqi-search-wasm.ts";

export interface WorkerEndpoint {
  postMessage(event: WorkerEvent): void;
  close?(): void;
}

function defaultYieldSlice(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof setTimeout === "function") {
      setTimeout(resolve, 0);
    } else {
      Promise.resolve().then(resolve);
    }
  });
}

/**
 * Stateful Web Worker search host implementing the Section 12 Search Adapter
 * Contract (`init`, `configure`, `search`, `cancel`, `dispose`) with generation
 * tracking so cancelled or superseded searches never emit stale `info` updates.
 */
export class XiangqiWorkerRuntime {
  private readonly endpoint: WorkerEndpoint;
  private readonly yieldSlice: () => Promise<void>;
  private readonly engine = new WasmXiangqiEngine();

  private activeGeneration = 0;
  private activeSearchId: string | null = null;
  private latestQueuedSearch: { generation: number; cmd: WorkerSearchCommand } | null = null;
  private loopRunning = false;
  private disposed = false;

  constructor(endpoint: WorkerEndpoint, yieldSlice: () => Promise<void> = defaultYieldSlice) {
    this.endpoint = endpoint;
    this.yieldSlice = yieldSlice;
  }

  async handleCommand(cmd: WorkerCommand): Promise<void> {
    if (this.disposed && cmd.type !== "dispose") {
      this.endpoint.postMessage({
        type: "error",
        request_id: "request_id" in cmd ? cmd.request_id : undefined,
        search_id: cmd.type === "search" ? cmd.search_id : undefined,
        state_id: cmd.type === "search" ? cmd.state_id : undefined,
        code: "DISPOSED",
        message: "worker engine runtime has already been disposed",
      });
      return;
    }

    try {
      switch (cmd.type) {
        case "init": {
          const ready = await this.engine.init({
            sharedControlBuffer: cmd.shared_control_buffer,
            configure: cmd.options,
            onProgress: (p) => {
              this.endpoint.postMessage({
                ...p,
                request_id: cmd.request_id,
              });
            },
          });
          this.endpoint.postMessage({
            type: "ready",
            request_id: cmd.request_id,
            capabilities: ready.capabilities,
            options: ready.options,
            manifest: ready.manifest,
          });
          return;
        }

        case "configure": {
          if (!this.engine.isInitialized()) {
            await this.engine.init({ configure: cmd.options });
          }
          // Cancel any running search before resizing TT or changing evaluation parameters.
          this.activeGeneration++;
          this.latestQueuedSearch = null;
          this.engine.requestStop();

          const configured = this.engine.applyConfigure(cmd.options ?? {});
          this.endpoint.postMessage({
            type: "configured",
            request_id: cmd.request_id,
            options: configured.options,
            manifest: configured.manifest,
          });
          return;
        }

        case "search": {
          if (!cmd.search_id || !cmd.state_id) {
            this.endpoint.postMessage({
              type: "error",
              search_id: cmd.search_id,
              state_id: cmd.state_id,
              code: "INVALID_REQUEST",
              message: "search command requires non-empty search_id and state_id",
            });
            return;
          }

          if (!this.engine.isInitialized()) {
            await this.engine.init();
          }

          // Validate FEN and continuation moves synchronously before superseding or queuing.
          const pos = new XiangqiPosition();
          const fenRes = pos.setFen(cmd.fen ?? "startpos");
          if (!fenRes.ok) {
            this.endpoint.postMessage({
              type: "error",
              search_id: cmd.search_id,
              state_id: cmd.state_id,
              code: "INVALID_POSITION",
              message: fenRes.error,
            });
            return;
          }
          if (cmd.moves) {
            for (const mv of cmd.moves) {
              const mvRes = pos.applyUciMove(mv);
              if (!mvRes.ok) {
                this.endpoint.postMessage({
                  type: "error",
                  search_id: cmd.search_id,
                  state_id: cmd.state_id,
                  code: "INVALID_POSITION",
                  message: mvRes.error,
                });
                return;
              }
            }
          }

          const nextGen = ++this.activeGeneration;
          this.engine.requestStop();

          // If a previous queued search had not even started yet, emit a tagged superseded frame for it.
          if (this.latestQueuedSearch) {
            const dropped = this.latestQueuedSearch.cmd;
            const manifest = this.engine.getManifest();
            this.endpoint.postMessage({
              type: "complete",
              status: "superseded",
              cancelled: true,
              superseded: true,
              stale: true,
              search_id: dropped.search_id,
              state_id: dropped.state_id,
              depth: 0,
              seldepth: 0,
              nodes: 0,
              nps: 0,
              time_ms: 0,
              hashfull: 0,
              side_to_move: "red",
              score: { kind: "cp", value: 0, red_value: 0, mate: null, perspective: "side_to_move" },
              best_move: null,
              pv: [],
              lines: [],
              ...manifest,
              manifest,
            });
          }

          this.latestQueuedSearch = { generation: nextGen, cmd };
          if (!this.loopRunning) {
            void this.drainSearchQueue();
          }
          return;
        }

        case "cancel": {
          if (!cmd.search_id || cmd.search_id === this.activeSearchId ||
              cmd.search_id === this.latestQueuedSearch?.cmd.search_id) {
            this.activeGeneration++;
            this.latestQueuedSearch = null;
            this.engine.requestStop();
          }
          return;
        }

        case "dispose": {
          this.disposed = true;
          this.activeGeneration++;
          this.latestQueuedSearch = null;
          this.engine.requestStop();
          this.endpoint.postMessage({
            type: "disposed",
            request_id: cmd.request_id,
          });
          this.endpoint.close?.();
          return;
        }
      }
    } catch (err) {
      this.endpoint.postMessage({
        type: "error",
        request_id: "request_id" in cmd ? cmd.request_id : undefined,
        search_id: cmd.type === "search" ? cmd.search_id : undefined,
        state_id: cmd.type === "search" ? cmd.state_id : undefined,
        code: "INTERNAL_ERROR",
        message: err instanceof Error ? err.message : String(err),
      });
    }
  }

  private async drainSearchQueue(): Promise<void> {
    this.loopRunning = true;
    try {
      while (this.latestQueuedSearch && !this.disposed) {
        const job = this.latestQueuedSearch;
        this.latestQueuedSearch = null;
        await this.executeSearchJob(job.generation, job.cmd);
      }
    } finally {
      this.loopRunning = false;
      this.activeSearchId = null;
    }
  }

  private async executeSearchJob(gen: number, cmd: WorkerSearchCommand): Promise<void> {
    const pos = new XiangqiPosition();
    pos.setFen(cmd.fen ?? "startpos");
    if (cmd.moves) {
      for (const mv of cmd.moves) {
        pos.applyUciMove(mv);
      }
    }

    this.activeSearchId = cmd.search_id;
    if (this.activeGeneration === gen && !this.disposed) {
      this.engine.clearStop();
    }

    const isStale = (): boolean => this.disposed || this.activeGeneration !== gen;
    const manifest = this.engine.getManifest();

    const result = await this.engine.searchAsync({
      position: pos,
      limits: cmd.limits,
      shouldAbort: isStale,
      yieldSlice: this.yieldSlice,
      onInfo: (upd) => {
        if (isStale()) return;
        const infoEvent: WorkerSearchInfoEvent = {
          type: "info",
          stale: false,
          superseded: false,
          search_id: cmd.search_id,
          state_id: cmd.state_id,
          depth: upd.depth,
          seldepth: upd.seldepth,
          multipv: upd.line.multipv,
          nodes: upd.nodes,
          nps: upd.nps,
          time_ms: upd.time_ms,
          hashfull: upd.hashfull,
          side_to_move: upd.side_to_move,
          score: upd.line.score,
          best_move: upd.line.best_move,
          pv: upd.line.pv,
          ...manifest,
          manifest,
        };
        this.endpoint.postMessage(infoEvent);
      },
    });

    if (this.disposed) return;

    const superseded = this.latestQueuedSearch !== null;
    const cancelled =
      superseded ||
      this.activeGeneration !== gen ||
      Boolean(cmd.limits?.infinite && result.aborted);

    const completeEvent: WorkerSearchCompleteEvent = {
      type: "complete",
      status: superseded ? "superseded" : cancelled ? "cancelled" : "completed",
      cancelled,
      superseded,
      stale: superseded,
      search_id: cmd.search_id,
      state_id: cmd.state_id,
      depth: result.depth,
      seldepth: result.seldepth,
      nodes: result.nodes,
      nps: result.nps,
      time_ms: result.time_ms,
      hashfull: result.hashfull,
      side_to_move: result.side_to_move,
      score: result.score,
      best_move: result.best_move,
      pv: result.pv,
      lines: result.lines,
      ...manifest,
      manifest,
    };
    this.endpoint.postMessage(completeEvent);
  }
}

export function attachDedicatedWorkerScope(scope?: {
  onmessage: ((ev: { data: WorkerCommand }) => void) | null;
  postMessage: (msg: WorkerEvent) => void;
  close?: () => void;
}): XiangqiWorkerRuntime {
  const target =
    scope ??
    (globalThis as unknown as {
      onmessage: ((ev: { data: WorkerCommand }) => void) | null;
      postMessage: (msg: WorkerEvent) => void;
      close?: () => void;
    });

  const runtime = new XiangqiWorkerRuntime({
    postMessage: (ev) => target.postMessage(ev),
    close: () => target.close?.(),
  });

  target.onmessage = (ev: { data: WorkerCommand }) => {
    void runtime.handleCommand(ev.data);
  };
  return runtime;
}

// Auto-bind when executed inside a browser DedicatedWorkerGlobalScope.
const maybeWorkerGlobal = globalThis as {
  WorkerGlobalScope?: unknown;
  postMessage?: unknown;
};
if (
  typeof maybeWorkerGlobal.WorkerGlobalScope !== "undefined" &&
  typeof maybeWorkerGlobal.postMessage === "function"
) {
  attachDedicatedWorkerScope();
}
