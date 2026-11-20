import { resolve } from 'node:path'
import type { Page } from '@playwright/test'
import { expect, test } from '@playwright/test'

const VISUAL_REF_DIR = resolve(process.cwd(), '../../docs/visual-reference')

async function playBoardSquares(page: Page, fromSq: string, toSq: string) {
  await expect(page.locator('[data-canvas-ready="true"]')).toBeVisible()
  await page.locator(`[data-square="${fromSq}"]`).dispatchEvent('click')
  await expect(page.locator(`[data-square="${fromSq}"]`)).toHaveAttribute('aria-label', /selected/)
  await page.locator(`[data-square="${toSq}"]`).dispatchEvent('click')
}

test.describe('Jade Slice A Web Showcase Journeys', () => {
  test('1. Home board, tactile move play, preference switching without state loss, and local engine worker', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 860 })
    await page.goto('/')

    await expect(page.getByTestId('xiangqi-board')).toBeVisible()
    await expect(page.getByTestId('board-status-strip')).toContainText('Red to move')

    // Play Cannon 2 to 5 (`h2` -> `e2`) and verify Chinese notation + turn switch
    await playBoardSquares(page, 'h2', 'e2')

    await expect(page.getByTestId('move-list')).toContainText('炮二平五')
    await expect(page.getByTestId('board-status-strip')).toContainText('Black to move')

    // Capture desktop light-paper visual reference plate
    await page.waitForTimeout(200)
    await page.screenshot({
      path: resolve(VISUAL_REF_DIR, 'web-home-desktop-paper.png'),
      fullPage: false,
    })

    // Switch locale (`zh-Hant-HK`), material (`boxwood`), notation (`uci`), and theme (`Dark`) WITHOUT losing active move!
    await page.getByTestId('pref-locale').selectOption('zh-Hant-HK')
    await page.getByTestId('pref-material').selectOption('boxwood')
    await page.getByTestId('pref-notation').selectOption('uci')
    await page.getByTestId('pref-theme').click()

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

      // Red plays h2 -> e2; Black sees 炮二平五!
      await playBoardSquares(redPage, 'h2', 'e2')
      await expect(redPage.getByTestId('match-move-list')).toContainText('炮二平五')
      await expect(blkPage.getByTestId('match-move-list')).toContainText('炮二平五', {
        timeout: 6_000,
      })

      // Black plays h9 -> g7; Red sees 马8进7!
      await playBoardSquares(blkPage, 'h9', 'g7')
      await expect(redPage.getByTestId('match-move-list')).toContainText('马8进7', {
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
      await expect(blkPage.getByTestId('match-move-list')).toContainText('马二进三', {
        timeout: 6_000,
      })
      await playBoardSquares(blkPage, 'i9', 'h9')
      await expect(redPage.getByTestId('match-move-list')).toContainText('车9平8', {
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
      await expect(page.getByTestId('continuation-table')).toContainText('炮二平五')
      await expect(page.getByTestId('explorer-source-label')).toContainText('Sample Archive')

      // Verify both disambiguated `林明` records appear distinctly in SSR HTML
      const playerItems = page.locator('[data-testid="player-list"] li')
      await expect(playerItems).toHaveCount(4)
      await expect(page.getByTestId('player-list')).toContainText('Lin, Ming (Club A)')
      await expect(page.getByTestId('player-list')).toContainText('Lin, Ming (Youth Open)')

      await page.screenshot({
        path: resolve(VISUAL_REF_DIR, 'web-archive-ssr-nojs.png'),
        fullPage: false,
      })

      // Navigate to player profile with JS disabled
      await page.goto('/players/sample-player-001')
      await expect(page.getByTestId('player-profile-page')).toContainText('赵汝权')
      await expect(page.getByTestId('player-profile-page')).toContainText('Chiu, Yu Kuen')
    } finally {
      await noJsContext.close()
    }
  })
})
