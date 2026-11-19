// SPDX-License-Identifier: Apache-2.0

import type {
  EngineConfigureOptions,
  EvaluationManifest,
  NormalizedScore,
  ResolvedEngineOptions,
  SearchLimits,
  SearchPvLine,
  WorkerProgressEvent,
  WorkerRuntimeCapabilities,
  XiangqiSide,
} from "./protocol.ts";

export const STARTPOS_FEN =
  "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1";

const NUM_FILES = 9;
const NUM_RANKS = 10;
const NUM_SQUARES = 90;

const SCORE_INFINITE = 32000;
const MATE_SCORE = 30000;
const MATE_THRESHOLD = 29000;
const MAX_PLY = 64;

// Piece encoding: 0 = empty, 1..7 = Red K,A,B,N,R,C,P; 8..14 = Black k,a,b,n,r,c,p
const PT_EMPTY = 0;
const PT_GENERAL = 1;
const PT_ADVISOR = 2;
const PT_ELEPHANT = 3;
const PT_HORSE = 4;
const PT_CHARIOT = 5;
const PT_CANNON = 6;
const PT_SOLDIER = 7;

const COLOR_RED = 0;
const COLOR_BLACK = 1;
const COLOR_NONE = 2;

// ---------------------------------------------------------------------------
// Deterministic RFC 6234 SHA-256 (works in Worker, Window, Node, and WebKit JSC)
// ---------------------------------------------------------------------------

const SHA256_K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function encodeUtf8(input: string): Uint8Array {
  if (typeof TextEncoder !== "undefined") {
    return new TextEncoder().encode(input);
  }
  const out: number[] = [];
  for (let i = 0; i < input.length; i++) {
    const code = input.charCodeAt(i);
    if (code < 0x80) {
      out.push(code);
    } else if (code < 0x800) {
      out.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else {
      out.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    }
  }
  return new Uint8Array(out);
}

function rotr32(x: number, n: number): number {
  return ((x >>> n) | (x << (32 - n))) >>> 0;
}

export function sha256Hex(input: Uint8Array | string): string {
  const bytes = typeof input === "string" ? encodeUtf8(input) : input;
  const bitLenHi = Math.floor((bytes.length * 8) / 0x100000000);
  const bitLenLo = (bytes.length * 8) >>> 0;

  const padLen = ((bytes.length + 9 + 63) & ~63);
  const msg = new Uint8Array(padLen);
  msg.set(bytes, 0);
  msg[bytes.length] = 0x80;
  const dv = new DataView(msg.buffer);
  dv.setUint32(padLen - 8, bitLenHi, false);
  dv.setUint32(padLen - 4, bitLenLo, false);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;

  const w = new Uint32Array(64);
  for (let off = 0; off < padLen; off += 64) {
    for (let i = 0; i < 16; i++) {
      w[i] = dv.getUint32(off + i * 4, false);
    }
    for (let i = 16; i < 64; i++) {
      const s0 = rotr32(w[i - 15], 7) ^ rotr32(w[i - 15], 18) ^ (w[i - 15] >>> 3);
      const s1 = rotr32(w[i - 2], 17) ^ rotr32(w[i - 2], 19) ^ (w[i - 2] >>> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) >>> 0;
    }

    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let h = h7;

    for (let i = 0; i < 64; i++) {
      const s1 = rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25);
      const ch = (e & f) ^ (~e & g);
      const temp1 = (h + s1 + ch + SHA256_K[i] + w[i]) >>> 0;
      const s0 = rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (s0 + maj) >>> 0;

      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }

    h0 = (h0 + a) >>> 0;
    h1 = (h1 + b) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
    h5 = (h5 + f) >>> 0;
    h6 = (h6 + g) >>> 0;
    h7 = (h7 + h) >>> 0;
  }

  const words = [h0, h1, h2, h3, h4, h5, h6, h7];
  let hex = "";
  for (const word of words) {
    hex += (word >>> 0).toString(16).padStart(8, "0");
  }
  return hex;
}

// ---------------------------------------------------------------------------
// Inline WebAssembly Search Kernel Binary Builder
// ---------------------------------------------------------------------------

function uleb128(val: number): number[] {
  const out: number[] = [];
  let v = val >>> 0;
  do {
    let byte = v & 0x7f;
    v >>>= 7;
    if (v !== 0) byte |= 0x80;
    out.push(byte);
  } while (v !== 0);
  return out;
}

function wasmSection(id: number, payload: number[]): number[] {
  return [id, ...uleb128(payload.length), ...payload];
}

function asciiBytes(s: string): number[] {
  const out = [s.length];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i));
  return out;
}

/**
 * Builds the inline WebAssembly search kernel binary (`\0asm\x01\0\0\0`).
 *
 * Linear memory layout (`env.memory`):
 *   - byte 0 (`i32`): stop flag (`0` = running, `1` = stop requested)
 *   - byte 4 (`i32`): kernel node counter
 *   - bytes 64..423 (`90 * i32`): signed square evaluation scratch buffer
 */
export function buildSearchKernelWasmBytes(sharedMemory = false): Uint8Array {
  const memLimitsFlag = sharedMemory ? 0x03 : 0x01;

  // Section 1: Types
  //   0: () -> i32
  //   1: () -> ()
  //   2: (i32) -> i32
  const typeSec = wasmSection(0x01, [
    0x03,
    0x60, 0x00, 0x01, 0x7f,
    0x60, 0x00, 0x00,
    0x60, 0x01, 0x7f, 0x01, 0x7f,
  ]);

  // Section 2: Import env.memory (min 2 pages = 128 KiB, max 16 pages = 1 MiB)
  const importSec = wasmSection(0x02, [
    0x01,
    ...asciiBytes("env"),
    ...asciiBytes("memory"),
    0x02,
    memLimitsFlag,
    0x02,
    0x10,
  ]);

  // Section 3: Functions (6 functions)
  const funcSec = wasmSection(0x03, [
    0x06,
    0x00, // 0: abi_version () -> i32
    0x01, // 1: request_stop () -> ()
    0x01, // 2: clear_stop () -> ()
    0x00, // 3: is_stopped () -> i32
    0x00, // 4: tick_node () -> i32
    0x02, // 5: sum_i32_90 (i32) -> i32
  ]);

  // Section 7: Exports
  const exportSec = wasmSection(0x07, [
    0x06,
    ...asciiBytes("abi_version"), 0x00, 0x00,
    ...asciiBytes("request_stop"), 0x00, 0x01,
    ...asciiBytes("clear_stop"), 0x00, 0x02,
    ...asciiBytes("is_stopped"), 0x00, 0x03,
    ...asciiBytes("tick_node"), 0x00, 0x04,
    ...asciiBytes("sum_i32_90"), 0x00, 0x05,
  ]);

  const fn0 = [0x00, 0x41, 0x01, 0x0b];
  const fn1 = [0x00, 0x41, 0x00, 0x41, 0x01, 0x36, 0x02, 0x00, 0x0b];
  const fn2 = [0x00, 0x41, 0x00, 0x41, 0x00, 0x36, 0x02, 0x00, 0x0b];
  const fn3 = [0x00, 0x41, 0x00, 0x28, 0x02, 0x00, 0x0b];
  const fn4 = [
    0x01, 0x01, 0x7f,             // 1 local i32
    0x41, 0x04,                   // addr 4
    0x41, 0x04, 0x28, 0x02, 0x00, // i32.load(4)
    0x41, 0x01, 0x6a,             // + 1
    0x22, 0x00,                   // local.tee 0
    0x36, 0x02, 0x00,             // i32.store(4, nodes)
    0x20, 0x00,                   // local.get 0
    0x41, 0xff, 0x01,             // i32.const 255 (signed LEB128)
    0x71, 0x45,                   // (nodes & 255) == 0
    0x41, 0x00, 0x28, 0x02, 0x00, // i32.load(0)
    0x41, 0x00, 0x47,             // != 0
    0x71,                         // and
    0x0b,
  ];
  const fn5 = [
    0x01, 0x02, 0x7f,             // locals: 1 = acc, 2 = idx
    0x02, 0x40,                   // block
    0x03, 0x40,                   // loop
    0x20, 0x02,                   // local.get idx
    0x41, 0xda, 0x00,             // i32.const 90 (signed LEB128)
    0x4f,                         // i32.ge_u
    0x0d, 0x01,                   // br_if 1
    0x20, 0x01,                   // local.get acc
    0x20, 0x00,                   // local.get base_ptr
    0x20, 0x02,                   // local.get idx
    0x41, 0x02, 0x74,             // idx << 2
    0x6a,                         // base_ptr + (idx << 2)
    0x28, 0x02, 0x00,             // i32.load
    0x6a,                         // acc + val
    0x21, 0x01,                   // local.set acc
    0x20, 0x02, 0x41, 0x01, 0x6a, // idx + 1
    0x21, 0x02,                   // local.set idx
    0x0c, 0x00,                   // br 0
    0x0b,                         // end loop
    0x0b,                         // end block
    0x20, 0x01,                   // local.get acc
    0x0b,
  ];

  const bodies = [fn0, fn1, fn2, fn3, fn4, fn5];
  const codePayload: number[] = [bodies.length];
  for (const b of bodies) {
    codePayload.push(...uleb128(b.length), ...b);
  }
  const codeSec = wasmSection(0x0a, codePayload);

  return new Uint8Array([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ...typeSec,
    ...importSec,
    ...funcSec,
    ...exportSec,
    ...codeSec,
  ]);
}

