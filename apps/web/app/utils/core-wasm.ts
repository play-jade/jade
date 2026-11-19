export interface ScreenPoint {
  x: number
  y: number
}

export interface BoardViewport {
  width: number
  height: number
  padding: number
  cell_size: number
  origin_x: number
  origin_y: number
}

export interface PieceViewRecord {
  visual_token: string
  piece_id: number
  side: 'red' | 'black'
  role: 'general' | 'advisor' | 'elephant' | 'chariot' | 'horse' | 'cannon' | 'soldier'
  square: string
  file: number
  rank: number
  glyph: string
  center: ScreenPoint | null
  selected: boolean
}

export interface MoveRecord {
  from: string
  to: string
}

export interface AdjudicationVerdict {
  outcome: 'red_win' | 'black_win' | 'draw'
  reason: string
  detail: string
}

export interface BoardViewSnapshot {
  source_revision: number
  presentation_revision: number
  side_to_move: 'red' | 'black'
  orientation: 'red_below' | 'black_below'
  viewport: BoardViewport | null
  pieces: PieceViewRecord[]
  selection: string | null
  legal_destinations: string[]
  last_move: MoveRecord | null
  in_check_side: 'red' | 'black' | null
  check_square: string | null
  adjudication: AdjudicationVerdict | null
}

export interface BoardViewResult {
  snapshot: BoardViewSnapshot
  fen: string
  moves_chinese: string[]
  legal_moves_uci: string[]
}

export interface ApplyMoveResult {
  move_uci: string
  move_chinese: string
  next_fen: string
  moves_uci: string[]
  moves_chinese: string[]
  side_to_move: 'red' | 'black'
  in_check: boolean
  adjudication: AdjudicationVerdict | null
  legal_moves_uci: string[]
}

export interface ArchivePlayerAlias {
  text: string
  script: string
  source: string
  verification_state: string
}

export interface ArchivePlayer {
  player_id: string
  primary_display_name: string
  native_display_name: string
  disambiguation_note: string
  aliases: ArchivePlayerAlias[]
}

export interface ArchiveSearchResult {
  dataset_id: string
  is_synthetic_fixture: boolean
  provenance_banner: string
  players: ArchivePlayer[]
}

export interface ArchiveGame {
  game_id: string
  dataset_id: string
  is_synthetic_fixture: boolean
  event_name: string
  date_precision: string
  date_display: string
  participant_kind: string
  game_context: string
  red_player_id: string
  black_player_id: string
  initial_fen: string
  moves_uci: string[]
  moves_chinese: string[]
  outcome: string
  opening_label: string
}

export interface PositionContinuation {
  move_uci: string
  move_chinese: string
  deduplicated_game_count: number
  total_occurrence_count: number
  known_result_count: number
  red_wins: number
  draws: number
  black_wins: number
  contributing_game_ids: string[]
}

export interface PositionExplorerResult {
  dataset_id: string
  is_synthetic_fixture: boolean
  position_key_sha256: string
  status: string
  evaluation_status: string
  sample_size_games: number
  continuations: PositionContinuation[]
}

interface RawWasmExports {
  memory: WebAssembly.Memory
  jade_wasm_alloc: (size: number) => number
  jade_wasm_free: (ptr: number, cap: number) => void
  jade_wasm_call: (ptr: number, len: number) => number
  jade_wasm_response_ptr: () => number
  jade_wasm_response_len: () => number
}

let cachedExportsPromise: Promise<RawWasmExports> | null = null
const encoder = new TextEncoder()
const decoder = new TextDecoder()

export function squareIndexToUci(sqIdx: number | string): string {
  if (typeof sqIdx === 'string') return sqIdx
  const file = sqIdx % 9
  const rank = Math.floor(sqIdx / 9)
  return `${String.fromCharCode(97 + file)}${rank}`
}

async function instantiateCoreWasm(): Promise<RawWasmExports> {
  let wasmBytes: BufferSource
  if (import.meta.server || typeof window === 'undefined') {
    const { readFile } = await import('node:fs/promises')
    const { resolve } = await import('node:path')
    const candidates = [
      resolve(process.cwd(), 'public/wasm/core_wasm.wasm'),
      resolve(process.cwd(), 'apps/web/public/wasm/core_wasm.wasm'),
    ]
    let loaded: Buffer | null = null
    for (const candidate of candidates) {
      try {
        loaded = await readFile(candidate)
        break
      } catch {
        // Try next candidate path
      }
    }
    if (!loaded) {
      throw new Error('core_wasm.wasm not found; run ../../dev/build-wasm.sh first')
    }
    wasmBytes = loaded
  } else {
    const resp = await fetch('/wasm/core_wasm.wasm')
    if (!resp.ok) {
      throw new Error(`Failed to fetch /wasm/core_wasm.wasm: ${resp.status}`)
    }
    wasmBytes = await resp.arrayBuffer()
  }

  const module = await WebAssembly.compile(wasmBytes)
  const imports: WebAssembly.Imports = {}
  for (const imp of WebAssembly.Module.imports(module)) {
    const modBag = (imports[imp.module] ||= {})
    if (imp.kind === 'function') {
      modBag[imp.name] = () => 0
    }
  }
  const instance = await WebAssembly.instantiate(module, imports)
  return instance.exports as unknown as RawWasmExports
}

export function getCoreWasm(): Promise<RawWasmExports> {
  if (!cachedExportsPromise) {
    cachedExportsPromise = instantiateCoreWasm()
  }
  return cachedExportsPromise
}

