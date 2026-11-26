import type {
  ArchiveAccessPolicy,
  ArchiveCollectionSummary,
  ArchiveError,
  ArchiveGameDetail,
  ArchiveGameRecord,
  ArchiveGameSummary,
  ArchiveGameSummaryPage,
  ArchivePlayer,
  ArchivePlayerAlias,
  ArchivePlayerProfile,
  ArchivePlayerSearch,
  ArchivePositionContinuation,
  ArchivePositionResponse,
  ArchiveRelease,
  SourceAttribution,
} from '@play-jade/protocol'

export interface ArchiveProxyResult {
  status: number
  body: unknown
}

/** A record ID is one canonical path segment: no separators, no dot segments, no escapes. */
export const RECORD_ID = /^[A-Za-z0-9][A-Za-z0-9._~-]*$/

type Check = (body: Record<string, unknown>) => boolean
type Field = 'text' | 'count' | 'flag' | 'textOrNull' | 'texts' | ((v: unknown) => boolean)

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

const FIELD: Record<Exclude<Field, (v: unknown) => boolean>, (v: unknown) => boolean> = {
  text: (v) => typeof v === 'string',
  count: (v) => Number.isSafeInteger(v) && (v as number) >= 0,
  flag: (v) => typeof v === 'boolean',
  textOrNull: (v) => v === null || typeof v === 'string',
  texts: (v) => Array.isArray(v) && v.every((x) => typeof x === 'string'),
}

/**
 * A validator for one protocol interface `T`: the field list must name every key of `T` (a missing or
 * renamed key fails type checking), and each field must be present with its type.
 */
function shape<T>(fields: { [K in keyof T]-?: Field }): (v: unknown) => boolean {
  return (v) =>
    isObject(v) &&
    (Object.entries(fields) as [string, Field][]).every(
      ([key, field]) => key in v && (typeof field === 'function' ? field : FIELD[field])(v[key]),
    )
}
const listOf = (item: (v: unknown) => boolean) => (v: unknown) => Array.isArray(v) && v.every(item)
const oneOf = (...values: string[]) => (v: unknown) => typeof v === 'string' && values.includes(v)

// Each validator mirrors its interface in protocol/index.ts field for field.
const isRelease = shape<ArchiveRelease>({ dataset_id: 'text', release_id: 'text', is_synthetic_fixture: 'flag' })
const isSource = shape<SourceAttribution>({ label: 'text', url: 'textOrNull', locator: 'textOrNull' })
const isAccessPolicy = shape<ArchiveAccessPolicy>({
  ordinary_search_and_replay: 'text',
  individual_game_export: 'text',
  collection_bulk_export: 'text',
})
const isAlias = shape<ArchivePlayerAlias>({ text: 'text', script: 'text', source: 'text', verification_state: 'text' })
const isPlayer = shape<ArchivePlayer>({
  player_id: 'text',
  primary_display_name: 'text',
  native_display_name: 'text',
  disambiguation_note: 'text',
  aliases: listOf(isAlias),
})
const isSummary = shape<ArchiveGameSummary>({
  game_id: 'text',
  red_player_id: 'text',
  red_display_name: 'text',
  black_player_id: 'text',
  black_display_name: 'text',
  event_name: 'text',
  round: 'textOrNull',
  date_precision: 'text',
  date_display: 'text',
  recorded_result: 'text',
  replay_validity: 'text',
})
const isGamePage = shape<ArchiveGameSummaryPage>({
  release: isRelease,
  status: oneOf('ok', 'no_games_found'),
  total: 'count',
  offset: 'count',
  limit: 'count',
  games: listOf(isSummary),
})
const isRecord = shape<ArchiveGameRecord>({
  game_id: 'text',
  dataset_id: 'text',
  is_synthetic_fixture: 'flag',
  event_name: 'text',
  date_precision: 'text',
  date_display: 'text',
  participant_kind: 'text',
  game_context: 'text',
  red_player_id: 'text',
  black_player_id: 'text',
  initial_fen: 'text',
  moves_uci: 'texts',
  moves_chinese: 'texts',
  recorded_result: 'text',
  recorded_result_reason: 'text',
  original_rules_provenance: 'text',
  replay_validity: 'text',
})
const POSITION_STATES = ['ok', 'no_games_found', 'not_yet_parsed', 'excluded_from_dataset', 'not_analysed']
const isContinuation = shape<ArchivePositionContinuation>({
  move_uci: 'text',
  move_chinese: 'text',
  deduplicated_game_count: 'count',
  total_occurrence_count: 'count',
  known_result_count: 'count',
  red_wins: 'count',
  draws: 'count',
  black_wins: 'count',
  contributing_game_ids: 'texts',
})

interface ReadRoute {
  collection: string
  withId: boolean
  query: string[]
  /** The complete answer for this route, release included (protocol/index.ts). */
  check: Check
}

