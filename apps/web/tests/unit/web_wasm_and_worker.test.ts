import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { BrowserWorkerSearchAdapter } from '@jade/wasm-worker'
import {
  applyCoreMove,
  computeBoardView,
  hitTestBoard,
  querySamplePosition,
  searchSampleArchive,
} from '../../app/utils/core-wasm'
import { archiveProviderFor, hostedArchiveProvider, sampleArchiveProvider } from '../../app/utils/archive-provider'
import { archivePathOf, readArchive } from '../../server/utils/archive-proxy'
import { resolveTraditionalScript, SUPPORTED_LOCALES, tTerm } from '../../app/utils/i18n'
import pikafishFixtures from '../../../../data/fixtures/rules/pikafish_examples.json'

describe('core_wasm isomorphic browser/SSR rules & geometry contract', () => {
  it('computes starting BoardViewSnapshot with 32 pieces and exact Section 4 hit-testing', async () => {
    const res = await computeBoardView({
      width: 450,
      height: 500,
      padding: 18,
      redBelow: true,
      selectedUci: 'h2',
    })
    expect(res.snapshot.pieces).toHaveLength(32)
    expect(res.snapshot.side_to_move).toBe('red')
    expect(res.snapshot.selection).toBe('h2')
    expect(res.snapshot.legal_destinations).toContain('e2')
    expect(res.legal_moves_uci).toContain('h2e2')

    const h2 = res.snapshot.pieces.find((p) => p.square === 'h2')!
    expect(h2.glyph).toBe('炮')
    expect(h2.center).not.toBeNull()

    const hit = await hitTestBoard({
      width: 450,
      height: 500,
      padding: 18,
      redBelow: true,
      px: h2.center!.x,
      py: h2.center!.y,
    })
    expect(hit).toBe('h2')

    const outside = await hitTestBoard({
      width: 450,
      height: 500,
      padding: 18,
      redBelow: true,
      px: 1,
      py: 1,
    })
    expect(outside).toBeNull()
  })

  it('applies Central Cannon opening moves and switches Simplified vs Traditional piece glyphs cleanly', async () => {
    const m1 = await applyCoreMove({ token: 'h2e2', traditionalGlyphs: false })
    expect(m1.move_uci).toBe('h2e2')
    expect(m1.move_chinese).toBe('炮二平五')

    const tradView = await computeBoardView({
      movesUci: ['h2e2', 'h9g7'],
      width: 450,
      height: 500,
      traditionalGlyphs: true,
    })
    const redGen = tradView.snapshot.pieces.find((p) => p.side === 'red' && p.role === 'general')!
    const blkHorse = tradView.snapshot.pieces.find((p) => p.side === 'black' && p.role === 'horse' && p.square === 'g7')!
    expect(redGen.glyph).toBe('帥')
    expect(blkHorse.glyph).toBe('馬')
  })

  it('verifies all 39 Pikafish repetition/chase examples through compiled core_wasm.wasm', async () => {
    const examples = (pikafishFixtures as {
      examples: Array<{
        id: string
        fen: string
        moves_uci: string[]
        expected_outcome: string
      }>
    }).examples
    expect(examples).toHaveLength(39)

    for (const ex of examples) {
      let moves: string[] = []
      let lastAdj = null
      for (const tok of ex.moves_uci) {
        const step = await applyCoreMove({
          fen: ex.fen,
          movesUci: moves,
          token: tok,
        })
        moves = step.moves_uci
        lastAdj = step.adjudication
      }
      expect(lastAdj, `Pikafish example ${ex.id}`).not.toBeNull()
      expect(lastAdj!.outcome, `Pikafish example ${ex.id} outcome`).toBe(ex.expected_outcome)
    }
  })

  it('queries disambiguated sample archive players and deduplicated position continuations', async () => {
    const linSearch = await searchSampleArchive('林明')
    expect(linSearch.is_synthetic_fixture).toBe(true)
    expect(linSearch.players).toHaveLength(2)
    expect(linSearch.players[0]!.player_id).not.toBe(linSearch.players[1]!.player_id)

    const exp = await querySamplePosition()
    expect(exp.sample_size_games).toBeGreaterThanOrEqual(1)
    expect(exp.continuations[0]!.move_uci).toBe('h2e2')
    expect(exp.continuations[0]!.deduplicated_game_count).toBeGreaterThanOrEqual(1)
  })
})

