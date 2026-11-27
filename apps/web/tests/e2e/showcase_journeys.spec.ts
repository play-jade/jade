import { resolve } from 'node:path'
import type { Page, WebSocketRoute } from '@playwright/test'
import { expect, test } from '@playwright/test'

const VISUAL_REF_DIR = resolve(process.cwd(), '../../docs/visual-reference')

/** The number of replay drafts this browser holds, read from the page's own IndexedDB. */
async function storedDraftCount(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      new Promise<number>((resolve, reject) => {
        const open = indexedDB.open('jade_offline_v1')
        open.onerror = () => reject(open.error)
        open.onsuccess = () => {
          const db = open.result
          if (!db.objectStoreNames.contains('replay_drafts')) {
            db.close()
            resolve(0)
            return
          }
          const count = db.transaction('replay_drafts', 'readonly').objectStore('replay_drafts').count()
          count.onsuccess = () => {
            db.close()
            resolve(count.result)
          }
          count.onerror = () => reject(count.error)
        }
      }),
  )
}

async function playBoardSquares(page: Page, fromSq: string, toSq: string) {
  await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
  await page.locator(`[data-square="${fromSq}"]`).dispatchEvent('click')
  await expect(page.locator(`[data-square="${fromSq}"]`)).toHaveAttribute('aria-label', /selected/)
  await page.locator(`[data-square="${toSq}"]`).dispatchEvent('click')
}