/** The archive read routes the browser may reach, with the query fields each forwards. */
const READ_ROUTES: ReadRoute[] = [
  {
    collection: 'collection',
    withId: false,
    query: [],
    check: shape<ArchiveCollectionSummary>({
      release: isRelease,
      title: 'text',
      scope: 'text',
      provenance_banner: 'text',
      source: isSource,
      access_policy: isAccessPolicy,
      game_count: 'count',
    }),
  },
  { collection: 'games', withId: false, query: ['player_id', 'text', 'offset', 'limit'], check: isGamePage },
  {
    collection: 'games',
    withId: true,
    query: [],
    check: shape<ArchiveGameDetail>({ release: isRelease, summary: isSummary, source: isSource, record: isRecord }),
  },
  {
    collection: 'players',
    withId: false,
    query: ['q'],
    check: shape<ArchivePlayerSearch>({ release: isRelease, provenance_banner: 'text', players: listOf(isPlayer) }),
  },
  {
    collection: 'players',
    withId: true,
    query: ['offset', 'limit'],
    check: shape<ArchivePlayerProfile>({ release: isRelease, player: isPlayer, games: isGamePage }),
  },
  {
    collection: 'positions',
    withId: false,
    query: ['fen'],
    check: shape<ArchivePositionResponse>({
      release: isRelease,
      dataset_id: 'text',
      is_synthetic_fixture: 'flag',
      position_key_sha256: 'text',
      status: oneOf(...POSITION_STATES),
      evaluation_status: oneOf(...POSITION_STATES),
      sample_size_games: 'count',
      continuations: listOf(isContinuation),
    }),
  },
]

function error(status: number, body: ArchiveError): ArchiveProxyResult {
  return { status, body }
}

/** The route a path names, or null for a path outside the archive. */
function routeFor(path: string): { route: ReadRoute; id: string | undefined } | null {
  const [collection, id, ...rest] = path.split('/')
  if (rest.length > 0) return null
  const route = READ_ROUTES.find((r) => r.collection === collection && r.withId === (id !== undefined))
  if (!route || (id !== undefined && !RECORD_ID.test(id))) return null
  return { route, id }
}

function isAnswerFor(route: ReadRoute, body: unknown): boolean {
  return isObject(body) && route.check(body)
}

function isJsonMediaType(contentType: string | null): boolean {
  return (contentType ?? '').split(';')[0]!.trim().toLowerCase() === 'application/json'
}

/**
 * The archive path of a same-origin request. `routerPath` is the router's mount-relative capture;
 * `rawTarget` is the request target as sent, before the router decodes escapes, drops a trailing
 * slash, or resolves a dot segment.
 */
export function archivePathOf(routerPath: string, rawTarget: string): string | null {
  const rawPath = rawTarget.split('?')[0] ?? ''
  const prefix = '/api/archive/'
  const at = rawPath.indexOf(prefix)
  // The raw suffix must be exactly the router's capture: any decoding, normalization, or dropped
  // slash between them means the request did not name a canonical archive route.
  return at >= 0 && rawPath.slice(at + prefix.length) === routerPath ? routerPath : null
}

/**
 * Reads one archive route from the configured hosted archive. Only archive JSON of the requested shape
 * reaches the caller; a missing record, a rejected request, and an archive that is not answering each
 * keep their own code. Redirects are never followed, so the proxy contacts only the configured archive.
 */
export async function readArchive(
  configuredBaseUrl: () => string,
  path: string,
  query: Record<string, unknown>,
  timeoutMs = 10_000,
): Promise<ArchiveProxyResult> {
  const target = routeFor(path)
  if (!target) return error(404, { code: 'archive_not_found', message: 'Unknown archive path' })
  const { route, id } = target

  // Configuration is read only for a real archive route.
  const baseUrl = configuredBaseUrl()
  if (!baseUrl) {
    return error(503, {
      code: 'archive_provider_unavailable',
      message: 'No hosted archive is configured for this deployment',
    })
  }
  let url: URL
  try {
    url = new URL(baseUrl)
  } catch {
    return error(503, { code: 'archive_provider_unavailable', message: 'The configured archive address is invalid' })
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return error(503, { code: 'archive_provider_unavailable', message: 'The configured archive address is not HTTP' })
  }
  const segments = ['api', 'v1', 'archive', route.collection, ...(id === undefined ? [] : [id])]
  url.pathname = `${url.pathname.replace(/\/+$/, '')}/${segments.map(encodeURIComponent).join('/')}`
  url.search = ''
  for (const key of route.query) {
    const value = query[key]
    if (value === undefined) continue
    if (typeof value !== 'string' && typeof value !== 'number') {
      return error(400, { code: 'archive_invalid_request', message: `Give ${key} at most once` })
    }
    url.searchParams.set(key, String(value))
  }

  let response: Response
  try {
    response = await fetch(url, {
      headers: { accept: 'application/json' },
      redirect: 'manual',
      signal: AbortSignal.timeout(timeoutMs),
    })
  } catch {
    return error(503, { code: 'archive_provider_unavailable', message: 'The archive did not answer' })
  }

  const reject = async (status: number, body: ArchiveError) => {
    await response.body?.cancel().catch(() => undefined)
    return error(status, body)
  }
  if (response.status === 404) return reject(404, { code: 'archive_not_found', message: 'Archive record not found' })
  if (response.status === 400) {
    return reject(400, { code: 'archive_invalid_request', message: 'The archive could not answer this request' })
  }
  if (response.status >= 300 && response.status < 400) {
    return reject(502, { code: 'archive_provider_unavailable', message: 'The archive answered with a redirect' })
  }
  if (!response.ok) {
    return reject(503, {
      code: 'archive_provider_unavailable',
      message: `The archive answered with status ${response.status}`,
    })
  }
  if (!isJsonMediaType(response.headers.get('content-type'))) {
    return reject(502, {
      code: 'archive_provider_unavailable',
      message: 'The archive answered with something other than JSON',
    })
  }

  const body: unknown = await response.json().catch(() => null)
  if (!isAnswerFor(route, body)) {
    return error(502, {
      code: 'archive_provider_unavailable',
      message: 'The archive answered with something other than the requested archive record',
    })
  }
  return { status: 200, body }
}