export const JADE_SEARCH_KERNEL_WASM_BYTES: Uint8Array = buildSearchKernelWasmBytes(false);
export const JADE_SEARCH_KERNEL_BINARY_HASH: string = sha256Hex(JADE_SEARCH_KERNEL_WASM_BYTES);
export const BUILTIN_NETWORK_HASH: string = sha256Hex("jade-wasm-hce-v1");

interface WasmKernelExports {
  abi_version(): number;
  request_stop(): void;
  clear_stop(): void;
  is_stopped(): number;
  tick_node(): number;
  sum_i32_90(basePtr: number): number;
}

export function detectRuntimeCapabilities(): WorkerRuntimeCapabilities {
  const coi =
    typeof globalThis !== "undefined" &&
    "crossOriginIsolated" in globalThis &&
    Boolean((globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated);
  const hasSab = typeof SharedArrayBuffer !== "undefined";
  const wasmThreads = coi && hasSab;
  const hwConcurrency =
    typeof navigator !== "undefined" && typeof navigator.hardwareConcurrency === "number"
      ? Math.max(1, navigator.hardwareConcurrency)
      : 1;

  return {
    cross_origin_isolated: coi,
    shared_array_buffer: hasSab,
    wasm_threads: wasmThreads,
    execution_mode: wasmThreads ? "shared-memory-worker" : "single-thread-worker",
    max_threads: wasmThreads ? Math.min(8, hwConcurrency) : 1,
  };
}

// ---------------------------------------------------------------------------
// Xiangqi Board, Zobrist Hashing, Legal Move Generation & Evaluation
// ---------------------------------------------------------------------------

function makePiece(color: number, pt: number): number {
  if (pt === PT_EMPTY || color === COLOR_NONE) return 0;
  return (color === COLOR_RED ? 0 : 7) + pt;
}

function colorOf(p: number): number {
  if (p === 0) return COLOR_NONE;
  return p <= 7 ? COLOR_RED : COLOR_BLACK;
}

function typeOf(p: number): number {
  if (p === 0) return PT_EMPTY;
  return p <= 7 ? p : p - 7;
}

function inPalace(color: number, f: number, r: number): boolean {
  if (f < 3 || f > 5) return false;
  return color === COLOR_RED ? r >= 0 && r <= 2 : r >= 7 && r <= 9;
}

function ownSideOfRiver(color: number, r: number): boolean {
  return color === COLOR_RED ? r <= 4 : r >= 5;
}

function crossedRiver(color: number, r: number): boolean {
  return color === COLOR_RED ? r >= 5 : r <= 4;
}

function packMove(fromSq: number, toSq: number): number {
  return (fromSq & 0x7f) | ((toSq & 0x7f) << 7);
}

function moveFrom(m: number): number {
  return m & 0x7f;
}

function moveTo(m: number): number {
  return (m >>> 7) & 0x7f;
}

export function moveToUci(m: number): string {
  if (m === 0) return "0000";
  const f0 = moveFrom(m) % 9;
  const r0 = Math.floor(moveFrom(m) / 9);
  const f1 = moveTo(m) % 9;
  const r1 = Math.floor(moveTo(m) / 9);
  return (
    String.fromCharCode(97 + f0) +
    String.fromCharCode(48 + r0) +
    String.fromCharCode(97 + f1) +
    String.fromCharCode(48 + r1)
  );
}

export function parseUciMove(uci: string): number | null {
  if (uci.length !== 4) return null;
  const f0 = uci.charCodeAt(0) - 97;
  const r0 = uci.charCodeAt(1) - 48;
  const f1 = uci.charCodeAt(2) - 97;
  const r1 = uci.charCodeAt(3) - 48;
  if (f0 < 0 || f0 >= 9 || r0 < 0 || r0 >= 10 || f1 < 0 || f1 >= 9 || r1 < 0 || r1 >= 10) {
    return null;
  }
  const fromSq = r0 * 9 + f0;
  const toSq = r1 * 9 + f1;
  if (fromSq === toSq) return null;
  return packMove(fromSq, toSq);
}

// Twin 32-bit Zobrist tables (avoids BigInt GC allocation in tight alpha-beta inner loop).
const ZOBRIST_HI = new Uint32Array(15 * NUM_SQUARES);
const ZOBRIST_LO = new Uint32Array(15 * NUM_SQUARES);
let ZOBRIST_BLACK_HI = 0;
let ZOBRIST_BLACK_LO = 0;

(function initZobrist() {
  let s0 = 0x4a414445 >>> 0;
  let s1 = 0x58515731 >>> 0;
  const next32 = (): number => {
    let x = s0;
    const y = s1;
    s0 = y;
    x ^= (x << 23) >>> 0;
    x ^= x >>> 17;
    x ^= y ^ (y >>> 26);
    s1 = x >>> 0;
    return (s0 + s1) >>> 0;
  };
  for (let p = 1; p < 15; p++) {
    for (let sq = 0; sq < NUM_SQUARES; sq++) {
      ZOBRIST_HI[p * NUM_SQUARES + sq] = next32();
      ZOBRIST_LO[p * NUM_SQUARES + sq] = next32();
    }
  }
  ZOBRIST_BLACK_HI = next32();
  ZOBRIST_BLACK_LO = next32();
})();

const MATERIAL_VALUE = [0, 10000, 210, 210, 435, 960, 465, 115];

const SOLDIER_PST = [
   0,  0,  0,  0,  0,  0,  0,  0,  0,
   0,  0,  0,  0,  0,  0,  0,  0,  0,
   0,  0,  0,  0,  0,  0,  0,  0,  0,
  -2,  0, -2,  0,  8,  0, -2,  0, -2,
   6,  0, 12,  0, 20,  0, 12,  0,  6,
  35, 45, 58, 70, 78, 70, 58, 45, 35,
  48, 64, 82, 98,108, 98, 82, 64, 48,
  55, 76, 96,120,132,120, 96, 76, 55,
  45, 68, 88,114,124,114, 88, 68, 45,
  20, 32, 42, 56, 62, 56, 42, 32, 20,
];

const HORSE_PST = [
 -20,-14,-12,-10,-10,-10,-12,-14,-20,
 -12, -4,  8,  6, -6,  6,  8, -4,-12,
  -8,  6, 22, 18, 12, 18, 22,  6, -8,
  -6, 10, 20, 26, 24, 26, 20, 10, -6,
  -4, 14, 26, 34, 32, 34, 26, 14, -4,
  -2, 16, 30, 40, 38, 40, 30, 16, -2,
   0, 20, 34, 46, 44, 46, 34, 20,  0,
   2, 18, 36, 48, 42, 48, 36, 18,  2,
  -6, 12, 24, 34, 20, 34, 24, 12, -6,
 -18, -8, -4,  4, -4,  4, -4, -8,-18,
];

const CANNON_PST = [
   0,  0,  4,  8, 12,  8,  4,  0,  0,
   0,  4,  6, 10, 12, 10,  6,  4,  0,
   4,  8, 10, 18, 34, 18, 10,  8,  4,
   2,  4,  8, 14, 22, 14,  8,  4,  2,
   4,  6, 10, 16, 24, 16, 10,  6,  4,
   6,  8, 12, 18, 26, 18, 12,  8,  6,
   6, 10, 14, 20, 28, 20, 14, 10,  6,
   8, 12, 16, 22, 32, 22, 16, 12,  8,
   4,  8, 10, 14, 18, 14, 10,  8,  4,
   2,  4,  6, 10, 14, 10,  6,  4,  2,
];

const CHARIOT_PST = [
  -6,  6,  4, 14,  8, 14,  4,  6, -6,
   4, 10,  8, 16, 10, 16,  8, 10,  4,
   4, 10, 10, 18, 14, 18, 10, 10,  4,
   8, 14, 14, 22, 20, 22, 14, 14,  8,
  14, 20, 20, 26, 26, 26, 20, 20, 14,
  16, 22, 22, 28, 28, 28, 22, 22, 16,
  18, 26, 26, 32, 32, 32, 26, 26, 18,
  18, 26, 26, 34, 34, 34, 26, 26, 18,
  20, 28, 28, 36, 30, 36, 28, 28, 20,
  14, 22, 20, 28, 24, 28, 20, 22, 14,
];

function pieceSquareEvalRed(pt: number, sq: number): number {
  const f = sq % 9;
  const r = Math.floor(sq / 9);
  switch (pt) {
    case PT_GENERAL:
      return f === 4 && r === 0 ? 18 : r === 0 ? 8 : r === 1 ? -4 : -14;
    case PT_ADVISOR:
      return 210 + (f === 4 && r === 1 ? 22 : 8);
    case PT_ELEPHANT:
      return 210 + (f === 4 && r === 2 ? 22 : (f === 2 || f === 6) && r === 4 ? 12 : 6);
    case PT_HORSE:
      return 435 + HORSE_PST[sq];
    case PT_CHARIOT:
      return 960 + CHARIOT_PST[sq];
    case PT_CANNON:
      return 465 + CANNON_PST[sq];
    case PT_SOLDIER:
      return 115 + SOLDIER_PST[sq];
    default:
      return 0;
  }
}

const PIECE_SQUARE_TABLE = new Int32Array(15 * NUM_SQUARES);
(function initPieceSquareTable() {
  for (let pt = 1; pt <= 7; pt++) {
    for (let sq = 0; sq < NUM_SQUARES; sq++) {
      const f = sq % 9;
      const r = Math.floor(sq / 9);
      const mirrorSq = (9 - r) * 9 + f;
      PIECE_SQUARE_TABLE[pt * NUM_SQUARES + sq] = pieceSquareEvalRed(pt, sq);
      PIECE_SQUARE_TABLE[(pt + 7) * NUM_SQUARES + sq] = -pieceSquareEvalRed(pt, mirrorSq);
    }
  }
})();

function charToPiece(ch: string): number {
  switch (ch) {
    case "K": return 1;
    case "A": return 2;
    case "B":
    case "E": return 3;
    case "N":
    case "H": return 4;
    case "R": return 5;
    case "C": return 6;
    case "P": return 7;
    case "k": return 8;
    case "a": return 9;
    case "b":
    case "e": return 10;
    case "n":
    case "h": return 11;
    case "r": return 12;
    case "c": return 13;
    case "p": return 14;
    default: return 0;
  }
}

interface UndoEntry {
  captured: number;
  prevKeyHi: number;
  prevKeyLo: number;
  prevHalfmove: number;
  prevRedGen: number;
  prevBlackGen: number;
}

export class XiangqiPosition {
  readonly board = new Uint8Array(NUM_SQUARES);
  side = COLOR_RED;
  keyHi = 0;
  keyLo = 0;
  halfmoveClock = 0;
  fullmoveNumber = 1;
  redGeneralSq = 4;
  blackGeneralSq = 85;
  private histHi: number[] = [];
  private histLo: number[] = [];

  constructor() {
    this.setFen(STARTPOS_FEN);
  }

  clone(): XiangqiPosition {
    const copy = new XiangqiPosition();
    copy.board.set(this.board);
    copy.side = this.side;
    copy.keyHi = this.keyHi;
    copy.keyLo = this.keyLo;
    copy.halfmoveClock = this.halfmoveClock;
    copy.fullmoveNumber = this.fullmoveNumber;
    copy.redGeneralSq = this.redGeneralSq;
    copy.blackGeneralSq = this.blackGeneralSq;
    copy.histHi = this.histHi.slice();
    copy.histLo = this.histLo.slice();
    return copy;
  }

  setFen(fenInput: string): { ok: true } | { ok: false; error: string } {
    const fen = fenInput.trim() === "startpos" || fenInput.trim() === "" ? STARTPOS_FEN : fenInput.trim();
    const parts = fen.split(/\s+/);
    if (parts.length < 2) {
      return { ok: false, error: "FEN must include piece placement and side to move" };
    }
    const nextBoard = new Uint8Array(NUM_SQUARES);
    let rank = 9;
    let file = 0;
    let redG = -1;
    let blackG = -1;
    let redCount = 0;
    let blackCount = 0;

    for (let i = 0; i < parts[0].length; i++) {
      const ch = parts[0][i];
      if (ch === "/") {
        if (file !== 9 || rank <= 0) {
          return { ok: false, error: "invalid rank separator in FEN" };
        }
        rank--;
        file = 0;
        continue;
      }
      if (ch >= "1" && ch <= "9") {
        file += ch.charCodeAt(0) - 48;
        if (file > 9) return { ok: false, error: "FEN rank exceeds 9 files" };
        continue;
      }
      const p = charToPiece(ch);
      if (p === 0 || file >= 9) {
        return { ok: false, error: `invalid piece '${ch}' in FEN` };
      }
      const sq = rank * 9 + file;
      nextBoard[sq] = p;
      if (p === 1) {
        if (!inPalace(COLOR_RED, file, rank)) return { ok: false, error: "Red general outside palace" };
        redG = sq;
        redCount++;
      } else if (p === 8) {
        if (!inPalace(COLOR_BLACK, file, rank)) return { ok: false, error: "Black general outside palace" };
        blackG = sq;
        blackCount++;
      }
      file++;
    }

    if (rank !== 0 || file !== 9) {
      return { ok: false, error: "FEN must specify 10 ranks of 9 files" };
    }
    if (redCount !== 1 || blackCount !== 1) {
      return { ok: false, error: "FEN must contain exactly one Red and one Black general" };
    }

    const sideTok = parts[1].toLowerCase();
    let nextSide = COLOR_RED;
    if (sideTok === "w" || sideTok === "r") {
      nextSide = COLOR_RED;
    } else if (sideTok === "b") {
      nextSide = COLOR_BLACK;
    } else {
      return { ok: false, error: `invalid side token '${parts[1]}'` };
    }

    this.board.set(nextBoard);
    this.side = nextSide;
    this.redGeneralSq = redG;
    this.blackGeneralSq = blackG;
    this.halfmoveClock = parts.length >= 5 ? Math.max(0, Number.parseInt(parts[4], 10) || 0) : 0;
    this.fullmoveNumber = parts.length >= 6 ? Math.max(1, Number.parseInt(parts[5], 10) || 1) : 1;
    this.recomputeKey();
    this.histHi = [];
    this.histLo = [];

    const nonMoving = nextSide === COLOR_RED ? COLOR_BLACK : COLOR_RED;
    if (this.inCheck(nonMoving)) {
      return { ok: false, error: "illegal FEN: non-moving side is already in check or flying general" };
    }
    return { ok: true };
  }

  private recomputeKey(): void {
    let hi = 0;
    let lo = 0;
    for (let sq = 0; sq < NUM_SQUARES; sq++) {
      const p = this.board[sq];
      if (p !== 0) {
        hi = (hi ^ ZOBRIST_HI[p * NUM_SQUARES + sq]) >>> 0;
        lo = (lo ^ ZOBRIST_LO[p * NUM_SQUARES + sq]) >>> 0;
      }
    }
    if (this.side === COLOR_BLACK) {
      hi = (hi ^ ZOBRIST_BLACK_HI) >>> 0;
      lo = (lo ^ ZOBRIST_BLACK_LO) >>> 0;
    }
    this.keyHi = hi;
    this.keyLo = lo;
  }

  isSquareAttackedBy(targetSq: number, attacker: number): boolean {
    const tf = targetSq % 9;
    const tr = Math.floor(targetSq / 9);
    const enemyGeneral = makePiece(attacker, PT_GENERAL);
    const enemyChariot = makePiece(attacker, PT_CHARIOT);
    const enemyCannon = makePiece(attacker, PT_CANNON);
    const enemySoldier = makePiece(attacker, PT_SOLDIER);
    const enemyHorse = makePiece(attacker, PT_HORSE);

    const orthoDirs = [
      [0, 1],
      [0, -1],
      [1, 0],
      [-1, 0],
    ];
    for (let d = 0; d < 4; d++) {
      const df = orthoDirs[d][0];
      const dr = orthoDirs[d][1];
      let screens = 0;
      let step = 0;
      let f = tf + df;
      let r = tr + dr;
      while (f >= 0 && f < 9 && r >= 0 && r < 10) {
        step++;
        const p = this.board[r * 9 + f];
        if (p !== 0) {
          if (screens === 0) {
            if (p === enemyChariot) return true;
            if (p === enemyGeneral && (df === 0 || (step === 1 && inPalace(attacker, tf, tr)))) {
              return true;
            }
            if (step === 1 && p === enemySoldier) {
              if (attacker === COLOR_RED) {
                if (df === 0 && dr === -1) return true;
                if (dr === 0 && crossedRiver(COLOR_RED, r)) return true;
              } else {
                if (df === 0 && dr === 1) return true;
                if (dr === 0 && crossedRiver(COLOR_BLACK, r)) return true;
              }
            }
            screens = 1;
          } else {
            if (p === enemyCannon) return true;
            break;
          }
        }
        f += df;
        r += dr;
      }
    }

    const horseOffsets = [
      [-2, -1], [-2, 1], [2, -1], [2, 1],
      [-1, -2], [-1, 2], [1, -2], [1, 2],
    ];
    for (let i = 0; i < 8; i++) {
      const hf = tf + horseOffsets[i][0];
      const hr = tr + horseOffsets[i][1];
      if (hf < 0 || hf >= 9 || hr < 0 || hr >= 10) continue;
      if (this.board[hr * 9 + hf] !== enemyHorse) continue;
      const legF = hf + Math.trunc((tf - hf) / 2);
      const legR = hr + Math.trunc((tr - hr) / 2);
      if (this.board[legR * 9 + legF] === 0) return true;
    }

    return false;
  }

  inCheck(color = this.side): boolean {
    const gsq = color === COLOR_RED ? this.redGeneralSq : this.blackGeneralSq;
    if (gsq < 0) return true;
    return this.isSquareAttackedBy(gsq, color === COLOR_RED ? COLOR_BLACK : COLOR_RED);
  }

  generatePseudoLegal(capturesOnly = false): number[] {
    const moves: number[] = [];
    const us = this.side;
    const them = us === COLOR_RED ? COLOR_BLACK : COLOR_RED;

    const tryPush = (fromSq: number, toSq: number): void => {
      const dst = this.board[toSq];
      if (colorOf(dst) === us) return;
      if (capturesOnly && dst === 0) return;
      moves.push(packMove(fromSq, toSq));
    };

    for (let sq = 0; sq < NUM_SQUARES; sq++) {
      const p = this.board[sq];
      if (colorOf(p) !== us) continue;
      const f = sq % 9;
      const r = Math.floor(sq / 9);
      const pt = typeOf(p);

      if (pt === PT_GENERAL) {
        if (inPalace(us, f, r + 1)) tryPush(sq, (r + 1) * 9 + f);
        if (inPalace(us, f, r - 1)) tryPush(sq, (r - 1) * 9 + f);
        if (inPalace(us, f + 1, r)) tryPush(sq, r * 9 + (f + 1));
        if (inPalace(us, f - 1, r)) tryPush(sq, r * 9 + (f - 1));
      } else if (pt === PT_ADVISOR) {
        const diag = [[-1, -1], [-1, 1], [1, -1], [1, 1]];
        for (let i = 0; i < 4; i++) {
          const nf = f + diag[i][0];
          const nr = r + diag[i][1];
          if (inPalace(us, nf, nr)) tryPush(sq, nr * 9 + nf);
        }
      } else if (pt === PT_ELEPHANT) {
        const ele = [[-2, -2], [-2, 2], [2, -2], [2, 2]];
        for (let i = 0; i < 4; i++) {
          const nf = f + ele[i][0];
          const nr = r + ele[i][1];
          if (nf < 0 || nf >= 9 || nr < 0 || nr >= 10 || !ownSideOfRiver(us, nr)) continue;
          const eyeSq = (r + ele[i][1] / 2) * 9 + (f + ele[i][0] / 2);
          if (this.board[eyeSq] === 0) tryPush(sq, nr * 9 + nf);
        }
      } else if (pt === PT_HORSE) {
        const steps = [
          [0, 1, -1, 2, 1, 2],
          [0, -1, -1, -2, 1, -2],
          [1, 0, 2, 1, 2, -1],
          [-1, 0, -2, 1, -2, -1],
        ];
        for (let i = 0; i < 4; i++) {
          const lf = f + steps[i][0];
          const lr = r + steps[i][1];
          if (lf < 0 || lf >= 9 || lr < 0 || lr >= 10 || this.board[lr * 9 + lf] !== 0) continue;
          const fA = f + steps[i][2];
          const rA = r + steps[i][3];
          if (fA >= 0 && fA < 9 && rA >= 0 && rA < 10) tryPush(sq, rA * 9 + fA);
          const fB = f + steps[i][4];
          const rB = r + steps[i][5];
          if (fB >= 0 && fB < 9 && rB >= 0 && rB < 10) tryPush(sq, rB * 9 + fB);
        }
      } else if (pt === PT_CHARIOT) {
        const dirs = [[0, 1], [0, -1], [1, 0], [-1, 0]];
        for (let d = 0; d < 4; d++) {
          let nf = f + dirs[d][0];
          let nr = r + dirs[d][1];
          while (nf >= 0 && nf < 9 && nr >= 0 && nr < 10) {
            const toSq = nr * 9 + nf;
            const dst = this.board[toSq];
            if (dst === 0) {
              if (!capturesOnly) moves.push(packMove(sq, toSq));
            } else {
              if (colorOf(dst) === them) moves.push(packMove(sq, toSq));
              break;
            }
            nf += dirs[d][0];
            nr += dirs[d][1];
          }
        }
      } else if (pt === PT_CANNON) {
        const dirs = [[0, 1], [0, -1], [1, 0], [-1, 0]];
        for (let d = 0; d < 4; d++) {
          let nf = f + dirs[d][0];
          let nr = r + dirs[d][1];
          let screen = false;
          while (nf >= 0 && nf < 9 && nr >= 0 && nr < 10) {
            const toSq = nr * 9 + nf;
            const dst = this.board[toSq];
            if (!screen) {
              if (dst === 0) {
                if (!capturesOnly) moves.push(packMove(sq, toSq));
              } else {
                screen = true;
              }
            } else if (dst !== 0) {
              if (colorOf(dst) === them) moves.push(packMove(sq, toSq));
              break;
            }
            nf += dirs[d][0];
            nr += dirs[d][1];
          }
        }
      } else if (pt === PT_SOLDIER) {
        const fwdR = r + (us === COLOR_RED ? 1 : -1);
        if (fwdR >= 0 && fwdR < 10) tryPush(sq, fwdR * 9 + f);
        if (crossedRiver(us, r)) {
          if (f > 0) tryPush(sq, r * 9 + (f - 1));
          if (f < 8) tryPush(sq, r * 9 + (f + 1));
        }
      }
    }
    return moves;
  }

  makeMove(m: number): UndoEntry {
    const fromSq = moveFrom(m);
    const toSq = moveTo(m);
    const moving = this.board[fromSq];
    const captured = this.board[toSq];
    const us = this.side;

    const undo: UndoEntry = {
      captured,
      prevKeyHi: this.keyHi,
      prevKeyLo: this.keyLo,
      prevHalfmove: this.halfmoveClock,
      prevRedGen: this.redGeneralSq,
      prevBlackGen: this.blackGeneralSq,
    };

    this.histHi.push(this.keyHi);
    this.histLo.push(this.keyLo);

    let hi = (this.keyHi ^ ZOBRIST_HI[moving * NUM_SQUARES + fromSq]) >>> 0;
    let lo = (this.keyLo ^ ZOBRIST_LO[moving * NUM_SQUARES + fromSq]) >>> 0;
    if (captured !== 0) {
      hi = (hi ^ ZOBRIST_HI[captured * NUM_SQUARES + toSq]) >>> 0;
      lo = (lo ^ ZOBRIST_LO[captured * NUM_SQUARES + toSq]) >>> 0;
      if (captured === 1) this.redGeneralSq = -1;
      if (captured === 8) this.blackGeneralSq = -1;
    }
    hi = (hi ^ ZOBRIST_HI[moving * NUM_SQUARES + toSq] ^ ZOBRIST_BLACK_HI) >>> 0;
    lo = (lo ^ ZOBRIST_LO[moving * NUM_SQUARES + toSq] ^ ZOBRIST_BLACK_LO) >>> 0;
    this.keyHi = hi;
    this.keyLo = lo;

    this.board[fromSq] = 0;
    this.board[toSq] = moving;
    if (moving === 1) this.redGeneralSq = toSq;
    else if (moving === 8) this.blackGeneralSq = toSq;

    this.halfmoveClock = captured !== 0 ? 0 : this.halfmoveClock + 1;
    if (us === COLOR_BLACK) this.fullmoveNumber++;
    this.side = us === COLOR_RED ? COLOR_BLACK : COLOR_RED;
    return undo;
  }

  unmakeMove(m: number, undo: UndoEntry): void {
    const fromSq = moveFrom(m);
    const toSq = moveTo(m);
    const moving = this.board[toSq];
    if (this.side === COLOR_RED) this.fullmoveNumber--;
    this.side = this.side === COLOR_RED ? COLOR_BLACK : COLOR_RED;
    this.board[fromSq] = moving;
    this.board[toSq] = undo.captured;
    this.redGeneralSq = undo.prevRedGen;
    this.blackGeneralSq = undo.prevBlackGen;
    this.halfmoveClock = undo.prevHalfmove;
    this.keyHi = undo.prevKeyHi;
    this.keyLo = undo.prevKeyLo;
    this.histHi.pop();
    this.histLo.pop();
  }

  generateLegal(capturesOnly = false): number[] {
    const pseudo = this.generatePseudoLegal(capturesOnly);
    const legal: number[] = [];
    const us = this.side;
    for (let i = 0; i < pseudo.length; i++) {
      const m = pseudo[i];
      const u = this.makeMove(m);
      if (!this.inCheck(us)) {
        legal.push(m);
      }
      this.unmakeMove(m, u);
    }
    return legal;
  }

  applyUciMove(uci: string): { ok: true } | { ok: false; error: string } {
    const parsed = parseUciMove(uci);
    if (parsed === null) {
      return { ok: false, error: `malformed UCI coordinate move: ${uci}` };
    }
    const legal = this.generateLegal(false);
    if (!legal.includes(parsed)) {
      return { ok: false, error: `illegal Xiangqi move in position: ${uci}` };
    }
    this.makeMove(parsed);
    return { ok: true };
  }

  isRepetition(): boolean {
    const len = this.histHi.length;
    const minIdx = Math.max(0, len - this.halfmoveClock);
    for (let i = len - 2; i >= minIdx; i -= 2) {
      if (this.histHi[i] === this.keyHi && this.histLo[i] === this.keyLo) {
        return true;
      }
    }
    return false;
  }

  sanitizePv(rawMoves: number[]): string[] {
    const out: string[] = [];
    const sim = this.clone();
    for (const m of rawMoves) {
      const legal = sim.generateLegal(false);
      if (!legal.includes(m)) break;
      out.push(moveToUci(m));
      sim.makeMove(m);
    }
    return out;
  }
}

export function normalizeScore(rawScore: number, sideToMove: number): NormalizedScore {
  const isMate = Math.abs(rawScore) >= MATE_THRESHOLD;
  let value = rawScore;
  if (isMate) {
    const plies = MATE_SCORE - Math.abs(rawScore);
    const mateMoves = Math.max(1, Math.trunc((plies + 1) / 2));
    value = rawScore > 0 ? mateMoves : -mateMoves;
  }
  const redValue = sideToMove === COLOR_RED ? value : -value;
  return {
    kind: isMate ? "mate" : "cp",
    value,
    red_value: redValue,
    mate: isMate ? value : null,
    perspective: "side_to_move",
  };
}

// ---------------------------------------------------------------------------
// Wasm-Backed Xiangqi Search Engine Instance
// ---------------------------------------------------------------------------

const TT_NONE = 0;
const TT_EXACT = 1;
const TT_LOWER = 2;
const TT_UPPER = 3;

export class WasmXiangqiEngine {
  private memory!: WebAssembly.Memory;
  private kernel!: WasmKernelExports;
  private memI32!: Int32Array;
  private externalControlI32: Int32Array | null = null;
  private capabilities: WorkerRuntimeCapabilities = detectRuntimeCapabilities();

  private options: ResolvedEngineOptions = {
    hash_mb: 16,
    threads: 1,
    multi_pv: 1,
    move_overhead_ms: 10,
    rules_profile: "program-2023.11-v1",
    engine_id: "jade-wasm-worker-0.1.0",
    network_hash: BUILTIN_NETWORK_HASH,
  };

  private binaryHash = JADE_SEARCH_KERNEL_BINARY_HASH;
  private configHash = "";
  private initialized = false;

  // Transposition Table flat typed arrays (zero per-node object churn).
  private ttMask = 0;
  private ttKeyHi = new Uint32Array(0);
  private ttKeyLo = new Uint32Array(0);
  private ttMove = new Uint16Array(0);
  private ttScore = new Int16Array(0);
  private ttDepth = new Int8Array(0);
  private ttBound = new Uint8Array(0);
  private ttGen = new Uint8Array(0);
  private generation = 0;

  private killers0 = new Uint16Array(MAX_PLY);
  private killers1 = new Uint16Array(MAX_PLY);
  private history = new Int32Array(15 * NUM_SQUARES);

  // Active search slice state.
  private nodes = 0;
  private seldepth = 0;
  private aborted = false;
  private startMs = 0;
  private hardDeadlineMs = 0;
  private softDeadlineMs = 0;
  private maxNodes = 0;
  private shouldAbortFn: (() => boolean) | null = null;

  async init(params?: {
    sharedControlBuffer?: SharedArrayBuffer;
    configure?: EngineConfigureOptions;
    onProgress?: (ev: WorkerProgressEvent) => void;
  }): Promise<{
    capabilities: WorkerRuntimeCapabilities;
    options: ResolvedEngineOptions;
    manifest: EvaluationManifest;
  }> {
    const caps = detectRuntimeCapabilities();
    let useSharedWasmMem = false;
    let mem: WebAssembly.Memory | null = null;

    if (caps.wasm_threads) {
      try {
        mem = new WebAssembly.Memory({ initial: 2, maximum: 16, shared: true });
        useSharedWasmMem = true;
      } catch {
        mem = null;
        useSharedWasmMem = false;
      }
    }
    if (!mem) {
      mem = new WebAssembly.Memory({ initial: 2, maximum: 16 });
    }

    const wasmBytes = buildSearchKernelWasmBytes(useSharedWasmMem);
    const totalBytes = wasmBytes.byteLength;

    params?.onProgress?.({
      type: "progress",
      phase: "wasm_fetch",
      loaded_bytes: totalBytes,
      total_bytes: totalBytes,
      percent: 35,
    });

    const compiledModule = await WebAssembly.compile(wasmBytes as BufferSource);
    params?.onProgress?.({
      type: "progress",
      phase: "wasm_compile",
      loaded_bytes: totalBytes,
      total_bytes: totalBytes,
      percent: 75,
    });

    const instance = await WebAssembly.instantiate(compiledModule, {
      env: { memory: mem },
    });

    this.memory = mem;
    this.kernel = instance.exports as unknown as WasmKernelExports;
    this.memI32 = new Int32Array(this.memory.buffer);
    if (this.kernel.abi_version() !== 1) {
      throw new Error("unexpected Wasm search kernel ABI version");
    }

    if (params?.sharedControlBuffer && typeof SharedArrayBuffer !== "undefined") {
      this.externalControlI32 = new Int32Array(params.sharedControlBuffer);
    }

    this.capabilities = {
      ...caps,
      wasm_threads: useSharedWasmMem,
      execution_mode:
        useSharedWasmMem || this.externalControlI32 !== null
          ? "shared-memory-worker"
          : "single-thread-worker",
    };

    this.binaryHash = JADE_SEARCH_KERNEL_BINARY_HASH;
    this.allocateTt(this.options.hash_mb);
    if (params?.configure) {
      this.applyConfigure(params.configure);
    } else {
      this.recomputeConfigHash();
    }

    this.initialized = true;
    params?.onProgress?.({
      type: "progress",
      phase: "ready",
      loaded_bytes: totalBytes,
      total_bytes: totalBytes,
      percent: 100,
    });

    return {
      capabilities: this.capabilities,
      options: { ...this.options },
      manifest: this.getManifest(),
    };
  }

  isInitialized(): boolean {
    return this.initialized;
  }

  getCapabilities(): WorkerRuntimeCapabilities {
    return { ...this.capabilities };
  }

  getOptions(): ResolvedEngineOptions {
    return { ...this.options };
  }

  getManifest(): EvaluationManifest {
    return {
      engine_id: this.options.engine_id,
      binary_hash: this.binaryHash,
      network_hash: this.options.network_hash,
      rules_profile: this.options.rules_profile,
      config_hash: this.configHash,
    };
  }

  applyConfigure(opts: EngineConfigureOptions): {
    options: ResolvedEngineOptions;
    manifest: EvaluationManifest;
  } {
    const prevHashMb = this.options.hash_mb;
    if (opts.hash_mb !== undefined) {
      this.options.hash_mb = Math.min(256, Math.max(1, Math.trunc(opts.hash_mb)));
    }
    if (opts.threads !== undefined) {
      this.options.threads = Math.min(
        Math.max(1, this.capabilities.max_threads),
        Math.max(1, Math.trunc(opts.threads)),
      );
    }
    if (opts.multi_pv !== undefined) {
      this.options.multi_pv = Math.min(16, Math.max(1, Math.trunc(opts.multi_pv)));
    }
    if (opts.move_overhead_ms !== undefined) {
      this.options.move_overhead_ms = Math.min(5000, Math.max(0, Math.trunc(opts.move_overhead_ms)));
    }
    if (opts.rules_profile !== undefined && opts.rules_profile.length > 0) {
      this.options.rules_profile = opts.rules_profile;
    }
    if (opts.engine_id !== undefined && opts.engine_id.length > 0) {
      this.options.engine_id = opts.engine_id;
    }
    if (opts.network_hash !== undefined && opts.network_hash.length > 0) {
      this.options.network_hash =
        opts.network_hash.length === 64 ? opts.network_hash : sha256Hex(opts.network_hash);
    }

    if (this.options.hash_mb !== prevHashMb || this.ttKeyHi.length === 0) {
      this.allocateTt(this.options.hash_mb);
    }
    this.recomputeConfigHash();
    return {
      options: { ...this.options },
      manifest: this.getManifest(),
    };
  }

  requestStop(): void {
    if (this.initialized) {
      this.kernel.request_stop();
    }
    if (this.externalControlI32) {
      if (typeof Atomics !== "undefined" && this.externalControlI32.buffer instanceof SharedArrayBuffer) {
        Atomics.store(this.externalControlI32, 0, 1);
      } else {
        this.externalControlI32[0] = 1;
      }
    }
  }

  clearStop(): void {
    if (this.initialized) {
      this.kernel.clear_stop();
    }
    if (this.externalControlI32) {
      if (typeof Atomics !== "undefined" && this.externalControlI32.buffer instanceof SharedArrayBuffer) {
        Atomics.store(this.externalControlI32, 0, 0);
      } else {
        this.externalControlI32[0] = 0;
      }
    }
  }

  private recomputeConfigHash(): void {
    const canonical =
      `engine_id=${this.options.engine_id};` +
      `rules_profile=${this.options.rules_profile};` +
      `network_hash=${this.options.network_hash};` +
      `hash_mb=${this.options.hash_mb};` +
      `threads=${this.options.threads};` +
      `multi_pv=${this.options.multi_pv};` +
      `move_overhead_ms=${this.options.move_overhead_ms}`;
    this.configHash = sha256Hex(canonical);
  }

  private allocateTt(hashMb: number): void {
    // 16 bytes per entry across typed arrays.
    const bytes = Math.max(1, Math.min(256, hashMb)) * 1024 * 1024;
    const targetEntries = Math.max(1024, Math.floor(bytes / 16));
    let pow2 = 1024;
    while (pow2 * 2 <= targetEntries && pow2 < 1 << 20) {
      pow2 *= 2;
    }
    this.ttMask = pow2 - 1;
    this.ttKeyHi = new Uint32Array(pow2);
    this.ttKeyLo = new Uint32Array(pow2);
    this.ttMove = new Uint16Array(pow2);
    this.ttScore = new Int16Array(pow2);
    this.ttDepth = new Int8Array(pow2);
    this.ttBound = new Uint8Array(pow2);
    this.ttGen = new Uint8Array(pow2);
  }

  private hashfullPermille(): number {
    const sample = Math.min(1000, this.ttBound.length);
    if (sample === 0) return 0;
    let used = 0;
    for (let i = 0; i < sample; i++) {
      if (this.ttBound[i] !== TT_NONE && this.ttGen[i] === this.generation) used++;
    }
    return Math.trunc((used * 1000) / sample);
  }

  private evaluateWasm(pos: XiangqiPosition): number {
    // Write signed piece-square values into Wasm linear memory at byte offset 64 (`Int32Array` index 16)
    // and execute the Wasm `sum_i32_90(64)` kernel function.
    const baseIdx = 16;
    for (let sq = 0; sq < NUM_SQUARES; sq++) {
      const p = pos.board[sq];
      this.memI32[baseIdx + sq] = p === 0 ? 0 : PIECE_SQUARE_TABLE[p * NUM_SQUARES + sq];
    }
    const redMinusBlack = this.kernel.sum_i32_90(64);
    const stmScore = pos.side === COLOR_RED ? redMinusBlack : -redMinusBlack;
    return stmScore + 12;
  }

  private checkAbort(): boolean {
    if (this.aborted) return true;
    if (this.kernel.is_stopped() !== 0) {
      this.aborted = true;
      return true;
    }
    if (this.externalControlI32) {
      const extFlag =
        typeof Atomics !== "undefined" && this.externalControlI32.buffer instanceof SharedArrayBuffer
          ? Atomics.load(this.externalControlI32, 0)
          : this.externalControlI32[0];
      if (extFlag !== 0) {
        this.aborted = true;
        return true;
      }
    }
    if (this.shouldAbortFn && this.shouldAbortFn()) {
      this.aborted = true;
      return true;
    }
    if (this.maxNodes > 0 && this.nodes >= this.maxNodes) {
      this.aborted = true;
      return true;
    }
    if (this.hardDeadlineMs > 0 && Date.now() - this.startMs >= this.hardDeadlineMs) {
      this.aborted = true;
      return true;
    }
    return false;
  }

  private scoreMoves(pos: XiangqiPosition, moves: number[], ttMove: number, ply: number): Int32Array {
    const scores = new Int32Array(moves.length);
    const k0 = ply < MAX_PLY ? this.killers0[ply] : 0;
    const k1 = ply < MAX_PLY ? this.killers1[ply] : 0;
    for (let i = 0; i < moves.length; i++) {
      const m = moves[i];
      if (ttMove !== 0 && m === ttMove) {
        scores[i] = 1_000_000;
        continue;
      }
      const victim = pos.board[moveTo(m)];
      const attacker = pos.board[moveFrom(m)];
      if (victim !== 0) {
        scores[i] = 500_000 + MATERIAL_VALUE[typeOf(victim)] * 16 - MATERIAL_VALUE[typeOf(attacker)];
      } else if (k0 !== 0 && m === k0) {
        scores[i] = 400_000;
      } else if (k1 !== 0 && m === k1) {
        scores[i] = 390_000;
      } else {
        scores[i] = this.history[attacker * NUM_SQUARES + moveTo(m)];
      }
    }
    return scores;
  }

  private quiescence(pos: XiangqiPosition, alpha: number, beta: number, ply: number): number {
    if (ply > this.seldepth) this.seldepth = ply;
    this.nodes++;
    if (this.kernel.tick_node() !== 0 || ((this.nodes & 255) === 0 && this.checkAbort())) {
      this.aborted = true;
      return 0;
    }
    if (this.maxNodes > 0 && this.nodes >= this.maxNodes) {
      this.aborted = true;
      return 0;
    }
    if (ply >= MAX_PLY - 1) return this.evaluateWasm(pos);

    const inChk = pos.inCheck();
    if (!inChk) {
      const standPat = this.evaluateWasm(pos);
      if (standPat >= beta) return beta;
      if (standPat > alpha) alpha = standPat;
    }

    const moves = pos.generateLegal(!inChk);
    if (moves.length === 0) {
      return inChk ? -MATE_SCORE + ply : alpha;
    }

    const scores = this.scoreMoves(pos, moves, 0, ply);
    for (let i = 0; i < moves.length; i++) {
      let bestIdx = i;
      for (let j = i + 1; j < moves.length; j++) {
        if (scores[j] > scores[bestIdx]) bestIdx = j;
      }
      if (bestIdx !== i) {
        const tmpM = moves[i];
        moves[i] = moves[bestIdx];
        moves[bestIdx] = tmpM;
        const tmpS = scores[i];
        scores[i] = scores[bestIdx];
        scores[bestIdx] = tmpS;
      }

      const u = pos.makeMove(moves[i]);
      const sc = -this.quiescence(pos, -beta, -alpha, ply + 1);
      pos.unmakeMove(moves[i], u);
      if (this.aborted) return 0;
      if (sc >= beta) return beta;
      if (sc > alpha) alpha = sc;
    }
    return alpha;
  }

  private alphaBeta(
    pos: XiangqiPosition,
    depth: number,
    alpha: number,
    beta: number,
    ply: number,
    pvOut: number[],
  ): number {
    pvOut.length = 0;
    if (ply > this.seldepth) this.seldepth = ply;
    this.nodes++;
    if (this.kernel.tick_node() !== 0 || ((this.nodes & 255) === 0 && this.checkAbort())) {
      this.aborted = true;
      return 0;
    }
    if (this.maxNodes > 0 && this.nodes >= this.maxNodes) {
      this.aborted = true;
      return 0;
    }

    if (ply > 0) {
      if (pos.isRepetition() || pos.halfmoveClock >= 120) return 0;
      alpha = Math.max(alpha, -MATE_SCORE + ply);
      beta = Math.min(beta, MATE_SCORE - ply - 1);
      if (alpha >= beta) return alpha;
    }

    if (ply >= MAX_PLY - 1) return this.evaluateWasm(pos);

    const inChk = pos.inCheck();
    if (inChk && ply < 24) depth++;
    if (depth <= 0) {
      this.nodes--;
      return this.quiescence(pos, alpha, beta, ply);
    }

    const isPv = beta - alpha > 1;
    const slot = pos.keyLo & this.ttMask;
    let ttMove = 0;
    if (
      this.ttBound[slot] !== TT_NONE &&
      this.ttKeyHi[slot] === pos.keyHi &&
      this.ttKeyLo[slot] === pos.keyLo
    ) {
      ttMove = this.ttMove[slot];
      if (!isPv && this.ttDepth[slot] >= depth && ply > 0) {
        let ttSc = this.ttScore[slot];
        if (ttSc >= MATE_THRESHOLD) ttSc -= ply;
        else if (ttSc <= -MATE_THRESHOLD) ttSc += ply;
        const bnd = this.ttBound[slot];
        if (bnd === TT_EXACT) return ttSc;
        if (bnd === TT_LOWER && ttSc >= beta) return ttSc;
        if (bnd === TT_UPPER && ttSc <= alpha) return ttSc;
      }
    }

    const legal = pos.generateLegal(false);
    if (legal.length === 0) {
      return -MATE_SCORE + ply;
    }

    const scores = this.scoreMoves(pos, legal, ttMove, ply);
    const origAlpha = alpha;
    let bestMove = legal[0];
    let bestScore = -SCORE_INFINITE;
    const childPv: number[] = [];

    for (let i = 0; i < legal.length; i++) {
      let bestIdx = i;
      for (let j = i + 1; j < legal.length; j++) {
        if (scores[j] > scores[bestIdx]) bestIdx = j;
      }
      if (bestIdx !== i) {
        const tmpM = legal[i];
        legal[i] = legal[bestIdx];
        legal[bestIdx] = tmpM;
        const tmpS = scores[i];
        scores[i] = scores[bestIdx];
        scores[bestIdx] = tmpS;
      }

      const m = legal[i];
      const isCapture = pos.board[moveTo(m)] !== 0;
      const movingPiece = pos.board[moveFrom(m)];
      const u = pos.makeMove(m);

      let sc = 0;
      if (i === 0) {
        sc = -this.alphaBeta(pos, depth - 1, -beta, -alpha, ply + 1, childPv);
      } else {
        const red = depth >= 3 && i >= 4 && !inChk && !isCapture && !pos.inCheck() ? 1 : 0;
        sc = -this.alphaBeta(pos, depth - 1 - red, -alpha - 1, -alpha, ply + 1, childPv);
        if (!this.aborted && sc > alpha && (red > 0 || isPv)) {
          sc = -this.alphaBeta(pos, depth - 1, -beta, -alpha, ply + 1, childPv);
        }
      }
      pos.unmakeMove(m, u);
      if (this.aborted) return 0;

      if (sc > bestScore) {
        bestScore = sc;
        bestMove = m;
      }
      if (sc > alpha) {
        alpha = sc;
        pvOut.length = 0;
        pvOut.push(m, ...childPv);
        if (alpha >= beta) {
          if (!isCapture && ply < MAX_PLY) {
            if (this.killers0[ply] !== m) {
              this.killers1[ply] = this.killers0[ply];
              this.killers0[ply] = m;
            }
            const hIdx = movingPiece * NUM_SQUARES + moveTo(m);
            this.history[hIdx] = Math.min(300_000, this.history[hIdx] + depth * depth);
          }
          break;
        }
      }
    }

    let storedSc = bestScore;
    if (storedSc >= MATE_THRESHOLD) storedSc += ply;
    else if (storedSc <= -MATE_THRESHOLD) storedSc -= ply;

    this.ttKeyHi[slot] = pos.keyHi;
    this.ttKeyLo[slot] = pos.keyLo;
    this.ttMove[slot] = bestMove;
    this.ttScore[slot] = Math.max(-SCORE_INFINITE, Math.min(SCORE_INFINITE, storedSc));
    this.ttDepth[slot] = depth;
    this.ttBound[slot] = bestScore <= origAlpha ? TT_UPPER : bestScore >= beta ? TT_LOWER : TT_EXACT;
    this.ttGen[slot] = this.generation;
    return bestScore;
  }

  async searchAsync(params: {
    position: XiangqiPosition;
    limits?: SearchLimits;
    shouldAbort: () => boolean;
    yieldSlice: () => Promise<void>;
    onInfo: (info: {
      depth: number;
      seldepth: number;
      nodes: number;
      nps: number;
      time_ms: number;
      hashfull: number;
      side_to_move: XiangqiSide;
      line: SearchPvLine;
    }) => void;
  }): Promise<{
    aborted: boolean;
    depth: number;
    seldepth: number;
    nodes: number;
    nps: number;
    time_ms: number;
    hashfull: number;
    side_to_move: XiangqiSide;
    best_move: string | null;
    score: NormalizedScore;
    pv: string[];
    lines: SearchPvLine[];
  }> {
    this.startMs = Date.now();
    this.nodes = 0;
    this.seldepth = 0;
    this.aborted = false;
    this.shouldAbortFn = params.shouldAbort;
    this.generation = (this.generation + 1) & 0xff;
    this.memI32[1] = 0;

    for (let i = 0; i < this.history.length; i++) {
      this.history[i] >>= 1;
    }

    const lim = params.limits ?? {};
    const infinite = Boolean(lim.infinite);
    const maxDepth = Math.min(MAX_PLY - 4, Math.max(1, lim.depth ?? (infinite ? 48 : 6)));
    this.maxNodes = lim.nodes && lim.nodes > 0 ? lim.nodes : 0;
    this.hardDeadlineMs = 0;
    this.softDeadlineMs = 0;
    if (!infinite && lim.movetime_ms && lim.movetime_ms > 0) {
      this.hardDeadlineMs = Math.max(1, lim.movetime_ms - this.options.move_overhead_ms);
      this.softDeadlineMs = this.hardDeadlineMs;
    }

    const rootPos = params.position.clone();
    const sideStr: XiangqiSide = rootPos.side === COLOR_RED ? "red" : "black";
    const rootLegal = rootPos.generateLegal(false);

    if (rootLegal.length === 0) {
      if (infinite) {
        while (!this.checkAbort()) {
          await params.yieldSlice();
        }
      }
      return {
        aborted: this.aborted,
        depth: 0,
        seldepth: 0,
        nodes: 0,
        nps: 0,
        time_ms: Math.max(0, Date.now() - this.startMs),
        hashfull: this.hashfullPermille(),
        side_to_move: sideStr,
        best_move: null,
        score: normalizeScore(-MATE_SCORE, rootPos.side),
        pv: [],
        lines: [],
      };
    }

    const mpvCount = Math.min(
      rootLegal.length,
      Math.max(1, Math.min(16, lim.multi_pv ?? this.options.multi_pv)),
    );

    // Initial root move ordering.
    const initScores = this.scoreMoves(rootPos, rootLegal, 0, 0);
    const orderedRoots = rootLegal
      .map((m, idx) => ({ m, s: initScores[idx] }))
      .sort((a, b) => b.s - a.s)
      .map((x) => x.m);

    let completedDepth = 0;
    let bestMoveStr: string | null = moveToUci(orderedRoots[0]);
    let bestScore: NormalizedScore = normalizeScore(this.evaluateWasm(rootPos), rootPos.side);
    let bestPv: string[] = [bestMoveStr];
    let bestLines: SearchPvLine[] = [];

    const workPos = rootPos.clone();
    let lastYieldMs = Date.now();

    for (let depth = 1; depth <= maxDepth; depth++) {
      if (this.checkAbort()) break;

      const depthLines: Array<{ rawScore: number; rawMove: number; line: SearchPvLine }> = [];
      const chosenRoots: number[] = [];

      for (let mpvIdx = 1; mpvIdx <= mpvCount; mpvIdx++) {
        let alpha = -SCORE_INFINITE;
        const beta = SCORE_INFINITE;
        let bestRoot = 0;
        let bestRootRaw = -SCORE_INFINITE;
        let bestRawPv: number[] = [];
        let searchedIdx = 0;

        for (let rIdx = 0; rIdx < orderedRoots.length; rIdx++) {
          const rm = orderedRoots[rIdx];
          if (chosenRoots.includes(rm)) continue;

          // Cooperative yield between root branches so single-thread workers process `cancel` / superseding `search` messages promptly.
          if (Date.now() - lastYieldMs >= 4) {
            await params.yieldSlice();
            lastYieldMs = Date.now();
            if (this.checkAbort()) break;
          }

          const u = workPos.makeMove(rm);
          const childPv: number[] = [];
          let sc = 0;
          if (searchedIdx === 0) {
            sc = -this.alphaBeta(workPos, depth - 1, -beta, -alpha, 1, childPv);
          } else {
            sc = -this.alphaBeta(workPos, depth - 1, -alpha - 1, -alpha, 1, childPv);
            if (!this.aborted && sc > alpha) {
              sc = -this.alphaBeta(workPos, depth - 1, -beta, -alpha, 1, childPv);
            }
          }
          workPos.unmakeMove(rm, u);
          searchedIdx++;

          if (this.aborted) break;

          if (sc > bestRootRaw || bestRoot === 0) {
            bestRootRaw = sc;
            bestRoot = rm;
            bestRawPv = [rm, ...childPv];
          }
          if (sc > alpha) alpha = sc;
        }

        if (this.aborted || bestRoot === 0) break;

        chosenRoots.push(bestRoot);
        const verifiedPv = rootPos.sanitizePv(bestRawPv);
        const mvStr = moveToUci(bestRoot);
        depthLines.push({
          rawScore: bestRootRaw,
          rawMove: bestRoot,
          line: {
            multipv: mpvIdx,
            depth,
            seldepth: Math.max(depth, this.seldepth),
            score: normalizeScore(bestRootRaw, rootPos.side),
            best_move: mvStr,
            pv: verifiedPv.length > 0 ? verifiedPv : [mvStr],
          },
        });
      }

      if (depthLines.length === 0) break;

      depthLines.sort((a, b) => b.rawScore - a.rawScore);
      for (let i = 0; i < depthLines.length; i++) {
        depthLines[i].line.multipv = i + 1;
      }

      if (!this.aborted || depth === 1) {
        completedDepth = depth;
        bestLines = depthLines.map((d) => d.line);
        bestMoveStr = bestLines[0].best_move;
        bestScore = bestLines[0].score;
        bestPv = bestLines[0].pv;

        const nextOrder = depthLines.map((d) => d.rawMove);
        for (const rm of orderedRoots) {
          if (!nextOrder.includes(rm)) nextOrder.push(rm);
        }
        orderedRoots.splice(0, orderedRoots.length, ...nextOrder);

        const elapsed = Math.max(0, Date.now() - this.startMs);
        const nps = elapsed > 0 ? Math.trunc((this.nodes * 1000) / elapsed) : this.nodes * 1000;
        const hf = this.hashfullPermille();

        if (!this.aborted) {
          for (const l of bestLines) {
            if (this.checkAbort()) break;
            params.onInfo({
              depth,
              seldepth: l.seldepth,
              nodes: this.nodes,
              nps,
              time_ms: elapsed,
              hashfull: hf,
              side_to_move: sideStr,
              line: l,
            });
          }
        }
      }

      if (this.aborted) break;
      if (this.softDeadlineMs > 0 && Date.now() - this.startMs >= this.softDeadlineMs) {
        break;
      }

      // Yield between completed depths so worker message queue stays responsive.
      await params.yieldSlice();
      lastYieldMs = Date.now();
    }

    if (infinite && !this.aborted) {
      while (!this.checkAbort()) {
        await params.yieldSlice();
      }
    }

    const totalMs = Math.max(0, Date.now() - this.startMs);
    const totalNps = totalMs > 0 ? Math.trunc((this.nodes * 1000) / totalMs) : this.nodes * 1000;
    if (bestLines.length === 0 && bestMoveStr !== null) {
      bestLines = [
        {
          multipv: 1,
          depth: Math.max(1, completedDepth),
          seldepth: Math.max(1, this.seldepth),
          score: bestScore,
          best_move: bestMoveStr,
          pv: bestPv,
        },
      ];
    }

    return {
      aborted: this.aborted,
      depth: completedDepth,
      seldepth: this.seldepth,
      nodes: this.nodes,
      nps: totalNps,
      time_ms: totalMs,
      hashfull: this.hashfullPermille(),
      side_to_move: sideStr,
      best_move: bestMoveStr,
      score: bestScore,
      pv: bestPv,
      lines: bestLines,
    };
  }
}