describe('archive provider contract', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('browses the sample collection, follows a player to their games, and opens a game with its source', async () => {
    const collection = await sampleArchiveProvider.collection()
    expect(collection.release.is_synthetic_fixture).toBe(true)
    expect(collection.release.release_id).toMatch(/^jade-public-sample-fixtures-v1@sha256:/)
    expect(collection.game_count).toBe(2)

    const page = await sampleArchiveProvider.browseGames({ offset: 1, limit: 1 })
    expect(page.status).toBe('ok')
    expect(page.total).toBe(2)
    expect(page.games[0]!.game_id).toBe('sample-game-screen-horse-002')
    expect(page.release).toEqual(collection.release)

    const profile = await sampleArchiveProvider.playerProfile('sample-player-003')
    expect(profile.games.games.map((g) => g.game_id)).toEqual(['sample-game-screen-horse-002'])

    const detail = await sampleArchiveProvider.gameDetail('sample-game-screen-horse-002')
    expect(detail.record.moves_uci).toHaveLength(8)
    expect(detail.source.locator).toBe('sample-game-screen-horse-002')

    const position = await sampleArchiveProvider.explorePosition(detail.record.initial_fen)
    expect(position.release).toEqual(collection.release)
    expect(position.continuations[0]!.contributing_game_ids).toContain('sample-game-screen-horse-002')
  })

  it('reports an empty sample search and a missing sample game distinctly', async () => {
    const empty = await sampleArchiveProvider.browseGames({ text: 'no such player' })
    expect(empty.status).toBe('no_games_found')
    expect(empty.games).toEqual([])
    await expect(sampleArchiveProvider.gameDetail('no-such-game')).rejects.toMatchObject({
      code: 'archive_not_found',
    })
    await expect(sampleArchiveProvider.explorePosition('  ')).rejects.toMatchObject({
      code: 'archive_invalid_request',
    })
  })

  it('keeps an explicit player ID over a stray field in the page argument', async () => {
    const page = { offset: 0, limit: 5, player_id: 'sample-player-001' } as Parameters<
      typeof sampleArchiveProvider.playerProfile
    >[1]
    const profile = await sampleArchiveProvider.playerProfile('sample-player-003', page)
    expect(profile.player.player_id).toBe('sample-player-003')
  })

  it('selects a provider only for a recognized archive source setting', () => {
    expect(archiveProviderFor('hosted')).toBe(hostedArchiveProvider)
    expect(archiveProviderFor('sample')).toBe(sampleArchiveProvider)
    expect(() => archiveProviderFor('Hosted')).toThrow(/Unknown archive source/)
  })

  it('maps hosted proxy errors to their archive codes', async () => {
    vi.stubGlobal('$fetch', () =>
      Promise.reject({ data: { code: 'archive_invalid_request', message: 'The archive could not answer' } }),
    )
    await expect(hostedArchiveProvider.explorePosition('x')).rejects.toMatchObject({
      code: 'archive_invalid_request',
    })
    vi.stubGlobal('$fetch', () => Promise.reject(new TypeError('fetch failed')))
    await expect(hostedArchiveProvider.collection()).rejects.toMatchObject({
      code: 'archive_provider_unavailable',
    })
  })
})

