import { describe, expect, it } from 'vitest'
import { BrowserWorkerSearchAdapter } from '@jade/wasm-worker'
import {
  applyCoreMove,
  computeBoardView,
  hitTestBoard,
  querySamplePosition,
  searchSampleArchive,
} from '../../app/utils/core-wasm'
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