async function callCoreJson<T>(payload: Record<string, unknown>): Promise<T> {
  const wasm = await getCoreWasm()
  const reqBytes = encoder.encode(JSON.stringify(payload))
  const ptr = wasm.jade_wasm_alloc(reqBytes.length)
  try {
    new Uint8Array(wasm.memory.buffer, ptr, reqBytes.length).set(reqBytes)
    wasm.jade_wasm_call(ptr, reqBytes.length)
  } finally {
    wasm.jade_wasm_free(ptr, reqBytes.length)
  }

  const outPtr = wasm.jade_wasm_response_ptr()
  const outLen = wasm.jade_wasm_response_len()
  const outStr = decoder.decode(new Uint8Array(wasm.memory.buffer, outPtr, outLen))
  const parsed = JSON.parse(outStr) as { ok: boolean; data?: T; error?: string }
  if (!parsed.ok || parsed.data === undefined) {
    throw new Error(parsed.error || 'core_wasm call failed')
  }
  return parsed.data
}

interface RawBoardViewResult {
  snapshot: {
    source_revision: number
    presentation_revision: number
    side_to_move: 'red' | 'black'
    orientation: 'redbelow' | 'blackbelow' | 'red_below' | 'black_below'
    viewport: BoardViewport | null
    pieces: Array<{
      visual_token: string
      piece_id: number
      side: 'red' | 'black'
      role: PieceViewRecord['role']
      file: number
      rank: number
      square: string
      glyph: string
    }>
    selection: number | string | null
    legal_destinations: Array<number | string>
    last_move: { from: number | string; to: number | string } | null
    check_square: number | string | null
    adjudication: AdjudicationVerdict | null
  }
  fen: string
  moves_chinese: string[]
  legal_moves_uci: string[]
}

export async function computeBoardView(params: {
  fen?: string
  movesUci?: string[]
  width: number
  height: number
  padding?: number
  redBelow?: boolean
  selectedUci?: string | null
  traditionalGlyphs?: boolean
  materialId?: string
  darkSurface?: boolean
  reducedMotion?: boolean
}): Promise<BoardViewResult> {
  const raw = await callCoreJson<RawBoardViewResult>({
    op: 'board_view',
    fen: params.fen,
    moves_uci: params.movesUci ?? [],
    width: params.width,
    height: params.height,
    padding: params.padding ?? 18,
    red_below: params.redBelow ?? true,
    selected_uci: params.selectedUci ?? null,
    traditional_glyphs: params.traditionalGlyphs ?? false,
    material_id: params.materialId ?? 'pale-jade',
    dark_surface: params.darkSurface ?? false,
    reduced_motion: params.reducedMotion ?? false,
  })

  const s = raw.snapshot
  const isRedBelow = s.orientation === 'redbelow' || s.orientation === 'red_below'
  const selection = s.selection !== null ? squareIndexToUci(s.selection) : null
  const checkSquare = s.check_square !== null ? squareIndexToUci(s.check_square) : null
  const vp = s.viewport

  const pieces: PieceViewRecord[] = s.pieces.map((p) => {
    let center: ScreenPoint | null = null
    if (vp) {
      center = isRedBelow
        ? {
            x: vp.origin_x + vp.cell_size * p.file,
            y: vp.origin_y + vp.cell_size * (9 - p.rank),
          }
        : {
            x: vp.origin_x + vp.cell_size * (8 - p.file),
            y: vp.origin_y + vp.cell_size * p.rank,
          }
    }
    return {
      ...p,
      center,
      selected: selection === p.square,
    }
  })

  return {
    fen: raw.fen,
    moves_chinese: raw.moves_chinese,
    legal_moves_uci: raw.legal_moves_uci,
    snapshot: {
      source_revision: s.source_revision,
      presentation_revision: s.presentation_revision,
      side_to_move: s.side_to_move,
      orientation: isRedBelow ? 'red_below' : 'black_below',
      viewport: vp,
      pieces,
      selection,
      legal_destinations: s.legal_destinations.map(squareIndexToUci),
      last_move: s.last_move
        ? {
            from: squareIndexToUci(s.last_move.from),
            to: squareIndexToUci(s.last_move.to),
          }
        : null,
      in_check_side: checkSquare ? s.side_to_move : null,
      check_square: checkSquare,
      adjudication: s.adjudication,
    },
  }
}

export function applyCoreMove(params: {
  fen?: string
  movesUci?: string[]
  token: string
  traditionalGlyphs?: boolean
}): Promise<ApplyMoveResult> {
  return callCoreJson<ApplyMoveResult>({
    op: 'apply_move',
    fen: params.fen,
    moves_uci: params.movesUci ?? [],
    token: params.token,
    traditional_glyphs: params.traditionalGlyphs ?? false,
  })
}

export async function hitTestBoard(params: {
  width: number
  height: number
  padding?: number
  redBelow?: boolean
  px: number
  py: number
}): Promise<string | null> {
  const res = await callCoreJson<{ square: string | null }>({
    op: 'hit_test',
    width: params.width,
    height: params.height,
    padding: params.padding ?? 18,
    red_below: params.redBelow ?? true,
    px: params.px,
    py: params.py,
  })
  return res.square
}

export function searchSampleArchive(query = ''): Promise<ArchiveSearchResult> {
  return callCoreJson<ArchiveSearchResult>({ op: 'archive_search', query })
}

export function getSampleArchiveGame(gameId: string): Promise<ArchiveGame> {
  return callCoreJson<ArchiveGame>({ op: 'archive_game', game_id: gameId })
}

export function querySamplePosition(fen?: string): Promise<PositionExplorerResult> {
  return callCoreJson<PositionExplorerResult>({ op: 'archive_position', fen })
}
