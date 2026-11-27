export interface LocalSavedGame {
  id: string
  initialFen: string
  movesUci: string[]
  updatedAtMs: number
}

/**
 * One retained variation per replay source, kept on this device only. A draft derived from a private
 * match carries the guest identity that made it (`ownerScope`) and is never shown to another guest.
 * The draft serves one tab: the latest save wins, so two tabs editing one source can overwrite each other.
 */
export interface ReplayDraft {
  schemaVersion: 1
  draftId: string
  sourceKind: 'local' | 'match' | 'archive'
  sourceId: string
  sourceRevision: string
  ownerScope: string | null
  rootFen: string
  rulesProfile: string
  recordedMovesDigest: string
  variationOriginPly: number
  variationMovesUci: string[]
  cursor: number
  activeMode: 'recorded' | 'variation'
  updatedAt: number
}

export type DraftReadResult =
  | { status: 'found'; draft: ReplayDraft }
  | { status: 'none' }
  | { status: 'unavailable'; reason: string }

export type DraftWriteResult = { ok: true } | { ok: false; reason: string }

const DB_NAME = 'jade_offline_v1'
// Version 2 adds the replay draft store; version 1's local games stay where they are.
const DB_VERSION = 2
const STORE_NAME = 'local_games'
const DRAFT_STORE_NAME = 'replay_drafts'

const BLOCKED_MESSAGE = 'Close other Jade tabs so this device can update its saved games'
/** How long a draft caller waits for the store; an open can queue behind one already blocked. */
const DRAFT_OPEN_WAIT_MS = 3000

/**
 * Opens the device store. Another tab holding the older version blocks the upgrade: local games wait
 * for it as before, while draft callers stop waiting after a bounded time with a close-other-tabs message.
 */
function openOfflineDb(onBlocked: 'wait' | 'fail' = 'wait'): Promise<IDBDatabase> {
  let abandoned = false
  let deadline: ReturnType<typeof setTimeout> | undefined
  const open = new Promise<IDBDatabase>((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: 'id' })
      }
      if (!db.objectStoreNames.contains(DRAFT_STORE_NAME)) {
        db.createObjectStore(DRAFT_STORE_NAME, { keyPath: 'draftId' })
      }
    }
    req.onsuccess = () => {
      clearTimeout(deadline)
      // A connection that arrives after its caller gave up is closed, so it holds up no later upgrade.
      if (abandoned) req.result.close()
      else resolve(req.result)
    }
    req.onerror = () => {
      clearTimeout(deadline)
      reject(req.error)
    }
    if (onBlocked === 'fail') {
      req.onblocked = () => {
        abandoned = true
        clearTimeout(deadline)
        reject(new Error(BLOCKED_MESSAGE))
      }
    }
  })
  if (onBlocked === 'wait') return open
  return Promise.race([
    open,
    new Promise<never>((_, reject) => {
      deadline = setTimeout(() => {
        abandoned = true
        reject(new Error(BLOCKED_MESSAGE))
      }, DRAFT_OPEN_WAIT_MS)
    }),
  ])
}

export async function saveOfflineGame(record: LocalSavedGame): Promise<void> {
  if (typeof indexedDB === 'undefined') return
  try {
    const db = await openOfflineDb()
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readwrite')
      tx.objectStore(STORE_NAME).put(record)
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error)
    })
  } catch {
    // Ignore private-browsing IndexedDB restrictions
  }
}

export async function loadOfflineGame(id = 'active_board'): Promise<LocalSavedGame | null> {
  if (typeof indexedDB === 'undefined') return null
  try {
    const db = await openOfflineDb()
    return await new Promise<LocalSavedGame | null>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, 'readonly')
      const req = tx.objectStore(STORE_NAME).get(id)
      req.onsuccess = () => resolve((req.result as LocalSavedGame | undefined) ?? null)
      req.onerror = () => reject(req.error)
    })
  } catch {
    return null
  }
}

/** The key of a source's single retained draft, scoped to the guest identity when one owns it. */
export function replayDraftId(
  sourceKind: ReplayDraft['sourceKind'],
  sourceId: string,
  ownerScope: string | null,
): string {
  return [sourceKind, sourceId, ownerScope ?? ''].join(':')
}

/** Replaces this source's draft; reports success only after the transaction completes. */
export async function saveReplayDraft(draft: ReplayDraft): Promise<DraftWriteResult> {
  if (typeof indexedDB === 'undefined') return { ok: false, reason: 'Browser storage is not available' }
  try {
    const db = await openOfflineDb('fail')
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(DRAFT_STORE_NAME, 'readwrite')
      tx.objectStore(DRAFT_STORE_NAME).put(draft)
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error)
      tx.onabort = () => reject(tx.error)
    })
    return { ok: true }
  } catch (error) {
    return { ok: false, reason: error instanceof Error ? error.message : String(error) }
  }
}

/** Removes a source's draft; reports success only after the transaction completes. */
export async function deleteReplayDraft(draftId: string): Promise<DraftWriteResult> {
  if (typeof indexedDB === 'undefined') return { ok: false, reason: 'Browser storage is not available' }
  try {
    const db = await openOfflineDb('fail')
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(DRAFT_STORE_NAME, 'readwrite')
      tx.objectStore(DRAFT_STORE_NAME).delete(draftId)
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error)
      tx.onabort = () => reject(tx.error)
    })
    return { ok: true }
  } catch (error) {
    return { ok: false, reason: error instanceof Error ? error.message : String(error) }
  }
}

export async function loadReplayDraft(draftId: string): Promise<DraftReadResult> {
  if (typeof indexedDB === 'undefined') return { status: 'unavailable', reason: 'Browser storage is not available' }
  try {
    const db = await openOfflineDb('fail')
    const draft = await new Promise<ReplayDraft | undefined>((resolve, reject) => {
      const req = db.transaction(DRAFT_STORE_NAME, 'readonly').objectStore(DRAFT_STORE_NAME).get(draftId)
      req.onsuccess = () => resolve(req.result as ReplayDraft | undefined)
      req.onerror = () => reject(req.error)
    })
    return draft ? { status: 'found', draft } : { status: 'none' }
  } catch (error) {
    return { status: 'unavailable', reason: error instanceof Error ? error.message : String(error) }
  }
}