test.describe('Jade Slice A Web Showcase Journeys', () => {
  test('0. Home board renders on first load without any interaction', async ({ page, request }) => {
    const ssr = await request.get('/')
    const html = await ssr.text()
    expect(html).toContain('data-testid="xiangqi-board"')
    expect(html).not.toContain('core_wasm.wasm not found')

    await page.goto('/')
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
    await expect(page.locator('[data-square="e0"]')).toHaveAttribute('aria-label', /\(/)
  })

  test('1. Home board, tactile move play, preference switching without state loss, and local engine worker', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 860 })
    await page.goto('/')

    await expect(page.getByTestId('xiangqi-board')).toBeVisible()
    await expect(page.getByTestId('board-status-strip')).toContainText('Red to move')

    // Play Cannon 2 to 5 (`h2` -> `e2`) and verify English-locale WXF notation + turn switch
    await playBoardSquares(page, 'h2', 'e2')

    await expect(page.getByTestId('move-list')).toContainText('C2=5')
    await expect(page.getByTestId('board-status-strip')).toContainText('Black to move')

    // Capture desktop light-paper visual reference plate
    await page.waitForTimeout(200)
    await page.screenshot({
      path: resolve(VISUAL_REF_DIR, 'web-home-desktop-paper.png'),
      fullPage: false,
    })

    // Switch locale (`zh-Hant-HK`), material (`boxwood`), notation (`uci`), and theme (`Dark`) WITHOUT losing active move!
    // Preferences live in the settings sheet, as on macOS.
    await page.getByTestId('settings-toggle').click()
    // The sheet is modal: focus moves in, the page behind it is inert, and Tab wraps inside the sheet.
    await expect(page.getByTestId('pref-locale')).toBeFocused()
    await expect(page.locator('main')).toHaveAttribute('inert', '')
    await page.getByTestId('pref-sound').focus()
    await page.keyboard.press('Tab')
    await expect(page.getByTestId('pref-locale')).toBeFocused()
    await page.getByTestId('pref-locale').selectOption('zh-Hant-HK')
    await page.getByTestId('pref-material').selectOption('boxwood')
    await page.getByTestId('pref-notation').selectOption('uci')
    await page.getByTestId('pref-theme').click()
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('settings-sheet')).toBeHidden()
    await expect(page.getByTestId('settings-toggle')).toBeFocused()
    await expect(page.locator('main')).not.toHaveAttribute('inert', '')

    await expect(page.getByTestId('move-list')).toContainText('h2e2')
    await expect(page.getByTestId('board-status-strip')).toContainText('黑方行棋')

    // Run local browser Web Worker engine analysis
    await page.getByTestId('engine-toggle-btn').click()
    await expect(page.getByTestId('engine-pv')).toBeVisible({ timeout: 10_000 })

    await page.screenshot({
      path: resolve(VISUAL_REF_DIR, 'web-home-desktop-dark.png'),
      fullPage: false,
    })
  })

  test('2. Mobile 390x844 layout without horizontal overflow', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto('/')
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()

    const hasHorizontalOverflow = await page.evaluate(() => {
      return document.documentElement.scrollWidth > window.innerWidth + 1
    })
    expect(hasHorizontalOverflow).toBe(false)

    await page.waitForTimeout(200)
    await page.screenshot({
      path: resolve(VISUAL_REF_DIR, 'web-board-mobile-390x844.png'),
      fullPage: false,
    })
  })

  test('3. Two independent browser contexts: friendly 10+5 invitation, live moves, agreed draw, postgame branch tryout, and rematch', async ({
    browser,
  }) => {
    const redContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    const blkContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    const redPage = await redContext.newPage()
    const blkPage = await blkContext.newPage()

    try {
      await redPage.goto('/')
      await expect(redPage.locator('[data-canvas-ready="true"]')).toBeVisible()
      await redPage.getByTestId('create-invite-btn').click()
      await redPage.waitForURL(/\/invite\/[a-z0-9]+/)
      await expect(redPage.getByTestId('match-status-badge')).toContainText('Waiting for friend')

      const inviteUrl = await redPage.getByTestId('share-link-input').inputValue()
      expect(inviteUrl).toContain('/invite/')
      expect(inviteUrl).not.toContain('jtok_')

      // Black joins via the clean shareable URL
      await blkPage.goto(new URL(inviteUrl).pathname)
      await expect(blkPage.getByTestId('match-status-badge')).toContainText('You play black')
      await expect(redPage.getByTestId('match-status-badge')).toContainText('You play red', {
        timeout: 6_000,
      })

      // Red plays h2 -> e2; Black sees C2=5 (WXF under the English locale)!
      await playBoardSquares(redPage, 'h2', 'e2')
      await expect(redPage.getByTestId('match-move-list')).toContainText('C2=5')
      await expect(blkPage.getByTestId('match-move-list')).toContainText('C2=5', {
        timeout: 6_000,
      })

      // Black plays h9 -> g7; Red sees H8+7!
      await playBoardSquares(blkPage, 'h9', 'g7')
      await expect(redPage.getByTestId('match-move-list')).toContainText('H8+7', {
        timeout: 6_000,
      })

      // Force-drop Red's live WebSocket and verify Red recovers gap AND reopens a brand-new live WebSocket for subsequent moves!
      await redPage.waitForFunction(() => {
        const s = (window as unknown as Record<string, WebSocket | undefined>).__jadeActiveMatchSocket
        return Boolean(s && s.readyState === WebSocket.OPEN)
      })
      await redPage.evaluate(() => {
        const sock = (window as unknown as Record<string, (WebSocket & { __droppedForTest?: boolean }) | undefined>).__jadeActiveMatchSocket
        if (sock) {
          sock.__droppedForTest = true
          sock.close()
        }
      })
      await redPage.waitForFunction(
        () => {
          const s = (window as unknown as Record<string, (WebSocket & { __droppedForTest?: boolean }) | undefined>).__jadeActiveMatchSocket
          return Boolean(s && !s.__droppedForTest && s.readyState === WebSocket.OPEN)
        },
        undefined,
        { timeout: 6_000 },
      )
      await expect(redPage.getByTestId('match-status-badge')).toHaveAttribute(
        'data-connection-state',
        'connected',
        { timeout: 6_000 },
      )
      await playBoardSquares(redPage, 'h0', 'g2')
      await expect(blkPage.getByTestId('match-move-list')).toContainText('H2+3', {
        timeout: 6_000,
      })
      await playBoardSquares(blkPage, 'i9', 'h9')
      await expect(redPage.getByTestId('match-move-list')).toContainText('R9=8', {
        timeout: 6_000,
      })

      await redPage.screenshot({
        path: resolve(VISUAL_REF_DIR, 'web-online-match-live.png'),
        fullPage: false,
      })

      // Red offers draw; Black accepts draw
      await redPage.getByTestId('btn-offer-draw').click()
      await expect(blkPage.getByTestId('btn-accept-draw')).toBeVisible({ timeout: 6_000 })
      await blkPage.getByTestId('btn-accept-draw').click()

      await expect(redPage.getByTestId('match-status-badge')).toContainText('agreed_draw', {
        timeout: 6_000,
      })

      // Postgame replay scrub to start & try alternative line (`b2e2`) without overwriting match moves
      await redPage.getByTestId('postgame-scrub-start').click()
      await playBoardSquares(redPage, 'b2', 'e2')
      await expect(redPage.getByTestId('postgame-branch-banner')).toContainText('b2e2')

      // Black opens the finished game's replay, which sits at the same table as play
      const replayHref = await blkPage.getByTestId('link-open-replay').getAttribute('href')
      await blkPage.goto(replayHref ?? '')
      await expect(blkPage.getByTestId('match-replay-page')).toContainText('C2=5')
      await expect(blkPage.locator('.jade-table .jade-capsule')).toBeVisible()
      await blkPage.screenshot({ path: test.info().outputPath('match-replay.png') })

      // Black tries another reply at ply 1, then the engine's suggestion; the recorded game stays as played.
      const recorded = blkPage.getByTestId('recorded-move-list').locator('button')
      await expect(recorded).toHaveCount(4)
      const recordedText = await blkPage.getByTestId('recorded-move-list').innerText()
      await recorded.first().click()
      await playBoardSquares(blkPage, 'b9', 'c7')
      const variationMoves = blkPage.getByTestId('variation-moves').locator('li')
      await expect(variationMoves).toHaveCount(1)
      await blkPage.getByTestId('engine-toggle-btn').click()
      await expect(blkPage.getByTestId('engine-play-best')).toBeVisible({ timeout: 10_000 })
      await blkPage.getByTestId('engine-play-best').click()
      await expect(variationMoves).toHaveCount(2)
      await expect(blkPage.getByTestId('variation-saved')).toBeVisible()
      expect(await blkPage.getByTestId('recorded-move-list').innerText()).toBe(recordedText)

      // After a reload at the linked ply, the kept variation comes back to resume, with both moves.
      await blkPage.reload()
      await expect(blkPage.getByTestId('variation-resume')).toBeVisible({ timeout: 10_000 })
      await blkPage.getByTestId('variation-resume').click()
      await expect(blkPage.getByTestId('variation-moves').locator('li')).toHaveCount(2)
      expect(await blkPage.getByTestId('recorded-move-list').innerText()).toBe(recordedText)

      // Rematch creates fresh invitation cleanly
      await redPage.getByTestId('btn-rematch').click()
      await expect(redPage.getByTestId('match-status-badge')).toContainText('Waiting for friend')
    } finally {
      await redContext.close()
      await blkContext.close()
    }
  })

  test('4. Public Sample Archive & Player Profile SSR with JavaScript completely disabled', async ({
    browser,
  }) => {
    const noJsContext = await browser.newContext({
      javaScriptEnabled: false,
      viewport: { width: 1240, height: 860 },
    })
    const page = await noJsContext.newPage()

    try {
      await page.goto('/archive')
      await expect(page.getByTestId('archive-provenance-banner')).toContainText(
        'jade-public-sample-fixtures-v1',
      )
      await expect(page.getByTestId('archive-game-list').locator('li')).toHaveCount(2)

      // Verify both disambiguated `林明` records appear distinctly in SSR HTML
      const playerItems = page.locator('[data-testid="player-list"] li')
      await expect(playerItems).toHaveCount(4)
      await expect(page.getByTestId('player-list')).toContainText('Lin, Ming (Club A)')
      await expect(page.getByTestId('player-list')).toContainText('Lin, Ming (Youth Open)')

      await page.screenshot({
        path: resolve(VISUAL_REF_DIR, 'web-archive-ssr-nojs.png'),
        fullPage: false,
      })

      // A game's own route renders its board and the archive's continuations without JavaScript.
      await page.goto('/archive/g/sample-game-central-cannon-001')
      await expect(page.getByTestId('archive-provenance-banner')).toContainText('jade-public-sample-fixtures-v1')
      await expect(page.getByTestId('continuation-table')).toContainText('炮二平五')
      await expect(page.getByTestId('explorer-source-label')).toContainText('Sample Archive')
      await expect(page.getByTestId('archive-move-list')).toContainText('C2=5')

      // Navigate to player profile with JS disabled
      await page.goto('/players/sample-player-001')
      await expect(page.getByTestId('player-profile-page')).toContainText('赵汝权')
      await expect(page.getByTestId('player-profile-page')).toContainText('Chiu, Yu Kuen')

      // A page past the player's last game says so and links back to it.
      await page.goto('/players/sample-player-001?offset=25')
      await expect(page.getByTestId('player-games-past-end')).toContainText("this player's 1 games")
      await page.getByTestId('player-games-past-end').getByRole('link').click()
      await expect(page.getByTestId('player-game-list').locator('li')).toHaveCount(1)
      await expect(page.getByTestId('player-game-list').getByRole('link')).toHaveAttribute(
        'href',
        '/archive/g/sample-game-central-cannon-001',
      )

      // A search submitted without scripts keeps the game page shown.
      await page.goto('/archive?offset=1')
      await expect(page.getByTestId('archive-game-list').locator('li')).toHaveCount(1)
      await page.getByTestId('player-search-input').fill('Ming')
      await page.getByTestId('player-search-btn').click()
      await expect(page).toHaveURL(/offset=1/)
      await expect(page).toHaveURL(/q=Ming/)
      await expect(page.getByTestId('player-list').locator('li')).toHaveCount(2)
      await expect(page.getByTestId('player-list').locator('li[data-player-id="sample-player-003"]')).toHaveCount(1)
      await expect(page.getByTestId('player-list').locator('li[data-player-id="sample-player-004"]')).toHaveCount(1)
      await expect(page.getByTestId('archive-game-list').getByRole('link')).toHaveCount(1)
      await expect(page.getByTestId('archive-game-list').getByRole('link')).toHaveAttribute(
        'href',
        '/archive/g/sample-game-screen-horse-002',
      )
    } finally {
      await noJsContext.close()
    }
  })

  test('6. Local variation: return, resume, and reload keep it, with the board and notation agreeing', async ({
    page,
  }) => {
    await page.goto('/')
    await playBoardSquares(page, 'h2', 'e2')
    await playBoardSquares(page, 'h9', 'g7')
    await expect(page.getByTestId('move-list')).toContainText('H8+7')

    // Back one ply, then Black tries another reply: a variation, kept on this device.
    await page.getByTestId('scrub-prev').click()
    await playBoardSquares(page, 'b9', 'c7')
    const variationMoves = page.getByTestId('variation-moves').locator('li')
    await expect(variationMoves).toHaveCount(1)
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    const tried = (await variationMoves.first().locator('.variation-label').innerText()).trim()
    await expect(page.getByTestId('move-list')).toContainText(tried)
    await expect(page.getByTestId('move-list')).not.toContainText('H8+7')

    // Return shows the game again and keeps the variation to resume.
    await page.getByTestId('variation-return').click()
    await expect(page.getByTestId('variation-resume')).toBeVisible()
    await expect(page.getByTestId('move-list')).not.toContainText(tried)

    // Resume brings it back; after a reload the game and the variation both return.
    await page.getByTestId('variation-resume').click()
    await expect(page.getByTestId('move-list')).toContainText(tried)
    // Saving is asynchronous; the reload waits until the device holds the resumed line.
    await expect(page.getByTestId('variation-saving')).toHaveCount(0)
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    await page.reload()
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
    await expect(page.getByTestId('variation-moves').locator('li')).toHaveCount(1, { timeout: 10_000 })
    await expect(page.getByTestId('move-list')).toContainText(tried)
    await expect(page.getByTestId('variation-return')).toBeVisible()

    // New game clears the board and the kept variation; a reload brings neither back.
    await page.getByTestId('btn-reset').click()
    await expect(page.getByTestId('variation-bar')).toHaveCount(0)
    // Removal is asynchronous and shows nothing on screen, so the reload waits until the device holds no draft.
    await expect.poll(() => storedDraftCount(page)).toBe(0)
    await page.reload()
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
    await expect(page.getByTestId('variation-bar')).toHaveCount(0)

    // Undo on the board takes back a variation's only move, and a reload does not bring it back.
    await playBoardSquares(page, 'h2', 'e2')
    await playBoardSquares(page, 'h9', 'g7')
    await page.getByTestId('scrub-prev').click()
    await playBoardSquares(page, 'b9', 'c7')
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    await page.getByTestId('btn-undo').click()
    await expect(page.getByTestId('variation-bar')).toHaveCount(0)
    await expect.poll(() => storedDraftCount(page)).toBe(0)
    await page.reload()
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
    await expect(page.getByTestId('move-list')).toContainText('H8+7')
    await expect(page.getByTestId('variation-bar')).toHaveCount(0)

    // Undo of a game move after the kept variation's start stores it again, so a reload still offers it.
    await page.getByTestId('scrub-prev').click()
    await playBoardSquares(page, 'b9', 'c7')
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    await page.getByTestId('variation-return').click()
    await page.getByTestId('btn-undo').click()
    await expect(page.getByTestId('move-list')).not.toContainText('H8+7')
    await expect(page.getByTestId('variation-saving')).toHaveCount(0)
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    await page.reload()
    await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
    await expect(page.getByTestId('variation-resume')).toBeVisible({ timeout: 10_000 })

    // Undo of the move it started from removes it, says so, and leaves nothing stored.
    await page.getByTestId('btn-undo').click()
    await expect(page.getByTestId('variation-notice')).toContainText('taken back')
    await expect(page.getByTestId('variation-resume')).toHaveCount(0)
    await expect.poll(() => storedDraftCount(page)).toBe(0)

    // A removal the browser refuses stays visible as a notice, with no save status or export for a line that is gone.
    await playBoardSquares(page, 'h2', 'e2')
    await playBoardSquares(page, 'h9', 'g7')
    await page.getByTestId('scrub-prev').click()
    await playBoardSquares(page, 'b9', 'c7')
    await expect(page.getByTestId('variation-saved')).toBeVisible()
    await page.evaluate(() => {
      IDBObjectStore.prototype.delete = () => {
        throw new Error('removal refused for this test')
      }
    })
    await page.getByTestId('btn-undo').click()
    await expect(page.getByTestId('variation-notice')).toContainText('could not be removed')
    await expect(page.getByTestId('variation-export')).toHaveCount(0)
    await expect(page.getByTestId('variation-unsaved')).toHaveCount(0)
    await expect(page.getByTestId('variation-saved')).toHaveCount(0)
  })

  test('7. Without browser storage, a variation reports it is not saved and still exports', async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(window, 'indexedDB', { configurable: true, get: () => undefined })
    })
    await page.goto('/')
    await playBoardSquares(page, 'h2', 'e2')
    await playBoardSquares(page, 'h9', 'g7')
    await page.getByTestId('scrub-prev').click()
    await playBoardSquares(page, 'b9', 'c7')
    await expect(page.getByTestId('variation-unsaved')).toContainText('Browser storage is not available')
    await expect(page.getByTestId('variation-saved')).toHaveCount(0)

    const download = page.waitForEvent('download')
    await page.getByTestId('variation-export').click()
    const file = await download
    expect(file.suggestedFilename()).toBe('jade-local-variation.json')
    const draft = JSON.parse(await (await import('node:fs/promises')).readFile((await file.path())!, 'utf8'))
    expect(draft.variationMovesUci).toEqual(['b9c7'])
    expect(draft.variationOriginPly).toBe(1)
  })

  test('8. Archive game: choose, reload its URL, try a recorded continuation and the engine, return, recover', async ({
    page,
  }) => {
    // Choose the first sample game from the collection; its route is stable.
    await page.goto('/archive')
    await page.getByTestId('archive-game-list').getByRole('link').first().click()
    await page.waitForURL(/\/archive\/g\/sample-game-central-cannon-001/)
    await expect(page.getByTestId('archive-recorded-result')).toBeVisible()

    // Step to ply 3; reloading the URL keeps the position.
    const moves = page.getByTestId('archive-move')
    await moves.nth(2).click()
    await page.waitForURL(/ply=3/)
    await page.reload()
    await expect(page.getByTestId('archive-ply')).toContainText('3 /')
    const recordedText = await page.getByTestId('archive-move-list').innerText()

    // The other sample game continued 3… b9c7 here: trying it starts a variation, then the engine extends it.
    await page.locator('[data-testid="continuation-row"][data-move="b9c7"]').click()
    const variationMoves = page.getByTestId('variation-moves').locator('li')
    await expect(variationMoves).toHaveCount(1)
    await page.getByTestId('engine-toggle-btn').click()
    await expect(page.getByTestId('engine-play-best')).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('engine-play-best').click()
    await expect(variationMoves).toHaveCount(2)
    await expect(page.getByTestId('variation-saving')).toHaveCount(0)
    await expect(page.getByTestId('variation-saved')).toBeVisible()

    // Return to the recorded game: the record is unchanged and the variation waits to resume.
    await page.getByTestId('variation-return').click()
    await expect(page.getByTestId('variation-resume')).toBeVisible()
    await expect(page.getByTestId('variation-saving')).toHaveCount(0)
    expect(await page.getByTestId('archive-move-list').innerText()).toBe(recordedText)

    // After a reload the kept variation comes back to resume, with both moves.
    await page.reload()
    await expect(page.getByTestId('variation-resume')).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('variation-resume').click()
    await expect(page.getByTestId('variation-moves').locator('li')).toHaveCount(2)

    // The home page links to the kept variation.
    await page.goto('/')
    await page.getByTestId('kept-archive-variation-link').click()
    await page.waitForURL(/\/archive\/g\/sample-game-central-cannon-001/)
    await expect(page.getByTestId('variation-moves').locator('li')).toHaveCount(2, { timeout: 10_000 })
  })

  test('9. Archive history: one position by two move orders, and search restored by Back', async ({ page }) => {
    // Game 1 plays 4. i9h9 5. i0h0 6. b9c7. A variation from ply 3 plays the same moves in another order, reaching
    // the same position; the recorded ply 6 is a different replay state, so its continuations load for it.
    await page.goto('/archive/g/sample-game-central-cannon-001?ply=3')
    const lineMoves = page.getByTestId('variation-moves').locator('li')
    await page.locator('[data-testid="continuation-row"][data-move="b9c7"]').click()
    await expect(lineMoves).toHaveCount(1)
    await playBoardSquares(page, 'i0', 'h0')
    await expect(lineMoves).toHaveCount(2)
    await playBoardSquares(page, 'i9', 'h9')
    await expect(lineMoves).toHaveCount(3)
    const c3c4 = page.locator('[data-testid="continuation-row"][data-move="c3c4"]')
    await expect(c3c4).toBeVisible()
    // Selecting recorded ply 6 parks the variation and shows the recorded state, whose own lookup offers c3c4.
    await page.getByTestId('archive-move').nth(5).click()
    await expect(page.getByTestId('archive-move').nth(5)).toHaveAttribute('aria-current', 'step')
    await expect(page.getByTestId('variation-resume')).toBeVisible()
    await expect(c3c4).toBeVisible()

    // A capture on the archive board is announced in the board's live region.
    await page.goto('/archive/g/sample-game-central-cannon-001?ply=0')
    await playBoardSquares(page, 'h2', 'h9')
    await expect(page.getByTestId('variation-moves').locator('li')).toHaveCount(1)
    await expect(page.locator('[aria-live="polite"]').filter({ hasText: 'capture' })).toHaveCount(1)

    // A submitted search lives in the URL: Back restores the earlier query in the box and in the results.
    await page.goto('/archive')
    await page.getByTestId('player-search-input').fill('Chiu')
    await page.getByTestId('player-search-btn').click()
    await expect(page).toHaveURL(/q=Chiu/)
    await expect(page.getByTestId('player-list').locator('li')).toHaveCount(1)
    await page.getByTestId('player-search-input').fill('Ming')
    await page.getByTestId('player-search-btn').click()
    await expect(page).toHaveURL(/q=Ming/)
    await expect(page.getByTestId('player-list').locator('li')).toHaveCount(2)
    await page.goBack()
    await expect(page).toHaveURL(/q=Chiu/)
    await expect(page.getByTestId('player-search-input')).toHaveValue('Chiu')
    await expect(page.getByTestId('player-list').locator('li')).toHaveCount(1)
  })

  test('10. Guest return: a new tab, a reopened browser, and saved choices keep the participant', async ({ browser }) => {
    const redContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    const blkContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    const contexts = [redContext, blkContext]
    try {
      const redPage = await redContext.newPage()
      const blkPage = await blkContext.newPage()
      await redPage.goto('/')
      await expect(redPage.locator('[data-canvas-ready="true"]')).toBeVisible()
      await redPage.getByTestId('create-invite-btn').click()
      await redPage.waitForURL(/\/invite\/[a-z0-9]+/)
      const room = new URL(redPage.url()).pathname
      await blkPage.goto(room)
      await expect(blkPage.getByTestId('match-status-badge')).toContainText('You play black')
      await expect(redPage.getByTestId('match-status-badge')).toContainText('You play red', { timeout: 6_000 })

      // A second tab of the same browser is the same guest, in the same seat.
      const tab = await redContext.newPage()
      await tab.goto(room)
      await expect(tab.getByTestId('match-status-badge')).toContainText('You play red')

      // The participant's saved choices come back after a reload; the match stays private until both choose.
      // Each choice saves as soon as it changes; the test waits for each save's own request.
      const saved = (page: typeof tab) =>
        page.waitForResponse((r) => r.url().endsWith('/privacy') && r.request().method() === 'POST')
      await tab.getByTestId('privacy-consent-card').locator('summary').click()
      let save = saved(tab)
      await tab.getByTestId('privacy-visibility').selectOption('public')
      expect((await save).status()).toBe(200)
      save = saved(tab)
      await tab.getByLabel('Allow internal model training').check()
      expect((await save).status()).toBe(200)
      await expect(tab.getByTestId('privacy-effective')).toContainText('Private')
      await tab.reload()
      await expect(tab.getByTestId('match-status-badge')).toContainText('You play red')
      await tab.getByTestId('privacy-consent-card').locator('summary').click()
      await expect(tab.getByTestId('privacy-visibility')).toHaveValue('public')
      await expect(tab.getByLabel('Allow internal model training')).toBeChecked()
      await expect(tab.getByLabel('Allow open dataset release')).not.toBeChecked()

      // A withdrawal is saved by itself too, and stays withdrawn after a reload.
      save = saved(tab)
      await tab.getByLabel('Allow internal model training').uncheck()
      expect((await save).status()).toBe(200)
      await tab.reload()
      await tab.getByTestId('privacy-consent-card').locator('summary').click()
      await expect(tab.getByLabel('Allow internal model training')).not.toBeChecked()
      await expect(tab.getByTestId('privacy-visibility')).toHaveValue('public')

      // Closing the browser and opening it again keeps the guest while its session lasts.
      const kept = await redContext.storageState()
      await redContext.close()
      const reopened = await browser.newContext({ storageState: kept, viewport: { width: 1200, height: 840 } })
      contexts.push(reopened)
      const back = await reopened.newPage()
      await back.goto(room)
      await expect(back.getByTestId('match-status-badge')).toContainText('You play red')

      // A browser without that session is another guest, which takes no seat in this private match.
      const other = await browser.newContext({ viewport: { width: 1200, height: 840 } })
      contexts.push(other)
      const stranger = await other.newPage()
      const joinAnswered = stranger.waitForResponse((r) => r.url().includes('/join'))
      await stranger.goto(room)
      // Both seats are taken and the match is private, so the room is refused to this browser.
      const join = await joinAnswered
      expect(join.status()).toBe(403)
      expect((await join.json()).error.code).toBe('private_match')
      await expect(stranger.getByTestId('match-status-badge')).not.toContainText('You play')
    } finally {
      for (const context of contexts) await context.close().catch(() => undefined)
    }
  })

  test('11. The sharing summary keeps the newest state, and a room stops when its session ends', async ({ browser }) => {
    const redContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    const blkContext = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    try {
      const redPage = await redContext.newPage()
      const blkPage = await blkContext.newPage()
      // Red's live streams pass through to the server; the test can drop the current one as a network would.
      const redStreams: WebSocketRoute[] = []
      let withholdRedFrames = false
      const heldRedFrames: string[] = []
      let lastRedFrame = ''
      await redPage.routeWebSocket(/\/api\/v1\/matches\/[^/]+\/ws$/, (stream) => {
        redStreams.push(stream)
        const server = stream.connectToServer()
        server.onMessage((message) => {
          if (typeof message !== 'string') return stream.send(message)
          lastRedFrame = message
          if (withholdRedFrames) heldRedFrames.push(message)
          else stream.send(message)
        })
      })
      const releaseRedFrames = () => {
        withholdRedFrames = false
        for (const frame of heldRedFrames.splice(0)) redStreams[redStreams.length - 1]!.send(frame)
      }
      await redPage.goto('/')
      await expect(redPage.locator('[data-canvas-ready="true"]')).toBeVisible()
      await redPage.getByTestId('create-invite-btn').click()
      await redPage.waitForURL(/\/invite\/[a-z0-9]+/)
      // Black's first snapshots arrive without a state version, as from a server that does not send one.
      let stripRevision = true
      const withoutRevision = (text: string) => {
        const data = JSON.parse(text)
        if (data.snapshot) delete data.snapshot.state_version
        delete data.state_version
        return data
      }
      await blkPage.route(
        /\/api\/v1\/invitations\/[^/]+\/join$/,
        async (route) => {
          const response = await route.fetch()
          await route.fulfill({ response, json: withoutRevision(await response.text()) })
        },
        { times: 1 },
      )
      // Its fallback snapshot reads too, so the assertion does not depend on how soon the stream opens.
      await blkPage.route(/\/api\/v1\/matches\/[^/?]+(\?.*)?$/, async (route) => {
        if (route.request().method() !== 'GET' || !stripRevision) return route.fallback()
        const response = await route.fetch()
        await route.fulfill({ response, json: withoutRevision(await response.text()) })
      })
      await blkPage.routeWebSocket(/\/api\/v1\/matches\/[^/]+\/ws$/, (stream) => {
        const server = stream.connectToServer()
        server.onMessage((message) =>
          stream.send(stripRevision && typeof message === 'string' ? JSON.stringify(withoutRevision(message)) : message),
        )
      })
      await blkPage.goto(new URL(redPage.url()).pathname)
      await expect(blkPage.getByTestId('match-status-badge')).toContainText('You play black')
      await expect(redPage.getByTestId('match-status-badge')).toContainText('You play red', { timeout: 6_000 })
      for (const page of [redPage, blkPage]) await page.getByTestId('privacy-consent-card').locator('summary').click()
      // Without a revision the effective visibility is not confirmed; the first versioned update confirms it.
      await expect(blkPage.getByTestId('privacy-effective')).toContainText('Not confirmed')
      stripRevision = false
      const saved = (page: typeof redPage) =>
        page.waitForResponse((r) => r.url().endsWith('/privacy') && r.request().method() === 'POST')

      /** Holds Red's next snapshot read: it is fetched now, so it carries the state now, and delivered on release. */
      const holdRedRead = async () => {
        let release: () => void = () => {}
        const gate = new Promise<void>((resolve) => (release = resolve))
        let fetched: () => void = () => {}
        const captured = new Promise<void>((resolve) => (fetched = resolve))
        await redPage.route(
          /\/api\/v1\/matches\/[^/?]+$/,
          async (route) => {
            if (route.request().method() !== 'GET') return route.fallback()
            const response = await route.fetch()
            fetched()
            await gate
            await route.fulfill({ response })
          },
          { times: 1 },
        )
        return { captured, release: () => release() }
      }
      const blackChooses = async (visibility: string) => {
        const save = saved(blkPage)
        await blkPage.getByTestId('privacy-visibility').selectOption(visibility)
        expect((await save).status()).toBe(200)
      }

      // Both choose public: the match is public.
      let save = saved(redPage)
      await redPage.getByTestId('privacy-visibility').selectOption('public')
      expect((await save).status()).toBe(200)
      await expect(blkPage.getByTestId('privacy-effective')).toContainText('Private')
      await blackChooses('public')
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Public')

      // Red's read-back is captured while public; Black withdraws; the older public reply arrives last.
      let held = await holdRedRead()
      save = saved(redPage)
      await redPage.getByLabel('Allow internal model training').check()
      expect((await save).status()).toBe(200)
      await held.captured
      await blackChooses('private')
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Private')
      held.release()
      await expect(redPage.getByTestId('privacy-visibility')).toBeEnabled()
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Private')

      // The other direction: a private reply captured before Black shares again arrives after the public update.
      held = await holdRedRead()
      save = saved(redPage)
      await redPage.getByLabel('Allow internal model training').uncheck()
      expect((await save).status()).toBe(200)
      await held.captured
      await blackChooses('public')
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Public')
      held.release()
      await expect(redPage.getByTestId('privacy-visibility')).toBeEnabled()
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Public')

      // Red's session ends on the server while the page still holds it; when the stream drops, the room asks the
      // server, learns the session has ended, and stops instead of reconnecting.
      // A save's acknowledgment alone leaves the sharing summary unconfirmed; a read below the version it wrote does not
      // confirm it, and a snapshot at that version does.
      withholdRedFrames = true
      await redPage.route(
        /\/api\/v1\/matches\/[^/?]+$/,
        async (route) => {
          if (route.request().method() !== 'GET') return route.fallback()
          const response = await route.fetch()
          const body = await response.json()
          body.state_version -= 1
          await route.fulfill({ response, json: body })
        },
        { times: 1 },
      )
      save = saved(redPage)
      await redPage.getByLabel('Allow published research analysis').check()
      expect((await save).status()).toBe(200)
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Not confirmed')
      await expect(redPage.getByTestId('privacy-visibility')).toBeEnabled()
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Not confirmed')
      releaseRedFrames()
      await expect(redPage.getByTestId('privacy-effective')).toContainText('Public')

      // A successful command reply that lacks a clock is not installed: the room says so and reads the match again.
      // Red's stream withholds the move, so Red can catch up only through that read.
      withholdRedFrames = true
      await redPage.route(
        /\/commands$/,
        async (route) => {
          const response = await route.fetch()
          const body = await response.json()
          delete body.red_remaining_ms
          await route.fulfill({ response, json: body })
        },
        { times: 1 },
      )
      await playBoardSquares(redPage, 'h2', 'e2')
      await expect(redPage.getByTestId('match-error')).toContainText('could not be read')
      await expect(blkPage.getByTestId('match-moves-card')).toContainText('C2=5')
      await expect(redPage.getByTestId('match-moves-card')).toContainText('C2=5')
      await expect(redPage.locator('body')).not.toContainText('NaN')
      withholdRedFrames = false
      heldRedFrames.length = 0

      // The same state arriving again changes nothing: Red's move reaches Red's stream first, the stream drops, and the
      // late reply to the move does not wind Black's running clock back.
      await playBoardSquares(blkPage, 'h9', 'g7')
      await expect(redPage.getByTestId('match-moves-card')).toContainText('H8+7')
      let releaseReply: () => void = () => {}
      const replyGate = new Promise<void>((resolve) => (releaseReply = resolve))
      let heldBlackMs = -1
      await redPage.route(
        /\/commands$/,
        async (route) => {
          const response = await route.fetch()
          const body = await response.json()
          heldBlackMs = body.black_remaining_ms
          await replyGate
          await route.fulfill({ response, json: body })
        },
        { times: 1 },
      )
      // The stream stays closed until the reply is handled: the reconnect's reads fail meanwhile, as offline.
      let blockRecovery = true
      await redPage.route(/\/api\/v1\/matches\/[^/?]+\?since_seq=\d+$/, (route) =>
        blockRecovery ? route.abort() : route.fallback(),
      )
      await playBoardSquares(redPage, 'b0', 'c2')
      await expect(redPage.getByTestId('match-moves-card')).toContainText('2.H8+7')
      const streamsBefore = redStreams.length
      await redStreams[streamsBefore - 1]!.close()
      const clockSeconds = async (side: 'red' | 'black') => {
        const [m, sec] = (await redPage.getByTestId(`clock-${side}`).innerText()).trim().split(':').map(Number)
        return m! * 60 + sec!
      }
      const seconds = () => clockSeconds('black')
      // Black's clock runs until it shows less than the reading in the held reply, so replaying that reading would show.
      await expect.poll(() => heldBlackMs).toBeGreaterThan(0)
      await expect.poll(seconds, { timeout: 10_000 }).toBeLessThan(Math.floor(heldBlackMs / 1000))
      const beforeReply = await seconds()
      const replied = redPage.waitForEvent('requestfinished', { predicate: (r) => r.url().endsWith('/commands') })
      releaseReply()
      await replied
      // Sampled across the reply's handling, Black's clock never shows more time than before the reply.
      for (let i = 0; i < 10; i++) {
        expect(await seconds()).toBeLessThanOrEqual(beforeReply)
        await redPage.waitForTimeout(100)
      }
      expect(redStreams.length).toBe(streamsBefore)
      blockRecovery = false
      await expect.poll(() => redStreams.length).toBe(streamsBefore + 1)

      // A reading of the same state may lower the running clock, and never moves the stopped one.
      await expect.poll(() => lastRedFrame.length).toBeGreaterThan(0)
      const stoppedBefore = await clockSeconds('red')
      const running = await seconds()
      const sameState = JSON.parse(lastRedFrame)
      sameState.black_remaining_ms = (running - 30) * 1000
      sameState.red_remaining_ms += 90_000
      redStreams[redStreams.length - 1]!.send(JSON.stringify(sameState))
      await expect.poll(seconds).toBeLessThanOrEqual(running - 29)
      expect(await clockSeconds('red')).toBe(stoppedBefore)

      // A save is answered only after the room stopped: its outcome is not shown, and nothing is read back.
      let releaseSave: () => void = () => {}
      const saveGate = new Promise<void>((resolve) => (releaseSave = resolve))
      let saveApplied: () => void = () => {}
      const applied = new Promise<void>((resolve) => (saveApplied = resolve))
      let saveStatus = 0
      await redPage.route(
        /\/privacy$/,
        async (route) => {
          const response = await route.fetch()
          saveStatus = response.status()
          saveApplied()
          await saveGate
          await route.fulfill({ response })
        },
        { times: 1 },
      )
      await redPage.getByLabel('Allow open dataset release').check()
      await applied
      expect(saveStatus).toBe(200)
      await expect.poll(() => redStreams.length).toBeGreaterThan(0)
      await redContext.clearCookies()
      const opened = redStreams.length
      await redStreams[opened - 1]!.close()
      await expect(redPage.getByTestId('match-error')).toContainText('session has ended')
      let readsAfterStop = 0
      redPage.on('request', (request) => {
        if (request.method() === 'GET' && /\/api\/v1\/matches\/[^/?]+$/.test(request.url())) readsAfterStop++
      })
      // The page's own leave guard holds while the save or its read-back is pending, and lets go once it is handled.
      const leavingGuarded = () =>
        redPage.evaluate(() => {
          const leaving = new Event('beforeunload', { cancelable: true })
          window.dispatchEvent(leaving)
          return leaving.defaultPrevented
        })
      expect(await leavingGuarded()).toBe(true)
      releaseSave()
      await expect.poll(leavingGuarded).toBe(false)
      expect(readsAfterStop).toBe(0)
      await expect(redPage.getByTestId('privacy-status')).not.toContainText('Open dataset release')
      await expect(redPage.getByTestId('match-error')).toContainText('session has ended')
      expect(redStreams.length).toBe(opened)
      await expect(redPage.getByTestId('privacy-visibility')).toBeDisabled()
    } finally {
      await redContext.close()
      await blkContext.close()
    }
  })

  test('12. A waiting room confirms its session before reopening its stream, and stops when it is gone', async ({
    browser,
  }) => {
    const context = await browser.newContext({ viewport: { width: 1200, height: 840 } })
    try {
      const page = await context.newPage()
      const events: string[] = []
      const streams: WebSocketRoute[] = []
      await page.routeWebSocket(/\/api\/v1\/matches\/[^/]+\/ws$/, (stream) => {
        streams.push(stream)
        events.push('stream')
        stream.connectToServer()
      })
      page.on('request', (request) => {
        if (request.url().includes('/api/v1/auth/session?reissue=false')) events.push('probe')
      })
      await page.goto('/')
      await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
      await page.getByTestId('create-invite-btn').click()
      await page.waitForURL(/\/invite\/[a-z0-9]+/)
      await expect(page.getByTestId('match-status-badge')).toContainText('Waiting for friend')
      await expect.poll(() => streams.length).toBe(1)

      // The stream drops while a waiting-room poll's reply is held; once the reply lands, the room asks the server,
      // read-only, before it opens a new stream.
      let release: () => void = () => {}
      const gate = new Promise<void>((resolve) => (release = resolve))
      let fetched: () => void = () => {}
      const held = new Promise<void>((resolve) => (fetched = resolve))
      // Every poll answers with an unreadable snapshot meanwhile: nothing is installed, and the stream still reopens.
      let breakPolls = true
      let firstPoll = true
      await page.route(/\/api\/v1\/matches\/[^/?]+\?since_seq=\d+$/, async (route) => {
        if (!breakPolls) return route.fallback()
        if (firstPoll) {
          firstPoll = false
          fetched()
          await gate
        }
        await route.fulfill({ status: 200, json: { match_id: 'another-match', status: 'finished' } })
      })
      // The probe's answer is held too: no new stream opens until the server has said the session stands.
      let answer: () => void = () => {}
      const answerGate = new Promise<void>((resolve) => (answer = resolve))
      let asked: () => void = () => {}
      const probed = new Promise<void>((resolve) => (asked = resolve))
      await page.route(
        /\/api\/v1\/auth\/session\?reissue=false$/,
        async (route) => {
          const response = await route.fetch()
          asked()
          await answerGate
          await route.fulfill({ response })
        },
        { times: 1 },
      )
      await held
      events.length = 0
      await streams[0]!.close()
      release()
      await probed
      await page.waitForTimeout(1_000)
      expect(streams.length).toBe(1)
      answer()
      await expect.poll(() => streams.length).toBe(2)
      expect(events.indexOf('probe')).toBeGreaterThanOrEqual(0)
      expect(events.indexOf('probe')).toBeLessThan(events.indexOf('stream'))
      await expect(page.getByTestId('match-status-badge')).toContainText('Waiting for friend')
      breakPolls = false

      // The session ends while the stream stays open: the next poll of the private room is refused, the room asks the
      // server, and it stops and closes its stream.
      let closedByPage = false
      streams[1]!.onClose(() => {
        closedByPage = true
      })
      await context.clearCookies()
      await expect(page.getByTestId('match-error')).toContainText('session has ended')
      await expect.poll(() => closedByPage).toBe(true)
      await page.waitForTimeout(1_500)
      expect(streams.length).toBe(2)
    } finally {
      await context.close()
    }
  })

  test('5. Archive replay: locale notation, stable move selection, every ply renders, and an overlaid notice', async ({
    page,
  }) => {
    const pageErrors: string[] = []
    page.on('pageerror', (e) => pageErrors.push(e.message))
    page.on('console', (m) => {
      if (m.type() === 'error') pageErrors.push(m.text())
    })
    await page.setViewportSize({ width: 1000, height: 640 })
    await page.goto('/archive/g/sample-game-central-cannon-001')

    await expect(page).toHaveTitle('Jade')
    await expect(page.getByTestId('brand-home-link')).toHaveText('Jade')

    // English reads WXF by default; a Chinese locale reads Chinese notation.
    const moves = page.getByTestId('archive-move')
    await expect(moves.first()).toHaveText('C2=5')
    await page.getByTestId('settings-toggle').click()
    await page.getByTestId('pref-locale').selectOption('zh-Hans-CN')
    await expect(moves.first()).toHaveText('炮二平五')
    await page.getByTestId('pref-locale').selectOption('en-US')
    await expect(moves.first()).toHaveText('C2=5')
    await page.keyboard.press('Escape')

    // Selecting a move jumps to that ply and stays there.
    await moves.nth(2).click()
    await expect(page.getByTestId('archive-ply')).toContainText('3 /')
    await page.waitForTimeout(1500)
    await expect(page.getByTestId('archive-ply')).toContainText('3 /')
    await expect(moves.nth(2)).toHaveAttribute('aria-current', 'step')

    // Every recorded ply, the last one included, renders without unmounting the page.
    const count = await moves.count()
    for (let i = 0; i < count; i++) {
      await moves.nth(i).click()
      await expect(page.getByTestId('archive-ply')).toContainText(`${i + 1} / ${count}`)
    }
    await expect(page.getByTestId('xiangqi-board')).toBeVisible()
    await expect(page.getByTestId('archive-error')).toHaveCount(0)

    // The sample notice overlays the page, so dismissing it moves nothing.
    const banner = page.getByTestId('archive-provenance-banner')
    await expect(banner).toHaveCSS('position', 'fixed')
    const boardTop = (await page.getByTestId('xiangqi-board').boundingBox())?.y
    await page.getByTestId('archive-provenance-dismiss').click()
    await expect(banner).toHaveCount(0)
    expect((await page.getByTestId('xiangqi-board').boundingBox())?.y).toBe(boardTop)

    expect(pageErrors).toEqual([])
  })
})
