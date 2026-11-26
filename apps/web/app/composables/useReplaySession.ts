import { computed, ref, unref, watch, type Ref } from 'vue'
import { applyCoreMove, notateMoves, type ApplyMoveResult } from '~/utils/core-wasm'
import { loadReplayDraft, replayDraftId, saveReplayDraft, type ReplayDraft } from '~/utils/offline-store'

/** What a replay shows: a local game, a finished match, or an archive game at a published release. */
export interface ReplaySource {
  kind: ReplayDraft['sourceKind']
  id: string
  revision: string
}

export interface Variation {
  originPly: number
  movesUci: string[]
}

export type DraftSaveState =
  | { state: 'none' }
  | { state: 'pending' }
  | { state: 'saved' }
  | { state: 'unsaved'; reason: string }

export type TryMoveResult =
  | { kind: 'followed'; applied: ApplyMoveResult }
  | { kind: 'variation'; applied: ApplyMoveResult }
  | { kind: 'extend'; uci: string; applied: ApplyMoveResult }
  | { kind: 'rejected'; message: string }

export interface ReplaySessionOptions {
  source: Ref<ReplaySource | null>
  rootFen: Ref<string>
  rulesProfile: Ref<string>
  /** The immutable recorded line; the session never edits it. */
  recordedMovesUci: Ref<string[]>
  /** The guest identity that owns a private match's drafts, or null for public sources. */
  ownerScope?: Ref<string | null>
  /** The reader's script for the core's move text. */
  traditionalGlyphs?: Ref<boolean>
  /**
   * A move at the end of the recorded line returns `extend` for the caller to append (local play);
   * otherwise every off-record move, including one at the end, starts a variation.
   */
  extendAtEnd?: boolean
  /** Retains one variation per source on this device while true. */
  persist?: boolean | Ref<boolean>
  /** Why a variation cannot be kept while persistence is off; shown with the export action. */
  unpersistedReason?: Ref<string | null>
}

/**
 * The position a `?ply=` link names: a non-empty decimal integer, or null when the link names none
 * (`?ply`, `?ply=`, or anything else).
 */
export function parsePlyQuery(raw: unknown): number | null {
  if (typeof raw !== 'string' || !/^\d+$/.test(raw)) return null
  const ply = Number(raw)
  return Number.isSafeInteger(ply) ? ply : null
}

/** A canonical UCI move, the only spelling the engine worker and stored drafts accept. */
const UCI_MOVE = /^[a-i][0-9][a-i][0-9]$/

/**
 * SHA-256 over the root and recorded moves, so a draft only reapplies to the record it was made on.
 * Null where Web Crypto is unavailable (a non-secure origin), which disables retention but not export.
 */
export async function recordedMovesDigest(rootFen: string, movesUci: string[]): Promise<string | null> {
  const subtle = globalThis.crypto?.subtle
  if (!subtle) return null
  const bytes = new TextEncoder().encode(`${rootFen}\n${movesUci.join(' ')}`)
  const hash = await subtle.digest('SHA-256', bytes)
  return Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, '0')).join('')
}

/**
 * Per draft key, the tail of this tab's pending writes. Writes to one draft run one at a time in edit
 * order across every replay page in the tab, so a page left with a write pending never stores an older
 * line over one saved after the page was opened again; writes to other drafts do not wait.
 */
const writeChains = new Map<string, Promise<boolean>>()

/**
 * Replay state shared by local play, match replay, and archive replay: one recorded line, one optional
 * variation, and the active path the board, notation, continuations, and engine all read.
 */
