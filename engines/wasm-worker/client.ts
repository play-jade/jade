// SPDX-License-Identifier: Apache-2.0

import type {
  EngineConfigureOptions,
  SearchAdapter,
  SearchAdapterRequest,
  WorkerCommand,
  WorkerConfiguredEvent,
  WorkerEvent,
  WorkerProgressEvent,
  WorkerReadyEvent,
  WorkerSearchCompleteEvent,
  WorkerSearchInfoEvent,
} from "./protocol.ts";
import { XiangqiWorkerRuntime } from "./worker.ts";

export interface WorkerLike {
  postMessage(message: WorkerCommand): void;
  terminate(): void;
  onmessage: ((ev: { data: WorkerEvent }) => void) | null;
  onerror?: ((err: unknown) => void) | null;
}

export type WorkerFactory = () => WorkerLike;

function createInProcessWorkerPair(): WorkerLike {
  let listener: ((ev: { data: WorkerEvent }) => void) | null = null;
  const runtime = new XiangqiWorkerRuntime({
    postMessage: (ev: WorkerEvent) => {
      listener?.({ data: ev });
    },
  });
  return {
    get onmessage() {
      return listener;
    },
    set onmessage(fn) {
      listener = fn;
    },
    postMessage(cmd: WorkerCommand) {
      void runtime.handleCommand(cmd);
    },
    terminate() {
      listener = null;
    },
  };
}

/**
 * Main-thread Browser Search Adapter (`init`, `configure`, `search`, `cancel`, `dispose`).
 *
 * Instantiates the Web Worker on demand, uses a `SharedArrayBuffer` control word
 * when `crossOriginIsolated` is true for instant atomic stop without waiting on
 * the Worker message queue, and filters stale updates by `(search_id, state_id)`.
 */
export class BrowserWorkerSearchAdapter implements SearchAdapter {
  private readonly createWorker: WorkerFactory;
  private worker: WorkerLike | null = null;
  private sharedControl: Int32Array | null = null;
  private sharedBuffer: SharedArrayBuffer | undefined = undefined;
  private nextReqSeq = 1;
  private currentSearchId: string | null = null;
  private disposed = false;

  private pendingRequests = new Map<
    string,
    {
      resolve: (ev: WorkerEvent) => void;
      reject: (err: Error) => void;
      onProgress?: (ev: WorkerProgressEvent) => void;
    }
  >();

  private activeSearches = new Map<
    string,
    {
      state_id: string;
      onInfo?: (ev: WorkerSearchInfoEvent) => void;
      resolve: (ev: WorkerSearchCompleteEvent) => void;
      reject: (err: Error) => void;
    }
  >();

  constructor(workerFactory?: WorkerFactory) {
    this.createWorker =
      workerFactory ??
      (() => {
        if (typeof window !== "undefined" && typeof Worker !== "undefined") {
          return new Worker(new URL("./worker.ts", import.meta.url), { type: "module" }) as unknown as WorkerLike;
        }
        return createInProcessWorkerPair();
      });
  }

