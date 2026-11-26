import type {
  ArchiveCollectionSummary,
  ArchiveErrorCode,
  ArchiveGameDetail,
  ArchiveGameQuery,
  ArchiveGameSummaryPage,
  ArchivePlayerProfile,
  ArchivePlayerSearch,
  ArchivePositionResponse,
} from '@play-jade/protocol'
import { callCoreJson, CoreCallError } from '~/utils/core-wasm'

export type ArchivePage = Pick<ArchiveGameQuery, 'offset' | 'limit'>

/** The archive read interface the pages use, answered by the embedded sample or the hosted archive. */
export interface ArchiveProvider {
  collection(): Promise<ArchiveCollectionSummary>
  findPlayers(query: string): Promise<ArchivePlayerSearch>
  playerProfile(playerId: string, page?: ArchivePage): Promise<ArchivePlayerProfile>
  browseGames(query: ArchiveGameQuery): Promise<ArchiveGameSummaryPage>
  gameDetail(gameId: string): Promise<ArchiveGameDetail>
  explorePosition(fen: string): Promise<ArchivePositionResponse>
}

export type ArchiveSource = 'sample' | 'hosted'

export class ArchiveRequestError extends Error {
  constructor(
    readonly code: ArchiveErrorCode,
    message: string,
  ) {
    super(message)
    this.name = 'ArchiveRequestError'
  }
}

/** The largest page position every binding can carry (the core's WASM build counts in 32 bits). */
const MAX_PAGE_NUMBER = 2 ** 32 - 1

/** Rejects a page position the contract cannot represent (it counts records from zero). */
function pageNumber(name: string, value: number | undefined): number | undefined {
  if (value === undefined) return undefined
  if (!Number.isSafeInteger(value) || value < 0 || value > MAX_PAGE_NUMBER) {
    throw new ArchiveRequestError('archive_invalid_request', `${name} must be a whole number from 0 to ${MAX_PAGE_NUMBER}`)
  }
  return value
}

/** Copies only the supported fields, so a stray field never overrides an operation or a record ID. */
function pageFields(page: ArchivePage): ArchivePage {
  return { offset: pageNumber('offset', page.offset), limit: pageNumber('limit', page.limit) }
}

function gameQueryFields(query: ArchiveGameQuery): ArchiveGameQuery {
  return { player_id: query.player_id, text: query.text, ...pageFields(query) }
}

/** A record ID is one canonical path segment, matching the same-origin route's rule. */
function recordId(id: string): string {
  if (!/^[A-Za-z0-9][A-Za-z0-9._~-]*$/.test(id)) {
    throw new ArchiveRequestError('archive_invalid_request', `Not an archive record ID: ${JSON.stringify(id)}`)
  }
  return id
}

function requireFen(fen: string): string {
  if (!fen.trim()) throw new ArchiveRequestError('archive_invalid_request', 'A position FEN is required')
  return fen
}

const CORE_ERROR_CODES: Record<string, ArchiveErrorCode> = {
  not_found: 'archive_not_found',
  invalid_request: 'archive_invalid_request',
  unavailable: 'archive_provider_unavailable',
}

async function callSample<T>(payload: Record<string, unknown>): Promise<T> {
  try {
    return await callCoreJson<T>(payload)
  } catch (error) {
    const code = error instanceof CoreCallError && error.code ? CORE_ERROR_CODES[error.code] : undefined
    if (code) throw new ArchiveRequestError(code, error instanceof Error ? error.message : String(error))
    throw error
  }
}

/** Runs the embedded sample in the core WASM module; it needs no network or credentials. */
export const sampleArchiveProvider: ArchiveProvider = {
  collection: () => callSample({ op: 'archive_collection' }),
  findPlayers: (query) => callSample({ op: 'archive_search', query }),
  playerProfile: async (playerId, page = {}) =>
    callSample({ ...pageFields(page), op: 'archive_player', player_id: recordId(playerId) }),
  browseGames: async (query) => callSample({ ...gameQueryFields(query), op: 'archive_games' }),
  gameDetail: async (gameId) => callSample({ op: 'archive_game_detail', game_id: recordId(gameId) }),
  explorePosition: async (fen) => callSample({ op: 'archive_position', fen: requireFen(fen) }),
}

async function getHosted<T>(path: string, query: Record<string, string | number | undefined> = {}): Promise<T> {
  try {
    return await $fetch<T>(path, { query })
  } catch (error) {
    const data = (error as { data?: { code?: string; message?: string } }).data
    if (data?.code === 'archive_not_found' || data?.code === 'archive_invalid_request') {
      throw new ArchiveRequestError(data.code, data.message ?? data.code)
    }
    throw new ArchiveRequestError('archive_provider_unavailable', data?.message ?? 'The archive did not answer')
  }
}

/** Reads the configured hosted archive through the same-origin `/api/archive` routes. */
export const hostedArchiveProvider: ArchiveProvider = {
  collection: () => getHosted('/api/archive/collection'),
  findPlayers: (query) => getHosted('/api/archive/players', { q: query }),
  playerProfile: async (playerId, page = {}) =>
    getHosted(`/api/archive/players/${recordId(playerId)}`, { ...pageFields(page) }),
  browseGames: async (query) => getHosted('/api/archive/games', { ...gameQueryFields(query) }),
  gameDetail: async (gameId) => getHosted(`/api/archive/games/${recordId(gameId)}`),
  explorePosition: async (fen) => getHosted('/api/archive/positions', { fen: requireFen(fen) }),
}

/** Selects the provider for a configured source; an unrecognized setting is an error, never the sample. */
export function archiveProviderFor(source: unknown): ArchiveProvider {
  if (source === 'hosted') return hostedArchiveProvider
  if (source === 'sample') return sampleArchiveProvider
  throw new Error(`Unknown archive source setting: ${String(source)} (expected "sample" or "hosted")`)
}

/** The provider this deployment is configured for. */
export function useArchiveProvider(): ArchiveProvider {
  return archiveProviderFor(useRuntimeConfig().public.archiveSource)
}