export function useReplaySession(options: ReplaySessionOptions) {
  const ownerScope = options.ownerScope ?? ref(null)
  const persistEnabled = () => unref(options.persist) === true
  const cursor = ref(0)
  const variation = ref<Variation | null>(null)
  /** The variation set aside by Return to game, which Resume brings back. */
  const parked = ref<Variation | null>(null)
  const saveState = ref<DraftSaveState>({ state: 'none' })
  /** Why a stored draft could not be applied to this record; the stored draft stays untouched. */
  const draftNotice = ref<string | null>(null)
  const feedback = ref<string | null>(null)
  /** Counts variation edits and navigation separately, so a late restore never overrides newer input. */
  let edits = 0
  let navigations = 0
  let writes = 0
  /** A position already handed back for extension; a second move validated against it is refused. */
  let extendedFrom: string | null = null
  /** Advances when the source, record, or guest changes; work begun in an earlier session is dropped. */
  let generation = 0

  const activeMode = computed<'recorded' | 'variation'>(() => (variation.value ? 'variation' : 'recorded'))

  const activePath = computed<string[]>(() => {
    const recorded = options.recordedMovesUci.value
    if (variation.value) {
      return [...recorded.slice(0, variation.value.originPly), ...variation.value.movesUci]
    }
    return recorded.slice(0, cursor.value)
  })

  /** State identity for analysis: the source, the root, and the full move path to the position. */
  const stateId = computed(() => {
    const source = options.source.value
    return [source?.kind ?? 'none', source?.id ?? '', options.rootFen.value, activePath.value.join(' ')].join('|')
  })

  watch(stateId, (next) => {
    if (next !== extendedFrom) extendedFrom = null
  })

  /** True when a new variation would replace the one retained for this source. */
  const willReplaceDraft = computed(() => activeMode.value === 'recorded' && parked.value !== null)

  function selectPly(ply: number) {
    navigations++
    feedback.value = null
    const leftVariation = parkVariation()
    cursor.value = Math.max(0, Math.min(options.recordedMovesUci.value.length, ply))
    // The draft records the selected position, so it is stored only once the cursor is there.
    if (leftVariation) void persist()
  }

  async function tryMove(token: string): Promise<TryMoveResult> {
    feedback.value = null
    const validatedAt = stateId.value
    const startedIn = generation
    let applied: ApplyMoveResult
    try {
      applied = await applyCoreMove({
        fen: options.rootFen.value,
        movesUci: activePath.value,
        token,
        traditionalGlyphs: options.traditionalGlyphs?.value ?? false,
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      feedback.value = message
      return { kind: 'rejected', message }
    }
    // The core judged the move against one position; it applies only if that is still the position shown,
    // in the same session, so a move begun before a guest change never becomes the new guest's line.
    if (startedIn !== generation || stateId.value !== validatedAt || extendedFrom === validatedAt) {
      const message = 'The position changed before the move was checked; try it again.'
      feedback.value = message
      return { kind: 'rejected', message }
    }

    const uci = applied.move_uci
    const recorded = options.recordedMovesUci.value
    if (!variation.value && recorded[cursor.value] === uci) {
      // Following the record moves along it like Next; it changes no variation.
      navigations++
      cursor.value += 1
      return { kind: 'followed', applied }
    }
    edits++
    if (variation.value) {
      variation.value = { ...variation.value, movesUci: [...variation.value.movesUci, uci] }
    } else if (options.extendAtEnd && cursor.value === recorded.length) {
      extendedFrom = validatedAt
      return { kind: 'extend', uci, applied }
    } else {
      // A new variation replaces the retained one, which stays in memory until the replacement is stored.
      const superseded = parked.value
      variation.value = { originPly: cursor.value, movesUci: [uci] }
      if ((await persist()) && parked.value === superseded) parked.value = null
      return { kind: 'variation', applied }
    }
    await persist()
    return { kind: 'variation', applied }
  }

  /** Takes back the variation's last move; taking back its only move leaves the variation. */
  function undoVariationMove() {
    if (!variation.value) return
    edits++
    if (variation.value.movesUci.length > 1) {
      variation.value = { ...variation.value, movesUci: variation.value.movesUci.slice(0, -1) }
      void persist()
    } else {
      cursor.value = variation.value.originPly
      variation.value = null
    }
  }

  /** Sets the active variation aside at its origin on the recorded line; false when none is active. */
  function parkVariation(): boolean {
    if (!variation.value) return false
    edits++
    parked.value = variation.value
    cursor.value = variation.value.originPly
    variation.value = null
    return true
  }

  /** Leaves the variation and returns to its origin on the recorded line, keeping it to resume. */
  function returnToRecordedLine() {
    if (parkVariation()) void persist()
  }

  function resumeDraft() {
    if (!parked.value) return
    edits++
    variation.value = parked.value
    parked.value = null
    void persist()
  }

  /** Drops the active or retained variation without touching the recorded line. */
  function discardVariation() {
    edits++
    variation.value = null
    parked.value = null
  }

  /**
   * The current line, its owner, and its key as plain data, captured at once; arrays are copied out of
   * Vue's reactive proxies. A later guest or edit change cannot alter a snapshot already taken.
   */
  function lineSnapshot() {
    const source = options.source.value
    const line = variation.value ?? parked.value
    if (!source || !line) return null
    const draft = {
      schemaVersion: 1 as const,
      draftId: replayDraftId(source.kind, source.id, ownerScope.value),
      sourceKind: source.kind,
      sourceId: source.id,
      sourceRevision: source.revision,
      ownerScope: ownerScope.value,
      rootFen: options.rootFen.value,
      rulesProfile: options.rulesProfile.value,
      variationOriginPly: line.originPly,
      variationMovesUci: [...line.movesUci],
      cursor: cursor.value,
      activeMode: activeMode.value,
      updatedAt: Date.now(),
    }
    return { draft, recorded: [...options.recordedMovesUci.value] }
  }

  type LineSnapshot = NonNullable<ReturnType<typeof lineSnapshot>>

  /** The current line as a plain record, with the digest of the record it was made on. */
  async function currentDraft(): Promise<
    (Omit<ReplayDraft, 'recordedMovesDigest'> & { recordedMovesDigest: string | null }) | null
  > {
    const snapshot = lineSnapshot()
    if (!snapshot) return null
    return { ...snapshot.draft, recordedMovesDigest: await recordedMovesDigest(snapshot.draft.rootFen, snapshot.recorded) }
  }

  /** Stores the current line after any earlier write; true when stored or the session does not persist. */
  function persist(): Promise<boolean> {
    if (!persistEnabled()) {
      const reason = options.unpersistedReason?.value
      if (reason && (variation.value || parked.value)) saveState.value = { state: 'unsaved', reason }
      return Promise.resolve(true)
    }
    const write = ++writes
    // The line, owner, and key are fixed when the edit is made; only the storage work waits its turn.
    const snapshot = lineSnapshot()
    if (!snapshot) {
      saveState.value = { state: 'none' }
      return Promise.resolve(true)
    }
    saveState.value = { state: 'pending' }
    const key = snapshot.draft.draftId
    const run = () => writeSnapshot(write, snapshot)
    const written = (writeChains.get(key) ?? Promise.resolve(true)).then(run, run)
    writeChains.set(key, written)
    const settle = () => {
      if (writeChains.get(key) === written) writeChains.delete(key)
    }
    void written.then(settle, settle)
    return written
  }

  async function writeSnapshot(write: number, snapshot: LineSnapshot): Promise<boolean> {
    // Only the latest write may report; an earlier one finishing late says nothing about the line shown.
    const report = (state: DraftSaveState) => {
      if (write === writes) saveState.value = state
    }
    const digest = await recordedMovesDigest(snapshot.draft.rootFen, snapshot.recorded)
    if (digest === null) {
      report({ state: 'unsaved', reason: 'This address cannot keep variations; export it instead' })
      return false
    }
    const result = await saveReplayDraft({ ...snapshot.draft, recordedMovesDigest: digest })
    report(result.ok ? { state: 'saved' } : { state: 'unsaved', reason: result.reason })
    return result.ok
  }

  /** Why a stored draft cannot be installed on this record, or null when it can. */
  async function draftProblem(draft: ReplayDraft, source: ReplaySource): Promise<string | null> {
    const recorded = options.recordedMovesUci.value
    if (draft.schemaVersion !== 1) return 'The saved variation uses a format this version does not read.'
    if (draft.sourceRevision !== source.revision) {
      return `The saved variation was made on revision ${draft.sourceRevision}; this record is ${source.revision}.`
    }
    if (draft.rulesProfile !== options.rulesProfile.value) {
      return `The saved variation follows ${draft.rulesProfile}, not ${options.rulesProfile.value}.`
    }
    const digest = await recordedMovesDigest(options.rootFen.value, [...recorded])
    if (digest === null) return 'This address cannot check the saved variation against the record.'
    if (draft.recordedMovesDigest !== digest || draft.rootFen !== options.rootFen.value) {
      return 'The saved variation was made on a different recorded line.'
    }
    const origin = draft.variationOriginPly
    const moves = draft.variationMovesUci
    if (
      !Number.isInteger(origin) ||
      origin < 0 ||
      origin > recorded.length ||
      !Number.isInteger(draft.cursor) ||
      draft.cursor < 0 ||
      draft.cursor > recorded.length ||
      (draft.activeMode !== 'variation' && draft.activeMode !== 'recorded') ||
      !Array.isArray(moves) ||
      moves.length === 0 ||
      !moves.every((m) => typeof m === 'string' && UCI_MOVE.test(m))
    ) {
      return 'The saved variation is incomplete.'
    }
    const replay = await notateMoves({
      fen: options.rootFen.value,
      movesUci: [...recorded.slice(0, origin), ...moves],
      wxf: false,
    }).catch(() => null)
    if (!replay || replay.invalid_ply !== null) return 'The saved variation does not replay under the rules.'
    return null
  }

  /**
   * Loads this source's retained draft and reapplies it only to the same record, revision, and rules.
   * With `keepPosition` (an explicit position link), the draft comes back resumable and the cursor stays.
   * A visitor's action during the read wins over the late restore.
   */
  async function restoreDraft(restore: { keepPosition?: boolean } = {}) {
    const source = options.source.value
    if (!persistEnabled() || !source) return
    const startedAt = edits
    const navigatedAt = navigations
    draftNotice.value = null
    // The key is fixed before the read; if the source or guest changes meanwhile, the row is never shown,
    // so one guest's private variation cannot appear under another.
    const draftId = replayDraftId(source.kind, source.id, ownerScope.value)
    const stillCurrent = () => {
      const now = options.source.value
      return now !== null && replayDraftId(now.kind, now.id, ownerScope.value) === draftId
    }
    const read = await loadReplayDraft(draftId)
    if (!stillCurrent()) return
    if (read.status === 'unavailable') {
      saveState.value = { state: 'unsaved', reason: read.reason }
      return
    }
    if (read.status === 'none') return
    const problem = await draftProblem(read.draft, source)
    // An edit made during the read is newer than the stored line, so the late restore installs nothing.
    if (edits !== startedAt || !stillCurrent()) return
    if (problem) {
      draftNotice.value = problem
      return
    }
    const draft = read.draft
    const line = { originPly: draft.variationOriginPly, movesUci: [...draft.variationMovesUci] }
    // A position the visitor asked for (a link, or navigation during the read) stays; the line resumes.
    const keepPosition = restore.keepPosition || navigations !== navigatedAt
    if (!keepPosition) cursor.value = draft.cursor
    if (draft.activeMode === 'variation' && !keepPosition) {
      variation.value = line
    } else {
      parked.value = line
    }
    saveState.value = { state: 'saved' }
  }

  /** The current variation as a portable JSON file, for when this device cannot keep it. */
  async function exportDraft(): Promise<Blob | null> {
    const draft = await currentDraft()
    return draft ? new Blob([JSON.stringify(draft, null, 2)], { type: 'application/json' }) : null
  }

  // A different source, record, or guest starts a fresh session, so no line follows a guest change.
  watch(
    [() => options.source.value?.kind, () => options.source.value?.id, () => options.source.value?.revision, ownerScope],
    () => {
      generation++
      edits++
      // A write still queued for the old session reports nothing to the new one.
      writes++
      extendedFrom = null
      variation.value = null
      parked.value = null
      draftNotice.value = null
      saveState.value = { state: 'none' }
    },
    // Synchronous, so no edit in the same turn as a guest change can read the old guest's line.
    { flush: 'sync' },
  )

  return {
    cursor,
    variation,
    parked,
    activeMode,
    activePath,
    stateId,
    saveState,
    draftNotice,
    feedback,
    willReplaceDraft,
    selectPly,
    tryMove,
    undoVariationMove,
    returnToRecordedLine,
    resumeDraft,
    discardVariation,
    restoreDraft,
    exportDraft,
    currentDraft,
  }
}
