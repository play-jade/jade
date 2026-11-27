import type { ArchiveGameSummaryPage } from '@play-jade/protocol'

/** The largest page position an archive provider accepts (a 32-bit count). */
const MAX_OFFSET = 2 ** 32 - 1

/** A page position from a `?offset=` query: a whole number a provider accepts, or 0 for anything else. */
export function queryOffset(raw: unknown): number {
  const offset = typeof raw === 'string' && /^\d+$/.test(raw) ? Number(raw) : 0
  return Number.isSafeInteger(offset) && offset <= MAX_OFFSET ? offset : 0
}

/**
 * Where the previous and next pages of a game list start, or null at either end. A position past the last game
 * is reported as such, with the start of the last populated page to go to instead.
 */
export function pageLinks(page: ArchiveGameSummaryPage) {
  // A page without a positive size cannot be paged through; the provider boundary already refuses one.
  if (page.limit <= 0) return null
  if (page.games.length === 0 && page.total > 0) {
    const lastPage = Math.floor((page.total - 1) / page.limit) * page.limit
    return { pastEnd: true as const, lastPage, total: page.total }
  }
  return {
    pastEnd: false as const,
    previous: page.offset > 0 ? Math.max(0, Math.min(page.offset, page.total) - page.limit) : null,
    next: page.offset + page.games.length < page.total ? page.offset + page.limit : null,
    first: page.offset + 1,
    last: page.offset + page.games.length,
    total: page.total,
  }
}
