export interface LocalSavedGame {
  id: string
  initialFen: string
  movesUci: string[]
  updatedAtMs: number
}

const DB_NAME = 'jade_offline_v1'
const STORE_NAME = 'local_games'

function openOfflineDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: 'id' })
      }
    }
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error)
  })
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