  async init(options?: {
    wasmUrl?: string;
    networkUrl?: string;
    configure?: EngineConfigureOptions;
    onProgress?: (progress: WorkerProgressEvent) => void;
  }): Promise<WorkerReadyEvent> {
    if (this.disposed) {
      throw new Error("BrowserWorkerSearchAdapter has been disposed");
    }
    this.ensureWorker();

    if (
      !this.sharedBuffer &&
      typeof SharedArrayBuffer !== "undefined" &&
      Boolean((globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated)
    ) {
      try {
        this.sharedBuffer = new SharedArrayBuffer(16);
        this.sharedControl = new Int32Array(this.sharedBuffer);
      } catch {
        this.sharedBuffer = undefined;
        this.sharedControl = null;
      }
    }

    const request_id = `req-${this.nextReqSeq++}`;
    const promise = new Promise<WorkerReadyEvent>((resolve, reject) => {
      this.pendingRequests.set(request_id, {
        resolve: (ev) => resolve(ev as WorkerReadyEvent),
        reject,
        onProgress: options?.onProgress,
      });
    });

    this.worker!.postMessage({
      type: "init",
      request_id,
      wasm_url: options?.wasmUrl,
      network_url: options?.networkUrl,
      shared_control_buffer: this.sharedBuffer,
      options: options?.configure,
    });
    return promise;
  }

  async configure(options: EngineConfigureOptions): Promise<WorkerConfiguredEvent> {
    if (this.disposed) {
      throw new Error("BrowserWorkerSearchAdapter has been disposed");
    }
    this.ensureWorker();
    this.signalSharedStop();

    const request_id = `req-${this.nextReqSeq++}`;
    const promise = new Promise<WorkerConfiguredEvent>((resolve, reject) => {
      this.pendingRequests.set(request_id, {
        resolve: (ev) => resolve(ev as WorkerConfiguredEvent),
        reject,
      });
    });
    this.worker!.postMessage({
      type: "configure",
      request_id,
      options,
    });
    return promise;
  }

  async search(request: SearchAdapterRequest): Promise<WorkerSearchCompleteEvent> {
    if (this.disposed) {
      throw new Error("BrowserWorkerSearchAdapter has been disposed");
    }
    if (!this.worker) {
      await this.init();
    }

    // Signal atomic stop on any superseded in-flight search before posting the new search.
    if (this.currentSearchId !== null && this.currentSearchId !== request.search_id) {
      this.signalSharedStop();
    }
    this.currentSearchId = request.search_id;

    return new Promise<WorkerSearchCompleteEvent>((resolve, reject) => {
      this.activeSearches.set(request.search_id, {
        state_id: request.state_id,
        onInfo: request.onInfo,
        resolve,
        reject,
      });

      this.worker!.postMessage({
        type: "search",
        search_id: request.search_id,
        state_id: request.state_id,
        fen: request.fen ?? "startpos",
        moves: request.moves,
        limits: request.limits,
      });
    });
  }

  async cancel(searchId?: string): Promise<void> {
    if (this.disposed || !this.worker) return;
    if (!searchId || searchId === this.currentSearchId) {
      this.signalSharedStop();
      this.currentSearchId = null;
    }
    this.worker.postMessage({
      type: "cancel",
      search_id: searchId,
    });
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.signalSharedStop();
    this.currentSearchId = null;

    if (this.worker) {
      const w = this.worker;
      this.worker = null;
      const request_id = `req-${this.nextReqSeq++}`;
      await new Promise<void>((resolve) => {
        const timer = typeof setTimeout === "function" ? setTimeout(resolve, 50) : null;
        this.pendingRequests.set(request_id, {
          resolve: () => {
            if (timer !== null && typeof clearTimeout === "function") clearTimeout(timer);
            resolve();
          },
          reject: () => resolve(),
        });
        w.postMessage({ type: "dispose", request_id });
      });
      w.terminate();
    }

    for (const [, pending] of this.pendingRequests) {
      pending.reject(new Error("adapter disposed"));
    }
    this.pendingRequests.clear();
    for (const [, s] of this.activeSearches) {
      s.reject(new Error("adapter disposed"));
    }
    this.activeSearches.clear();
  }

  private signalSharedStop(): void {
    if (!this.sharedControl) return;
    if (typeof Atomics !== "undefined" && this.sharedControl.buffer instanceof SharedArrayBuffer) {
      Atomics.store(this.sharedControl, 0, 1);
    } else {
      this.sharedControl[0] = 1;
    }
  }

  private ensureWorker(): void {
    if (this.worker) return;
    const w = this.createWorker();
    w.onmessage = (ev: { data: WorkerEvent }) => this.handleWorkerEvent(ev.data);
    this.worker = w;
  }

  private handleWorkerEvent(ev: WorkerEvent): void {
    if (ev.type === "progress") {
      if (ev.request_id) {
        this.pendingRequests.get(ev.request_id)?.onProgress?.(ev);
      }
      return;
    }

    if (ev.type === "ready" || ev.type === "configured" || ev.type === "disposed") {
      if (ev.request_id) {
        const entry = this.pendingRequests.get(ev.request_id);
        if (entry) {
          this.pendingRequests.delete(ev.request_id);
          entry.resolve(ev);
        }
      }
      return;
    }

    if (ev.type === "error") {
      if (ev.request_id) {
        const entry = this.pendingRequests.get(ev.request_id);
        if (entry) {
          this.pendingRequests.delete(ev.request_id);
          entry.reject(new Error(`${ev.code}: ${ev.message}`));
          return;
        }
      }
      if (ev.search_id) {
        const searchEntry = this.activeSearches.get(ev.search_id);
        if (searchEntry) {
          this.activeSearches.delete(ev.search_id);
          searchEntry.reject(new Error(`${ev.code}: ${ev.message}`));
        }
      }
      return;
    }

    if (ev.type === "info") {
      // Suppress stale info updates if this search is no longer the active board search.
      if (ev.stale || ev.superseded || ev.search_id !== this.currentSearchId) {
        return;
      }
      const searchEntry = this.activeSearches.get(ev.search_id);
      if (searchEntry && searchEntry.state_id === ev.state_id) {
        searchEntry.onInfo?.(ev);
      }
      return;
    }

    if (ev.type === "complete") {
      const searchEntry = this.activeSearches.get(ev.search_id);
      if (searchEntry) {
        this.activeSearches.delete(ev.search_id);
        if (this.currentSearchId === ev.search_id) {
          this.currentSearchId = null;
        }
        searchEntry.resolve(ev);
      }
    }
  }
}