describe('hosted archive proxy', () => {
  const release = { dataset_id: 'd', release_id: 'd@r1', is_synthetic_fixture: false }
  const summary = {
    game_id: 'g1',
    red_player_id: 'p1',
    red_display_name: 'Red',
    black_player_id: 'p2',
    black_display_name: 'Black',
    event_name: 'Event',
    round: null,
    date_precision: 'day',
    date_display: '2022-11-01',
    recorded_result: 'draw',
    replay_validity: 'valid_prefix_complete',
  }
  const player = {
    player_id: 'p1',
    primary_display_name: 'Red',
    native_display_name: '紅',
    disambiguation_note: '',
    aliases: [{ text: '紅', script: 'zh-Hant', source: 's', verification_state: 'reviewed' }],
  }
  const page = { release, status: 'ok', total: 1, offset: 0, limit: 25, games: [summary] }
  const record = {
    game_id: 'g1',
    dataset_id: 'd',
    is_synthetic_fixture: false,
    event_name: 'Event',
    date_precision: 'day',
    date_display: '2022-11-01',
    participant_kind: 'human',
    game_context: 'tournament',
    red_player_id: 'p1',
    black_player_id: 'p2',
    initial_fen: 'start',
    moves_uci: ['h2e2'],
    moves_chinese: ['炮二平五'],
    recorded_result: 'draw',
    recorded_result_reason: 'agreed',
    original_rules_provenance: 'as recorded',
    replay_validity: 'valid_prefix_complete',
  }
  const detail = { release, summary, source: { label: 's', url: null, locator: 'p2' }, record }
  const collection = {
    release,
    title: 'Collection',
    scope: 'Scope',
    provenance_banner: 'Banner',
    source: { label: 's', url: 'https://example.invalid/book.pdf', locator: 'p2' },
    access_policy: { ordinary_search_and_replay: 'free', individual_game_export: 'allowed', collection_bulk_export: 'none' },
    game_count: 1,
  }
  // The start position and its key as the sample provider answers them (crates/archive-client).
  const START_FEN = 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1'
  let startKey = ''
  const position = {
    release,
    dataset_id: 'd',
    is_synthetic_fixture: false,
    get position_key_sha256() {
      return startKey
    },
    status: 'ok',
    evaluation_status: 'not_analysed',
    sample_size_games: 1,
    continuations: [
      {
        move_uci: 'h2e2',
        move_chinese: '炮二平五',
        deduplicated_game_count: 1,
        total_occurrence_count: 1,
        known_result_count: 1,
        red_wins: 0,
        draws: 1,
        black_wins: 0,
        contributing_game_ids: ['g1'],
      },
    ],
  }
  const seen: string[] = []
  let upstreamClosed: (() => void) | null = null
  const cancelledUpstream = new Promise<void>((resolve) => {
    upstreamClosed = resolve
  })
  let upstream: Server
  let baseUrl = ''

  beforeAll(async () => {
    startKey = (await sampleArchiveProvider.explorePosition(START_FEN)).position_key_sha256
    upstream = createServer((req, res) => {
      seen.push(req.url ?? '')
      const path = new URL(req.url ?? '/', 'http://upstream').pathname
      const send = (status: number, body: unknown, type = 'application/json; charset=utf-8') => {
        res.writeHead(status, { 'content-type': type })
        res.end(JSON.stringify(body))
      }
      switch (path) {
        case '/base/api/v1/archive/games':
          // A page without `limit` is incomplete; every other directory request gets a full page.
          return req.url?.includes('text=partial') ? send(200, { ...page, limit: undefined }) : send(200, page)
        case '/base/api/v1/archive/games/g1':
          return send(200, detail)
        case '/base/api/v1/archive/games/no-round':
          return send(200, { ...detail, summary: { ...summary, round: undefined } })
        case '/base/api/v1/archive/games/missing-provenance':
          return send(200, { ...detail, record: { ...record, original_rules_provenance: undefined } })
        case '/base/api/v1/archive/games/bad-source':
          return send(200, { ...detail, source: { label: 's', url: 42, locator: null } })
        case '/base/api/v1/archive/games/empty-objects':
          return send(200, { release, summary: {}, source: {}, record: {} })
        case '/base/api/v1/archive/collection':
          return send(200, collection)
        case '/base/api/v1/archive/players/p1':
          return send(200, { release, player, games: page })
        case '/base/api/v1/archive/players/p-bad-page':
          return send(200, { release, player, games: { ...page, release: undefined } })
        case '/base/api/v1/archive/games/missing':
          return send(404, { error: 'not found' })
        case '/base/api/v1/archive/games/wrong-shape':
          return send(200, { release, title: 'a collection, not a game', scope: 's', game_count: 1, source: { label: 's' } })
        case '/base/api/v1/archive/games/disguised':
          return send(200, detail, 'text/plain; note="application/json"')
        case '/base/api/v1/archive/games/moved':
          res.writeHead(302, { location: 'https://example.invalid/login' })
          return res.end()
        case '/base/api/v1/archive/games/dropped':
          return req.socket.destroy()
        case '/base/api/v1/archive/games/endless-error':
          // An error page that never finishes: the proxy must release it rather than wait for it.
          res.on('close', () => upstreamClosed?.())
          res.writeHead(500, { 'content-type': 'text/html' })
          return res.write('<html>'.repeat(1000))
        case '/base/api/v1/archive/positions':
          if (new URL(req.url ?? '/', 'http://upstream').searchParams.get('fen') === START_FEN) {
            return send(200, position)
          }
          return req.url?.includes('fen=null-row')
            ? send(200, { ...position, continuations: [...position.continuations, null] })
            : send(400, { error: 'invalid_fen' })
        case '/base/api/v1/archive/players':
          // A search without its provenance banner is incomplete, even with a release.
          return req.url?.includes('q=good')
            ? send(200, { release, provenance_banner: 'Banner', players: [player] })
            : send(200, { release, players: [player] })
        default:
          return send(200, '<html>sign in</html>', 'text/html')
      }
    })
    await new Promise<void>((resolve) => upstream.listen(0, '127.0.0.1', resolve))
    baseUrl = `http://127.0.0.1:${(upstream.address() as AddressInfo).port}/base`
  })

  afterAll(async () => {
    upstream.closeAllConnections()
    await new Promise((resolve) => upstream.close(resolve))
  })

  const read = (path: string, query: Record<string, unknown> = {}) => readArchive(() => baseUrl, path, query)

  it('forwards only the route and its query fields, and returns a complete archive answer', async () => {
    const result = await read('games', { player_id: 'p1', limit: '5', secret: 'x' })
    expect(result.status).toBe(200)
    expect(result.body).toMatchObject({ release, total: 1 })
    expect(seen.at(-1)).toBe('/base/api/v1/archive/games?player_id=p1&limit=5')
    expect(await read('games/g1')).toMatchObject({ status: 200, body: detail })
    expect(await read('collection')).toMatchObject({ status: 200, body: collection })
    expect(await read('players', { q: 'good' })).toEqual({
      status: 200,
      body: { release, provenance_banner: 'Banner', players: [player] },
    })
    expect(await read('players/p1')).toEqual({ status: 200, body: { release, player, games: page } })
    expect(await read('positions', { fen: START_FEN })).toEqual({
      status: 200,
      body: { ...position, position_key_sha256: startKey },
    })
  })

  it('keeps a missing record, a rejected request, and an answer of the wrong kind distinct', async () => {
    // Answers that are not the requested archive record: wrong shape, missing fields, wrong media type,
    // a redirect, or a dropped connection. Each is reported unavailable rather than passed through.
    const malformed: [string, Record<string, unknown>][] = [
      ['players', { q: 'a' }],
      ['players/p-bad-page', {}],
      ['games/wrong-shape', {}],
      ['games/sign-in', {}],
      ['games', { text: 'partial' }],
      ['games/missing-provenance', {}],
      ['games/bad-source', {}],
      ['games/no-round', {}],
      ['positions', { fen: 'null-row' }],
      ['games/empty-objects', {}],
      ['games/disguised', {}],
      ['games/moved', {}],
    ]
    const cases: [string, Record<string, unknown>, number, string][] = [
      ['games/missing', {}, 404, 'archive_not_found'],
      ['positions', { fen: 'bad' }, 400, 'archive_invalid_request'],
      ['games', { player_id: ['a', 'b'] }, 400, 'archive_invalid_request'],
      ['games/dropped', {}, 503, 'archive_provider_unavailable'],
      ...malformed.map(([path, query]): [string, Record<string, unknown>, number, string] => [
        path,
        query,
        502,
        'archive_provider_unavailable',
      ]),
    ]
    for (const [path, query, status, code] of cases) {
      expect(await read(path, query), path).toMatchObject({ status, body: { code } })
    }
  })

  it('releases an unread upstream body instead of waiting for it', async () => {
    expect(await read('games/endless-error')).toMatchObject({ status: 503 })
    await expect(
      Promise.race([cancelledUpstream, new Promise((_, reject) => setTimeout(() => reject(new Error('still open')), 2000))]),
    ).resolves.toBeUndefined()
  })

  it('refuses paths outside the archive routes without contacting the upstream or reading configuration', async () => {
    const before = seen.length
    const config = vi.fn(() => baseUrl)
    for (const path of ['corrections', 'games/..', 'games/a\\b', 'games/%2e%2e', 'games/a/b', 'games/']) {
      expect(await readArchive(config, path, {}), path).toMatchObject({
        status: 404,
        body: { code: 'archive_not_found' },
      })
    }
    expect(config).not.toHaveBeenCalled()
    expect(seen.length).toBe(before)
    // The route refuses a raw target that normalization would change, and keeps the router's
    // mount-relative path otherwise (so a non-root app base still routes).
    expect(archivePathOf('games', '/api/archive/games/')).toBeNull()
    expect(archivePathOf('players', '/api/archive/games/../players')).toBeNull()
    expect(archivePathOf('players', '/api/archive/games/%2E%2E/players')).toBeNull()
    expect(archivePathOf('games/g1', '/api/archive/games/%67%31')).toBeNull()
    expect(archivePathOf('games', '/jade/api/archive/games?text=x')).toBe('games')
    expect(archivePathOf('games/g1', '/api/archive/games/g1')).toBe('games/g1')
  })

  it('reports an unconfigured, unparseable, or non-HTTP archive address as unavailable', async () => {
    for (const configured of ['', 'http://[', 'data:application/json,{}']) {
      expect(await readArchive(() => configured, 'collection', {}), configured).toMatchObject({
        status: 503,
        body: { code: 'archive_provider_unavailable' },
      })
    }
  })

  it('checks record IDs and page positions before building a request', async () => {
    await expect(hostedArchiveProvider.gameDetail('')).rejects.toMatchObject({ code: 'archive_invalid_request' })
    for (const offset of [-1, Number.NaN, 2 ** 32]) {
      await expect(sampleArchiveProvider.browseGames({ offset }), String(offset)).rejects.toMatchObject({
        code: 'archive_invalid_request',
      })
    }
    expect((await sampleArchiveProvider.browseGames({ offset: 2 ** 32 - 1 })).games).toEqual([])
    expect(() => archiveProviderFor('')).toThrow(/Unknown archive source/)
  })
})

