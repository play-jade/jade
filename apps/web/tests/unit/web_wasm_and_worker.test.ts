import { describe, expect, it, vi } from 'vitest'
import { BrowserWorkerSearchAdapter } from '@jade/wasm-worker'
import {
  applyCoreMove,
  computeBoardView,
  hitTestBoard,
  querySamplePosition,
  searchSampleArchive,
} from '../../app/utils/core-wasm'
import { resolveTraditionalScript, SUPPORTED_LOCALES, tTerm } from '../../app/utils/i18n'
import { loadOfflineGame } from '../../app/utils/offline-store'
import {
  parsePlyQuery,
  recordedMovesDigest,
  useReplaySession,
  type ReplaySessionOptions,
} from '../../app/composables/useReplaySession'
import { ref, type Ref } from 'vue'
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

describe('replay session', () => {
  const START = 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1'
  const recorded = ['h2e2', 'h9g7', 'h0g2']

  function session(extra: Partial<ReplaySessionOptions> = {}) {
    return useReplaySession({
      source: ref({ kind: 'archive', id: 'game-1', revision: 'release-1' }),
      rootFen: ref(START),
      rulesProfile: ref('program-2023.11-v1'),
      recordedMovesUci: ref([...recorded]),
      ...extra,
    })
  }

  it('starts a variation from an earlier ply, keeps the record, and resumes it after Return to game', async () => {
    const s = session()
    s.selectPly(1)
    const before = s.stateId.value
    const result = await s.tryMove('b9c7')
    expect(result.kind).toBe('variation')
    expect(s.activePath.value).toEqual(['h2e2', 'b9c7'])
    expect(s.stateId.value).not.toBe(before)

    s.returnToRecordedLine()
    expect(s.activeMode.value).toBe('recorded')
    expect(s.cursor.value).toBe(1)
    expect(s.activePath.value).toEqual(['h2e2'])
    expect(s.willReplaceDraft.value).toBe(true)

    s.resumeDraft()
    expect(s.activePath.value).toEqual(['h2e2', 'b9c7'])
  })

  it('follows the recorded move instead of starting a variation', async () => {
    const s = session()
    s.selectPly(1)
    expect((await s.tryMove('h9g7')).kind).toBe('followed')
    expect(s.cursor.value).toBe(2)
    expect(s.variation.value).toBeNull()
  })

  it('rejects an illegal move with feedback and leaves the path unchanged', async () => {
    const s = session()
    s.selectPly(1)
    const result = await s.tryMove('a0a5')
    expect(result.kind).toBe('rejected')
    expect(s.feedback.value).toBeTruthy()
    expect(s.activePath.value).toEqual(['h2e2'])
  })

  it('never reports a saved draft without browser storage, and still exports it', async () => {
    const s = session({ persist: true })
    s.selectPly(0)
    await s.tryMove('b0c2')
    expect(s.saveState.value.state).toBe('unsaved')
    const blob = await s.exportDraft()
    const draft = JSON.parse(await blob!.text())
    expect(draft).toMatchObject({
      schemaVersion: 1,
      sourceKind: 'archive',
      sourceId: 'game-1',
      sourceRevision: 'release-1',
      rulesProfile: 'program-2023.11-v1',
      variationOriginPly: 0,
      variationMovesUci: ['b0c2'],
      activeMode: 'variation',
    })
    expect(draft.recordedMovesDigest).toBe(await recordedMovesDigest(START, recorded))
  })

  it('builds a draft IndexedDB can store, free of reactive proxies', async () => {
    const s = session({ persist: false })
    s.selectPly(1)
    await s.tryMove('b9c7')
    await s.tryMove('b0c2')
    const draft = await s.currentDraft()
    expect(() => structuredClone(draft)).not.toThrow()
    expect(draft!.variationMovesUci).toEqual(['b9c7', 'b0c2'])
  })

  it('rejects the second of two overlapping moves, because the position changed under it', async () => {
    const s = session()
    s.selectPly(1)
    const [first, second] = await Promise.all([s.tryMove('b9c7'), s.tryMove('h9g7')])
    expect(first.kind).toBe('variation')
    expect(second.kind).toBe('rejected')
    expect(s.activePath.value).toEqual(['h2e2', 'b9c7'])
  })

  it("reports the core's move text in the reader's script", async () => {
    const simplified = await session().tryMove('h2e2')
    const traditional = await session({ traditionalGlyphs: ref(true) }).tryMove('b0c2')
    const simplifiedHorse = await session().tryMove('b0c2')
    expect(simplified.kind).toBe('followed')
    expect(traditional.kind === 'variation' && simplifiedHorse.kind === 'variation').toBe(true)
    if (traditional.kind === 'variation' && simplifiedHorse.kind === 'variation') {
      expect(traditional.applied.move_chinese).not.toBe(simplifiedHorse.applied.move_chinese)
    }
  })

  it('keeps export working where Web Crypto is unavailable, without claiming a save', async () => {
    vi.stubGlobal('crypto', {})
    try {
      const s = session({ persist: true })
      await s.tryMove('b0c2')
      expect(s.saveState.value).toMatchObject({ state: 'unsaved' })
      const draft = JSON.parse(await (await s.exportDraft())!.text())
      expect(draft.recordedMovesDigest).toBeNull()
      expect(draft.variationMovesUci).toEqual(['b0c2'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  /** A small in-memory IndexedDB: open, and transactions that complete once their requests finish. */
  function memoryIndexedDb(options: { holdOpens?: boolean; blockOpens?: boolean } = {}) {
    const stores = new Map<string, Map<string, unknown>>()
    const puts: Record<string, unknown>[] = []
    const held: (() => void)[] = []
    const openWaiters: { count: number; resolve: () => void }[] = []
    let opens = 0
    let closes = 0
    const db = {
      objectStoreNames: { contains: (name: string) => stores.has(name) },
      createObjectStore: (name: string) => stores.set(name, new Map()),
      transaction: (name: string) => {
        const store = stores.get(name)!
        const tx: Record<string, unknown> & { oncomplete?: () => void } = {}
        let pending = 0
        const settle = () => setTimeout(() => pending === 0 && tx.oncomplete?.(), 0)
        const request = <T>(run: () => T) => {
          const req: { result?: T; onsuccess?: () => void } = {}
          pending++
          setTimeout(() => {
            req.result = run()
            req.onsuccess?.()
            pending--
            settle()
          }, 0)
          return req
        }
        tx.objectStore = () => ({
          put: (value: { draftId?: string; id?: string }) =>
            request(() => {
              const copy = structuredClone(value)
              puts.push(structuredClone(copy) as Record<string, unknown>)
              store.set((copy.draftId ?? copy.id)!, copy)
            }),
          // Like IndexedDB, a read returns a copy, so a caller cannot change the stored row in place.
          get: (key: string) => request(() => structuredClone(store.get(key))),
        })
        return tx
      },
    }
    return {
      stores,
      puts,
      get opens() {
        return opens
      },
      get closes() {
        return closes
      },
      /** Resolves once `count` opens have been requested. */
      opened: (count: number) =>
        new Promise<void>((resolve) => (opens >= count ? resolve() : openWaiters.push({ count, resolve }))),
      /** Lets every held open request succeed, as when the blocking tab closes. */
      releaseOpens: () => held.splice(0).forEach((go) => go()),
      open: () => {
        opens++
        for (const waiter of openWaiters.filter((w) => opens >= w.count)) {
          openWaiters.splice(openWaiters.indexOf(waiter), 1)
          waiter.resolve()
        }
        const req: {
          result: typeof db & { close?: () => void }
          onupgradeneeded?: () => void
          onsuccess?: () => void
          onblocked?: () => void
        } = {
          result: {
            ...db,
            close: () => {
              closes++
            },
          },
        }
        // The blocked event needs no timer, so a test can hold the deadline clock while it arrives.
        if (options.blockOpens) queueMicrotask(() => req.onblocked?.())
        const go = () =>
          setTimeout(() => {
            if (!stores.has('replay_drafts')) req.onupgradeneeded?.()
            req.onsuccess?.()
          }, 0)
        if (options.holdOpens) held.push(go)
        else go()
        return req
      },
    }
  }

  /** A digest that waits for `release()` on its first call, to hold one write open deliberately. */
  function heldDigest() {
    const realSubtle = globalThis.crypto.subtle
    let release: () => void = () => {}
    const gate = new Promise<void>((resolve) => {
      release = resolve
    })
    let first = true
    let entered = false
    let calls = 0
    vi.stubGlobal('crypto', {
      subtle: {
        digest: async (algorithm: string, data: BufferSource) => {
          calls++
          if (first) {
            first = false
            entered = true
            await gate
          }
          return realSubtle.digest(algorithm, data)
        },
      },
    })
    return Object.assign(() => release(), { isHolding: () => entered, calls: () => calls })
  }

  /** Stores a variation at ply 1 for the owner, as an earlier page in this tab would have. */
  async function saveLineFor(owner: Ref<string | null>) {
    const writer = session({ persist: true, ownerScope: owner })
    writer.selectPly(1)
    await writer.tryMove('b9c7')
  }

  function expectNothingRestored(reader: ReturnType<typeof session>) {
    expect(reader.variation.value).toBeNull()
    expect(reader.parked.value).toBeNull()
    expect(reader.draftNotice.value).toBeNull()
  }

  /** Starts a save of game-1's variation for the owner and holds it at its digest. */
  async function holdFirstWrite(owner: Ref<string | null>, release: ReturnType<typeof heldDigest>) {
    const first = session({ persist: true, ownerScope: owner })
    first.selectPly(1)
    const move = first.tryMove('b9c7')
    await vi.waitFor(() => expect(release.isHolding()).toBe(true), { interval: 1 })
    // Wrapped, so awaiting this helper does not wait for the held save itself.
    return { move }
  }

  const storedRows = (memory: ReturnType<typeof memoryIndexedDb>) =>
    [...(memory.stores.get('replay_drafts')?.values() ?? [])] as Record<string, unknown>[]

  it('saves a variation and restores it in a new session, as active or at an explicit position', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const first = session({ persist: true, ownerScope: owner })
      first.selectPly(1)
      await first.tryMove('b9c7')
      await first.tryMove('b0c2')
      expect(first.saveState.value).toEqual({ state: 'saved' })
      expect(storedRows(memory)[0]).toMatchObject({ activeMode: 'variation', cursor: 1 })

      const active = session({ persist: true, ownerScope: owner })
      active.selectPly(3)
      await active.restoreDraft()
      expect(active.variation.value).toEqual({ originPly: 1, movesUci: ['b9c7', 'b0c2'] })
      expect(active.activePath.value).toEqual(['h2e2', 'b9c7', 'b0c2'])

      const linked = session({ persist: true, ownerScope: owner })
      linked.selectPly(0)
      await linked.restoreDraft({ keepPosition: true })
      expect(linked.cursor.value).toBe(0)
      expect(linked.variation.value).toBeNull()
      expect(linked.parked.value?.movesUci).toEqual(['b9c7', 'b0c2'])

      // A parked line brings back its saved cursor when no position is asked for.
      first.returnToRecordedLine()
      await vi.waitFor(() => expect(storedRows(memory)[0]).toMatchObject({ activeMode: 'recorded' }))
      const parked = session({ persist: true, ownerScope: owner })
      parked.selectPly(3)
      await parked.restoreDraft()
      expect(parked.cursor.value).toBe(1)
      expect(parked.parked.value?.movesUci).toEqual(['b9c7', 'b0c2'])

      // Another guest on the same device sees nothing.
      const other = session({ persist: true, ownerScope: ref('guest-2') })
      await other.restoreDraft()
      expect(other.parked.value).toBeNull()
      expect(other.variation.value).toBeNull()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('keeps a stored line resumable when the visitor follows the record during the read', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const first = session({ persist: true, ownerScope: owner })
      first.selectPly(1)
      await first.tryMove('b9c7')
      const next = session({ persist: true, ownerScope: owner })
      next.selectPly(1)
      const restoring = next.restoreDraft()
      expect((await next.tryMove('h9g7')).kind).toBe('followed')
      await restoring
      expect(next.cursor.value).toBe(2)
      expect(next.parked.value?.movesUci).toEqual(['b9c7'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('stores the newest line when an earlier write is held open', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    const release = heldDigest()
    try {
      const s = session({ persist: true, ownerScope: ref('guest-1') })
      s.selectPly(1)
      const firstMove = s.tryMove('b9c7')
      await vi.waitFor(() => expect(s.variation.value).not.toBeNull(), { interval: 1 })
      const secondMove = s.tryMove('b0c2')
      await vi.waitFor(() => expect(s.variation.value?.movesUci).toHaveLength(2), { interval: 1 })
      release()
      await Promise.all([firstMove, secondMove])
      expect(s.saveState.value).toEqual({ state: 'saved' })
      expect(storedRows(memory)[0]?.variationMovesUci).toEqual(['b9c7', 'b0c2'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('refuses malformed stored drafts with a notice and leaves them untouched', async () => {
    const owner = ref<string | null>('guest-1')
    const writer = session({ persist: true, ownerScope: owner })
    writer.selectPly(1)
    await writer.tryMove('b9c7')
    const good = (await writer.currentDraft())!
    const broken = [
      { cursor: 9 },
      { cursor: 1.5 },
      { activeMode: 'sideways' },
      { variationMovesUci: ['B9-C7'] },
    ]
    for (const change of broken) {
      const memory = memoryIndexedDb()
      vi.stubGlobal('indexedDB', memory)
      try {
        const row = { ...good, ...change }
        const before = structuredClone(row)
        memory.stores.set('local_games', new Map())
        memory.stores.set('replay_drafts', new Map([[row.draftId!, row]]))
        const reader = session({ persist: true, ownerScope: owner })
        await reader.restoreDraft()
        expect(reader.draftNotice.value, JSON.stringify(change)).toBeTruthy()
        expect(reader.variation.value).toBeNull()
        expect(reader.parked.value).toBeNull()
        expect(storedRows(memory)[0]).toEqual(before)

        // A damaged row cannot be restored, but a new variation may replace it.
        reader.selectPly(1)
        await reader.tryMove('b9c7')
        expect(reader.saveState.value, JSON.stringify(change)).toEqual({ state: 'saved' })
      } finally {
        vi.unstubAllGlobals()
      }
    }
  })

  it('records each write as the line stood when it began', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    const release = heldDigest()
    try {
      const s = session({ persist: true, ownerScope: ref('guest-1') })
      s.selectPly(1)
      const move = s.tryMove('b9c7')
      await vi.waitFor(() => expect(s.variation.value).not.toBeNull(), { interval: 1 })
      s.returnToRecordedLine()
      release()
      await move
      await vi.waitFor(() => expect(memory.puts).toHaveLength(2), { interval: 1 })
      expect(memory.puts.map((row) => row.activeMode)).toEqual(['variation', 'recorded'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('stops a draft save waiting on a blocked store while a local-game read keeps waiting', async () => {
    // Only timeouts are faked; real immediates let the core and digest finish without moving the clock.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const memory = memoryIndexedDb({ holdOpens: true })
    vi.stubGlobal('indexedDB', memory)
    try {
      const writer = session({ persist: true, ownerScope: ref('guest-1') })
      writer.selectPly(1)
      const localRead = loadOfflineGame('active_board')
      let localSettled = false
      void localRead.then(() => {
        localSettled = true
      })
      const move = writer.tryMove('b9c7')
      let moveSettled = false
      void move.then(() => {
        moveSettled = true
      })
      // The draft open starts only after the core and digest finish; wait for both held opens to exist.
      await memory.opened(2)
      await vi.advanceTimersByTimeAsync(2999)
      expect(moveSettled, JSON.stringify(writer.saveState.value)).toBe(false)
      await vi.advanceTimersByTimeAsync(1)
      await move
      expect(writer.saveState.value).toMatchObject({ state: 'unsaved', reason: expect.stringMatching(/Close other Jade tabs/) })
      expect(localSettled).toBe(false)
      memory.releaseOpens()
      await vi.advanceTimersByTimeAsync(10)
      expect(await localRead).toBeNull()
      // The draft's connection arrived after its caller gave up, so it was closed.
      expect(memory.closes).toBe(1)
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  it('reads a ply link only from a non-empty whole number', () => {
    expect(parsePlyQuery(null)).toBeNull()
    expect(parsePlyQuery('')).toBeNull()
    expect(parsePlyQuery(undefined)).toBeNull()
    expect(parsePlyQuery('1.5')).toBeNull()
    expect(parsePlyQuery('-1')).toBeNull()
    expect(parsePlyQuery('0')).toBe(0)
    expect(parsePlyQuery('12')).toBe(12)
    expect(parsePlyQuery('9007199254740991')).toBe(Number.MAX_SAFE_INTEGER)
    expect(parsePlyQuery('9007199254740992')).toBeNull()
    expect(parsePlyQuery('9'.repeat(309))).toBeNull()
  })

  it('refuses a blocked open at once and closes the connection that arrives later', async () => {
    // The deadline clock never moves, so only the blocked event can settle the save.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    const memory = memoryIndexedDb({ holdOpens: true, blockOpens: true })
    vi.stubGlobal('indexedDB', memory)
    try {
      const s = session({ persist: true, ownerScope: ref('guest-1') })
      s.selectPly(1)
      await s.tryMove('b9c7')
      expect(s.saveState.value).toMatchObject({ state: 'unsaved', reason: expect.stringMatching(/Close other Jade tabs/) })
      memory.releaseOpens()
      await vi.advanceTimersByTimeAsync(10)
      expect(memory.closes).toBe(1)
    } finally {
      vi.useRealTimers()
      vi.unstubAllGlobals()
    }
  })

  it('keeps the latest save when two tabs edit the same game', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const writer = session({ persist: true, ownerScope: owner })
      writer.selectPly(1)
      await writer.tryMove('b9c7')
      const tabA = session({ persist: true, ownerScope: owner })
      const tabB = session({ persist: true, ownerScope: owner })
      await tabA.restoreDraft()
      await tabB.restoreDraft()
      await tabA.tryMove('b0c2')
      await tabB.tryMove('h0g2')
      expect(tabA.saveState.value).toEqual({ state: 'saved' })
      expect(tabB.saveState.value).toEqual({ state: 'saved' })
      expect(storedRows(memory)[0]?.variationMovesUci).toEqual(['b9c7', 'h0g2'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it("never shows one guest's stored variation after the guest changes during the read", async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-a')
      await saveLineFor(owner)
      const reader = session({ persist: true, ownerScope: owner })
      const restoring = reader.restoreDraft()
      owner.value = 'guest-b'
      await restoring
      expectNothingRestored(reader)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it("never shows one guest's stored variation after the guest changes during validation", async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-a')
      await saveLineFor(owner)
      const release = heldDigest()
      const reader = session({ persist: true, ownerScope: owner })
      const restoring = reader.restoreDraft()
      // The read has returned guest A's line; the check against the record is held at its digest.
      await vi.waitFor(() => expect(release.isHolding()).toBe(true), { interval: 1 })
      owner.value = 'guest-b'
      release()
      await restoring
      expectNothingRestored(reader)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('stores the newer line when a page reopened in the same tab saves before the old page', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    const release = heldDigest()
    const moves: Promise<unknown>[] = []
    try {
      const owner = ref<string | null>('guest-1')
      moves.push((await holdFirstWrite(owner, release)).move)
      const reopened = session({ persist: true, ownerScope: owner })
      reopened.selectPly(0)
      const reopenedMove = reopened.tryMove('b0c2')
      moves.push(reopenedMove)
      await vi.waitFor(() => expect(reopened.saveState.value).toEqual({ state: 'pending' }), { interval: 1 })
      // The line changes while the first save waits; each queued save keeps the line of its own edit.
      reopened.returnToRecordedLine()
      // The guest changes while both saves wait; they keep the guest that made the line.
      owner.value = 'guest-2'
      // Every queued step has had its turn: the reopened write has not even started its digest.
      await new Promise((resolve) => setImmediate(resolve))
      expect(release.calls()).toBe(1)
      release()
      await Promise.all(moves)
      await vi.waitFor(() => expect(memory.puts).toHaveLength(3), { interval: 1 })
      expect(memory.puts.map((row) => [row.ownerScope, row.activeMode, row.variationMovesUci])).toEqual([
        ['guest-1', 'variation', ['b9c7']],
        ['guest-1', 'variation', ['b0c2']],
        ['guest-1', 'recorded', ['b0c2']],
      ])
      expect(storedRows(memory)).toEqual([expect.objectContaining({ draftId: expect.stringMatching(/:guest-1$/) })])
      // The last old-guest write has completed (put, then its transaction) and reported nothing here.
      for (let turn = 0; turn < 3; turn++) await new Promise((resolve) => setTimeout(resolve, 0))
      expect(reopened.saveState.value).toEqual({ state: 'none' })
    } finally {
      release()
      await Promise.allSettled(moves)
      vi.unstubAllGlobals()
    }
  })

  it("saves another game's variation while one game's write is held", async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    const release = heldDigest()
    const moves: Promise<unknown>[] = []
    try {
      const owner = ref<string | null>('guest-1')
      moves.push((await holdFirstWrite(owner, release)).move)
      const other = session({
        persist: true,
        ownerScope: owner,
        source: ref({ kind: 'archive', id: 'game-2', revision: 'release-1' }),
      })
      other.selectPly(0)
      moves.push(other.tryMove('b0c2'))
      // Bounded, so a regression that queues this save behind the held one still reaches finally.
      await vi.waitFor(() => expect(other.saveState.value).toEqual({ state: 'saved' }), { timeout: 2000, interval: 1 })
      expect(storedRows(memory).map((row) => row.sourceId)).toEqual(['game-2'])
    } finally {
      release()
      await Promise.allSettled(moves)
      vi.unstubAllGlobals()
    }
  })

  it('stores the selected recorded ply when navigation leaves a variation', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const s = session({ persist: true, ownerScope: ref('guest-1') })
      s.selectPly(1)
      await s.tryMove('b9c7')
      s.selectPly(3)
      await vi.waitFor(() => expect(memory.puts).toHaveLength(2), { interval: 1 })
      expect(storedRows(memory)[0]).toMatchObject({ activeMode: 'recorded', cursor: 3 })
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('drops the line in memory as soon as the guest changes', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const s = session({ persist: true, ownerScope: owner })
      s.selectPly(1)
      await s.tryMove('b9c7')
      // An edit in the same turn as the guest change finds no line to store.
      owner.value = 'guest-2'
      s.returnToRecordedLine()
      expect(s.variation.value).toBeNull()
      expect(s.parked.value).toBeNull()
      await vi.waitFor(() => expect(memory.puts).toHaveLength(1), { interval: 1 })
      expect(memory.puts.map((row) => row.ownerScope)).toEqual(['guest-1'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('refuses a move begun before the guest changed', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const s = session({ persist: true, ownerScope: owner })
      s.selectPly(1)
      const move = s.tryMove('b9c7')
      owner.value = 'guest-2'
      expect(await move).toMatchObject({ kind: 'rejected' })
      expect(s.variation.value).toBeNull()
      expect(memory.puts).toHaveLength(0)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('keeps an edit made during a restore over the stored line', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const owner = ref<string | null>('guest-1')
      const writer = session({ persist: true, ownerScope: owner })
      writer.selectPly(1)
      await writer.tryMove('b9c7')
      const release = heldDigest()
      const late = session({ persist: true, ownerScope: owner })
      late.selectPly(0)
      const restoring = late.restoreDraft()
      // The restore is now checking the stored line against the record, held at its digest.
      await vi.waitFor(() => expect(release.isHolding()).toBe(true), { interval: 1 })
      await late.tryMove('b0c2')
      release()
      await restoring
      expect(late.variation.value?.movesUci).toEqual(['b0c2'])
      expect(late.parked.value).toBeNull()
      expect(late.saveState.value).toEqual({ state: 'saved' })
      expect(storedRows(memory)[0]?.variationMovesUci).toEqual(['b0c2'])
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('offers export without opening storage when the guest is unknown', async () => {
    const memory = memoryIndexedDb()
    vi.stubGlobal('indexedDB', memory)
    try {
      const s = session({
        persist: false,
        ownerScope: ref(null),
        unpersistedReason: ref('This tab does not know your guest identity, so the variation is not kept'),
      })
      s.selectPly(1)
      await s.tryMove('b9c7')
      expect(s.saveState.value).toMatchObject({ state: 'unsaved' })
      const draft = JSON.parse(await (await s.exportDraft())!.text())
      expect(draft.variationMovesUci).toEqual(['b9c7'])
      expect(memory.opens).toBe(0)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('refuses a second extension validated against the same end of a local game', async () => {
    const s = session({ extendAtEnd: true })
    s.selectPly(3)
    const results = await Promise.all([s.tryMove('b9c7'), s.tryMove('i9i8')])
    expect(results.map((r) => r.kind).sort()).toEqual(['extend', 'rejected'])
  })

  it('hands a move at the end of a local game back to the caller to extend', async () => {
    const s = session({ extendAtEnd: true })
    s.selectPly(3)
    const result = await s.tryMove('b9c7')
    expect(result).toMatchObject({ kind: 'extend', uci: 'b9c7' })
    expect(s.variation.value).toBeNull()
  })
})