describe('5-locale glossary & browser WASM search worker contract', () => {
  it('resolves all 5 locales and UK vs US + Simplified vs Traditional defaults', () => {
    expect(SUPPORTED_LOCALES).toHaveLength(5)
    expect(tTerm('settings.colour_scheme', 'en-US')).toBe('Color appearance')
    expect(tTerm('settings.colour_scheme', 'en-GB')).toBe('Colour appearance')
    expect(tTerm('outcome.no_legal_moves_loss', 'zh-Hans-CN')).toBe('困毙判负')
    expect(tTerm('outcome.no_legal_moves_loss', 'zh-Hant-TW')).toBe('困斃判負')
    expect(resolveTraditionalScript('zh-Hans-CN', 'auto')).toBe(false)
    expect(resolveTraditionalScript('zh-Hant-HK', 'auto')).toBe(true)
    expect(resolveTraditionalScript('en-US', 'traditional')).toBe(true)
  })

  it('starts, searches, and cleanly disposes @jade/wasm-worker returning legal Xiangqi PV', async () => {
    const adapter = new BrowserWorkerSearchAdapter()
    try {
      const ready = await adapter.init()
      expect(ready.manifest.rules_profile).toBe('program-2023.11-v1')
      await adapter.configure({ hash_mb: 8, threads: 1, multi_pv: 1 })

      const res = await adapter.search({
        search_id: 'unit-s1',
        state_id: 'unit-startpos',
        fen: 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1',
        moves: ['h2e2'],
        limits: { depth: 3, movetime_ms: 400 },
      })
      expect(res.best_move).toBeTruthy()
      const checkLegal = await applyCoreMove({
        movesUci: ['h2e2'],
        token: res.best_move!,
      })
      expect(checkLegal.moves_uci).toHaveLength(2)
    } finally {
      await adapter.dispose()
    }
  })
})
