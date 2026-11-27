<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { applyCoreMove, computeBoardView } from '~/utils/core-wasm'
import { percussionPlayer } from '~/utils/audio'
import { capturedFaces, pairMoves, plateOrder, sideName } from '~/utils/side-plates'
import {
  matchServerBase,
  SessionEndedError,
  useBrowserSession,
  type BrowserSession,
} from '~/composables/useBrowserSession'

const STARTING_FEN = 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1'

interface MatchSnapshot {
  match_id: string
  invitation_code: string
  rules_profile: string
  status: 'waiting' | 'playing' | 'finished' | 'interrupted'
  seq: number
  fen: string
  side_to_move: 'red' | 'black'
  moves_uci: string[]
  moves_chinese: string[]
  red_seated: boolean
  black_seated: boolean
  clock_initial_ms: number | null
  clock_increment_ms: number
  red_remaining_ms: number | null
  black_remaining_ms: number | null
  is_friendly: boolean
  draw_offered_by: 'red' | 'black' | null
  takeback_requested_by: 'red' | 'black' | null
  next_rematch_invitation_code: string | null
  outcome: string | null
  reason_code: string | null
  visibility: 'private' | 'unlisted' | 'public'
  /** Advances with every change the server stores for the match; orders snapshots across routes. */
  state_version?: number
  /** This participant's own saved choices; the effective `visibility` needs both players. */
  viewer_choices?: {
    side: 'red' | 'black'
    revision: number
    visibility: 'private' | 'unlisted' | 'public'
    allow_internal_training: boolean
    allow_published_research: boolean
    allow_dataset_release: boolean
  }
}

const route = useRoute()
const router = useRouter()
const config = useRuntimeConfig()
const {
  locale,
  material,
  darkSurface,
  soundEnabled,
  reducedMotion,
  traditionalGlyphs,
  t,
} = usePreferences()

const code = computed(() => String(route.params.code || ''))
const assignedSide = ref<'red' | 'black' | null>(null)
const snapshot = ref<MatchSnapshot | null>(null)
const { labels: moveLabels } = useNotatedMoves(computed(() => snapshot.value?.moves_uci ?? []))
const connectionState = ref<'connecting' | 'connected' | 'reconnecting' | 'error'>('connecting')
const errorBanner = ref<string | null>(null)
const copiedLink = ref(false)

const redBelow = ref(true)
const selectedUci = ref<string | null>(null)
const lastAnnouncement = ref('')

// Postgame replay & variation tryout
const replayPly = ref<number | null>(null)
const postgameBranchMoves = ref<string[]>([])

// Privacy & dual-participant consent state
const desiredVisibility = ref<'private' | 'unlisted' | 'public'>('private')
const consentTraining = ref(false)
const consentResearch = ref(false)
const consentDataset = ref(false)
const privacyStatusMsg = ref<string | null>(null)

// Live clock interpolation anchored to monotonic performance.now()
const displayRedMs = ref<number | null>(null)
const displayBlackMs = ref<number | null>(null)
let baseRedMs: number | null = null
let baseBlackMs: number | null = null
let clockAnchorPerfMs = 0
let pollTickCount = 0
let clockTimer: ReturnType<typeof setInterval> | null = null
let ws: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let reconnectingInFlight = false
let isUnmounted = false
const browserSession = useBrowserSession()
let guestSession: BrowserSession | null = null

const shareUrl = computed(() => {
  if (typeof window === 'undefined') return `/invite/${code.value}`
  return `${window.location.origin}/invite/${code.value}`
})

const effectiveMovesUci = computed(() => {
  const main = snapshot.value?.moves_uci ?? []
  if (replayPly.value !== null) {
    return [...main.slice(0, replayPly.value), ...postgameBranchMoves.value]
  }
  return main
})

const { data: boardData, refresh: refreshBoard } = await useAsyncData(
  () => `invite-board-${code.value}`,
  () =>
    computeBoardView({
      fen: STARTING_FEN,
      movesUci: effectiveMovesUci.value,
      width: 480,
      height: 533,
      padding: 20,
      redBelow: redBelow.value,
      selectedUci: selectedUci.value,
      traditionalGlyphs: traditionalGlyphs.value,
      materialId: material.value,
      darkSurface: darkSurface.value,
      reducedMotion: reducedMotion.value,
    }),
  {
    watch: [effectiveMovesUci, redBelow, selectedUci, traditionalGlyphs, material, darkSurface],
  },
)

function syncClocksFromSnapshot(snap: MatchSnapshot) {
  baseRedMs = snap.red_remaining_ms
  baseBlackMs = snap.black_remaining_ms
  clockAnchorPerfMs = typeof performance !== 'undefined' ? performance.now() : Date.now()
  displayRedMs.value = snap.red_remaining_ms
  displayBlackMs.value = snap.black_remaining_ms
}

function formatClock(ms: number | null): string {
  if (ms === null) return t('clock.untimed')
  const clamped = Math.max(0, ms)
  const totalSec = Math.floor(clamped / 1000)
  const mins = Math.floor(totalSec / 60)
  const secs = totalSec % 60
  return `${mins}:${String(secs).padStart(2, '0')}`
}

/**
 * A request to the match server as this browser's guest. A mutation carries the CSRF token. When the server refuses
 * the session, the browser's session is checked again: the request is retried only if a session still exists, so it
 * never silently becomes another guest's request.
 */
async function guestFetch(
  path: string,
  init: RequestInit = {},
  mutation = false,
  options: { recovery?: boolean } = {},
): Promise<Response> {
  const send = (sess: BrowserSession) =>
    fetch(`${matchServerBase()}${path}`, {
      credentials: 'include',
      ...init,
      headers: mutation ? browserSession.mutationHeaders(sess) : init.headers,
    })
  if (roomStopped.value) throw new Error(stopReason)
  const bound = guestSession
  let session = bound
  // A request checks the session first, except a recovery read, which travels with the cookie alone: recovery asks the
  // server itself, once, before it reopens anything.
  if (mutation || !bound || !options.recovery) {
    // A session the server confirms has ended stops the room, so nothing queued or reconnecting acts or reads for it;
    // a network failure only fails this request.
    session = await browserSession.current().catch((error: unknown) => {
      if (error instanceof SessionEndedError) stopRoom(error.message)
      throw error
    })
    // The room belongs to the guest that opened it; a browser now holding another guest stops rather than act for it.
    if (bound && session.account_id !== bound.account_id) {
      stopRoom()
      throw new Error(ANOTHER_GUEST)
    }
    guestSession = session
  }
  const resp = await send(session!)
  if (!(await browserSession.refusedBy(resp))) return resp
  const again = await browserSession.recover({ fresh: true, readOnly: true })
  if (!again) {
    const ended = new SessionEndedError()
    stopRoom(ended.message)
    throw ended
  }
  if (again.account_id !== session!.account_id) {
    stopRoom()
    throw new Error(ANOTHER_GUEST)
  }
  guestSession = again
  return send(again)
}

const ANOTHER_GUEST = 'This browser is now signed in as another guest; reload the room to continue as that guest'
const SEAT_LOST = 'This room no longer shows your seat; reload the room to continue'

/**
 * Stops the room at once for a snapshot that does not show this guest's seat, then asks the server, without setting
 * the cookie again, which it was: the session ended, or the browser now holds another guest.
 */
function stopForLostSeat() {
  stopRoom(SEAT_LOST)
  const room = guestSession
  void browserSession.recover({ fresh: true, readOnly: true }).then(
    (fresh) => {
      const reason = !fresh
        ? new SessionEndedError().message
        : room && fresh.account_id !== room.account_id
          ? ANOTHER_GUEST
          : null
      if (reason && stopReason === SEAT_LOST) {
        stopReason = reason
        errorBanner.value = reason
      }
    },
    () => undefined,
  )
}

/** The server's own explanation of a refusal, or a plain fallback with the status. */
async function refusalMessage(resp: Response, fallback: string): Promise<string> {
  const body = (await resp.json().catch(() => null)) as { error?: { message?: unknown } } | null
  return typeof body?.error?.message === 'string' ? body.error.message : `${fallback} (${resp.status})`
}

type ChoiceField = 'visibility' | 'allow_internal_training' | 'allow_published_research' | 'allow_dataset_release'

const VISIBILITIES = ['private', 'unlisted', 'public']
const CHOICE_LABELS: Record<ChoiceField, string> = {
  visibility: 'Your visibility choice',
  allow_internal_training: 'Internal model training',
  allow_published_research: 'Published research analysis',
  allow_dataset_release: 'Open dataset release',
}
type Choices = NonNullable<MatchSnapshot['viewer_choices']>

/** True while one choice is being saved; every control waits until it settles, so no save queues behind another. */
const savingChoice = ref(false)
/** True while the saved choices are being read back after a save; the controls stay locked until they are shown. */
const reconciling = ref(false)
/** The privacy controls accept a change only when they show confirmed, current choices. */
const choicesLocked = computed(
  () => savingChoice.value || reconciling.value || choicesUnconfirmed.value || roomStopped.value,
)
/** The revision of the choices the controls show: a save is made against exactly what the player saw. */
let shownRevision = -1
/** The match and state version of the snapshot shown; one of the same match installs only when its version is newer. */
let shownVersion: { matchId: string; version: number } | null = null

/** Whether the shown effective visibility came from a versioned snapshot with a known value. */
const visibilityConfirmed = ref(false)

const STATUSES: readonly string[] = ['waiting', 'playing', 'finished', 'interrupted']
const SIDES: readonly unknown[] = [null, 'red', 'black']

/** Whether `data` is a snapshot of `matchId` with every field the room reads, in the shapes the server sends. */
function isSnapshotOf(data: unknown, matchId: string | undefined): data is MatchSnapshot {
  if (typeof data !== 'object' || data === null) return false
  const d = data as Record<string, unknown>
  const strings = (v: unknown) => Array.isArray(v) && v.every((x) => typeof x === 'string')
  const clock = (v: unknown) => v === null || (typeof v === 'number' && Number.isFinite(v))
  const text = (v: unknown) => v === null || typeof v === 'string'
  return (
    typeof d.match_id === 'string' &&
    (matchId === undefined || d.match_id === matchId) &&
    Number.isSafeInteger(d.seq) &&
    STATUSES.includes(d.status as string) &&
    (d.side_to_move === 'red' || d.side_to_move === 'black') &&
    typeof d.fen === 'string' &&
    strings(d.moves_uci) &&
    strings(d.moves_chinese) &&
    typeof d.red_seated === 'boolean' &&
    typeof d.black_seated === 'boolean' &&
    clock(d.red_remaining_ms) &&
    clock(d.black_remaining_ms) &&
    clock(d.clock_initial_ms) &&
    typeof d.clock_increment_ms === 'number' &&
    typeof d.is_friendly === 'boolean' &&
    SIDES.includes(d.draw_offered_by) &&
    SIDES.includes(d.takeback_requested_by) &&
    text(d.next_rematch_invitation_code) &&
    text(d.outcome) &&
    text(d.reason_code) &&
    typeof d.visibility === 'string'
  )
}

/**
 * The version the effective visibility shown must reach to count as confirmed: raised by this room's own save and by a
 * newer snapshot whose visibility could not be read, and met only by a valid snapshot at least that new.
 */
let requiredVersion = 0

/** Whether the shown effective visibility is the server's at a version no older than any change known to follow it. */
function refreshConfirmation() {
  visibilityConfirmed.value = shownVersion !== null && shownVersion.version >= requiredVersion
}

/**
 * Installs a snapshot from the stream or any HTTP reply and reports whether it was taken. The server advances a match's
 * state version with every change it stores, so a snapshot of the shown match is taken only when its version is newer:
 * whatever order the routes deliver them in, the room never goes back. Within one turn (same sequence and phase) the
 * clocks only run down: a reading sampled earlier never gives time back, and only the running clock takes a lower
 * reading. A server that sends no version is ordered by event sequence instead. Another match (a rematch) starts its own
 * order.
 */
function install(snap: MatchSnapshot): boolean {
  const current = snapshot.value
  const sameMatch = current?.match_id === snap.match_id
  if (!sameMatch) requiredVersion = 0
  const version = snap.state_version
  const versioned = Number.isSafeInteger(version) && (version as number) >= 0
  // A snapshot whose effective visibility cannot be read is not shown. A newer one means the visibility shown may be
  // superseded, so it stays unconfirmed until a valid snapshot that new; a stale one changes nothing.
  if (!VISIBILITIES.includes(snap.visibility)) {
    if (sameMatch && versioned && (!shownVersion || (version as number) > shownVersion.version)) {
      requiredVersion = Math.max(requiredVersion, version as number)
      refreshConfirmation()
    }
    return false
  }
  if (current && sameMatch) {
    if (shownVersion) {
      if (!versioned || (version as number) < shownVersion.version) return false
      if (version === shownVersion.version) {
        lowerRunningClock(snap)
        return false
      }
    } else if (snap.seq < current.seq) {
      return false
    }
  }
  const sameTurn = current !== null && sameMatch && current.seq === snap.seq && current.status === snap.status
  snapshot.value = snap
  shownVersion = versioned ? { matchId: snap.match_id, version: version as number } : null
  refreshConfirmation()
  if (sameTurn) lowerRunningClock(snap)
  else syncClocksFromSnapshot(snap)
  return true
}

/** Brings the running clock, and only it, down to a reading of the same turn when that reading is lower. */
function lowerRunningClock(snap: MatchSnapshot) {
  if (snap.status !== 'playing') return
  const now = typeof performance !== 'undefined' ? performance.now() : Date.now()
  if (snap.side_to_move === 'red') {
    const incoming = snap.red_remaining_ms
    if (incoming !== null && displayRedMs.value !== null && incoming < displayRedMs.value) {
      baseRedMs = incoming
      clockAnchorPerfMs = now
      displayRedMs.value = incoming
    }
  } else {
    const incoming = snap.black_remaining_ms
    if (incoming !== null && displayBlackMs.value !== null && incoming < displayBlackMs.value) {
      baseBlackMs = incoming
      clockAnchorPerfMs = now
      displayBlackMs.value = incoming
    }
  }
}

/**
 * True until the controls show a validated personal snapshot, and again whenever the saved choices could not be read
 * back; the controls stay locked meanwhile, so no change is made against choices that are not confirmed.
 */
const choicesUnconfirmed = ref(true)
/** Set when a snapshot belongs to another guest; the room then acts no further until it is reloaded. */
const roomStopped = ref(false)
/** Why the room stopped, kept so every later action reports the same reason. */
let stopReason = ''

/** Stops the room: no further requests, and the original guest's live stream closes, so nothing more of the match
 * reaches this browser under the guest it no longer holds. */
function stopRoom(reason = ANOTHER_GUEST) {
  if (!roomStopped.value) stopReason = reason
  roomStopped.value = true
  errorBanner.value = stopReason
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  if (ws) {
    const socket = ws
    ws = null
    socket.onclose = null
    socket.onmessage = null
    socket.close()
  }
}

function wellFormedChoices(value: unknown): value is Choices {
  if (typeof value !== 'object' || value === null) return false
  const c = value as Record<string, unknown>
  return (
    (c.side === 'red' || c.side === 'black') &&
    Number.isSafeInteger(c.revision) &&
    (c.revision as number) >= 0 &&
    VISIBILITIES.includes(c.visibility as string) &&
    typeof c.allow_internal_training === 'boolean' &&
    typeof c.allow_published_research === 'boolean' &&
    typeof c.allow_dataset_release === 'boolean'
  )
}

/**
 * Shows this participant's own saved choices, separately from the match's effective visibility, and reports whether
 * it did. A snapshot naming another seat, or none, belongs to another guest: the room stops. One older than the
 * choices shown, or one arriving while a save is in flight, changes nothing; the controls and their revision only
 * ever move together.
 */
function hydrateChoices(snap: MatchSnapshot): boolean {
  if (!assignedSide.value) return false
  const latest = snap.viewer_choices
  if (latest?.side !== assignedSide.value) {
    stopForLostSeat()
    return false
  }
  if (!wellFormedChoices(latest)) {
    choicesUnconfirmed.value = true
    return false
  }
  if (savingChoice.value || latest.revision < shownRevision) return false
  if (!VISIBILITIES.includes(snap.visibility)) {
    choicesUnconfirmed.value = true
    return false
  }
  shownRevision = latest.revision
  desiredVisibility.value = latest.visibility
  consentTraining.value = latest.allow_internal_training
  consentResearch.value = latest.allow_published_research
  consentDataset.value = latest.allow_dataset_release
  choicesUnconfirmed.value = false
  return true
}

/** Reads the saved choices back; true once a current personal snapshot has been shown. */
async function refreshChoices(): Promise<boolean> {
  const matchId = snapshot.value?.match_id
  if (!matchId) return false
  try {
    const resp = await guestFetch(`/api/v1/matches/${matchId}`)
    const read: unknown = resp.ok ? await resp.json().catch(() => null) : null
    // A read of another match, or one missing what the room reads, is a failed read: none of it is used.
    const snap = isSnapshotOf(read, matchId) ? read : null
    if (roomStopped.value) return false
    // Only a snapshot actually shown counts as confirmed; one ignored as older or malformed does not.
    const shown = snap ? hydrateChoices(snap) : false
    if (roomStopped.value) return false
    // The read installs like any snapshot: only if it is newer than the one shown, whichever participant changed it.
    if (snap) install(snap)
    if (snap && shown) return true
    // A read superseded by a newer snapshot already shown leaves that newer confirmation in place.
    if (
      snap &&
      wellFormedChoices(snap.viewer_choices) &&
      snap.viewer_choices.revision < shownRevision &&
      !choicesUnconfirmed.value
    ) {
      return true
    }
  } catch {
    // Reported below as an unconfirmed read.
  }
  if (!roomStopped.value) choicesUnconfirmed.value = true
  return false
}

async function recheckChoices() {
  privacyStatusMsg.value = (await refreshChoices())
    ? 'Your saved choices are shown.'
    : 'Your saved choices could not be read back; the controls may not show what is saved.'
}

/**
 * Saves one choice as soon as the player changes it, naming only that choice and the revision of the choices shown.
 * The change is the explicit action, so a withdrawal is always sent; the server refuses a save made against an older
 * revision, so it can never overwrite a choice saved meanwhile from another tab.
 */
async function saveChoice(field: ChoiceField, value: string | boolean) {
  const matchId = snapshot.value?.match_id
  if (!matchId || savingChoice.value || roomStopped.value) return
  savingChoice.value = true
  // The edit makes the shown choices unconfirmed until a validated snapshot shows what was saved.
  choicesUnconfirmed.value = true
  const madeAgainst = shownRevision
  let outcome: string
  try {
    const resp = await guestFetch(
      `/api/v1/matches/${matchId}/privacy`,
      { method: 'POST', body: JSON.stringify({ [field]: value, expected_revision: madeAgainst }) },
      true,
    )
    const ack = (await resp.json().catch(() => null)) as
      | { effective_visibility?: unknown; choices_revision?: unknown; state_version?: unknown }
      | null
    if (resp.status === 409) {
      outcome = `${CHOICE_LABELS[field]} was not saved: your choices changed in another tab. Showing what is saved now.`
    } else if (!resp.ok) {
      outcome = `${CHOICE_LABELS[field]} was not saved (${resp.status}). Showing what is saved now.`
    } else if (
      !roomStopped.value &&
      typeof ack?.effective_visibility === 'string' &&
      VISIBILITIES.includes(ack.effective_visibility) &&
      ack.choices_revision === madeAgainst + 1
    ) {
      // A save advances the revision by exactly one; any other acknowledgment is not adopted.
      shownRevision = madeAgainst + 1
      // The save changed what the match's effective visibility may be; until a snapshot at least as new as the version
      // it wrote is shown, the summary says it is not confirmed rather than show the state before the save.
      if (Number.isSafeInteger(ack.state_version)) {
        requiredVersion = Math.max(requiredVersion, ack.state_version as number)
        refreshConfirmation()
      }
      // The effective visibility is shown in one place, the summary above, which the read-back keeps current.
      outcome = `${CHOICE_LABELS[field]} saved. Sharing takes effect only when both players choose it.`
    } else {
      outcome = `${CHOICE_LABELS[field]} may not have been saved. Showing what is saved now.`
    }
  } catch (err) {
    outcome = `${CHOICE_LABELS[field]} may not have been saved: ${err instanceof Error ? err.message : String(err)}`
  } finally {
    // The controls stay locked through the read-back below, until the saved choices are shown.
    reconciling.value = true
    savingChoice.value = false
  }
  // A reply that arrives after the room stopped tells the player nothing; the reason it stopped stays in view.
  if (roomStopped.value) {
    reconciling.value = false
    return
  }
  privacyStatusMsg.value = outcome
  try {
    if (!(await refreshChoices()) && !roomStopped.value) {
      privacyStatusMsg.value = `${outcome} Your saved choices could not be read back; check again before changing them.`
    }
  } finally {
    reconciling.value = false
  }
}

/** Leaving with a choice still being saved could lose it (a withdrawal included), so the browser asks first. */
function warnWhileSaving(event: BeforeUnloadEvent) {
  if (savingChoice.value || reconciling.value) event.preventDefault()
}

/** Leaving the room, or moving to another invitation within it, asks first while a choice is being saved. */
const confirmLeavingWhileSaving = () =>
  savingChoice.value || reconciling.value
    ? window.confirm('A privacy choice is still being saved. Leave anyway?')
    : true
onBeforeRouteLeave(confirmLeavingWhileSaving)
// Within the room, only a change of invitation leaves it; a query or hash change keeps the room and its save.
onBeforeRouteUpdate((to, from) => (to.params.code === from.params.code ? true : confirmLeavingWhileSaving()))

function connectMatchWebSocket(matchId: string) {
  if (isUnmounted || roomStopped.value) return
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
  if (ws) {
    const prev = ws
    ws = null
    prev.onclose = null
    prev.close()
  }
  const httpBase = matchServerBase()
  const wsBase = httpBase.replace(/^http/, 'ws')
  // Browser WebSocket uses HttpOnly cookie set on /api/v1/auth/guest; if cross-port in local dev, fallback poll/reconnect stays synced
  const socket = new WebSocket(`${wsBase}/api/v1/matches/${matchId}/ws`)
  ws = socket
  if (typeof window !== 'undefined') {
    ;(window as unknown as Record<string, unknown>).__jadeActiveMatchSocket = socket
  }

  socket.onopen = () => {
    if (ws === socket) {
      connectionState.value = 'connected'
    }
  }

  socket.onmessage = (ev) => {
    if (roomStopped.value) return
    try {
      const data = JSON.parse(String(ev.data))
      if (isSnapshotOf(data, snapshot.value?.match_id)) {
        const prevLen = snapshot.value?.moves_uci.length ?? 0
        install(data)
        hydrateChoices(data)
        if (roomStopped.value) return
        if (snapshot.value!.moves_uci.length > prevLen) {
          void percussionPlayer.play(material.value, 'place', soundEnabled.value, false)
          const latestZh = snapshot.value!.moves_chinese[snapshot.value!.moves_chinese.length - 1]
          if (latestZh) lastAnnouncement.value = latestZh
        }
      }
    } catch {
      // Ignore malformed frame
    }
  }

  socket.onclose = () => {
    if (ws === socket) {
      ws = null
    }
    if (!isUnmounted && snapshot.value && snapshot.value.status !== 'finished') {
      connectionState.value = 'reconnecting'
      if (!reconnectTimer) {
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null
          void recoverGapAndReconnect(matchId)
        }, 500)
      }
    }
  }
}

/**
 * Asks the server, without setting the cookie again, whether this browser still holds the room's guest; stops the
 * room when it does not. True only when the session stands; a network failure answers false without stopping, so the
 * caller retries later.
 */
async function confirmSessionOrStop(): Promise<boolean> {
  let fresh: BrowserSession | null
  try {
    fresh = await browserSession.recover({ fresh: true, readOnly: true })
  } catch {
    return false
  }
  if (roomStopped.value) return false
  if (!fresh) {
    stopRoom(new SessionEndedError().message)
    return false
  }
  if (guestSession && fresh.account_id !== guestSession.account_id) {
    stopRoom()
    return false
  }
  return true
}

async function recoverGapAndReconnect(matchId: string) {
  if (isUnmounted || reconnectingInFlight || roomStopped.value) return
  reconnectingInFlight = true
  try {
    const since = snapshot.value?.seq ?? 0
    const resp = await guestFetch(`/api/v1/matches/${matchId}?since_seq=${since}`, {}, false, { recovery: true })
    if (roomStopped.value) return
    // Whether the server has confirmed, in this recovery, that the browser still holds the room's guest.
    let confirmed = false
    if (resp.ok) {
      const snap: unknown = await resp.json().catch(() => null)
      // A read answered after the room stopped belongs to the guest it no longer holds; it is dropped.
      if (roomStopped.value) return
      // An unreadable read installs nothing, and the stream can still be reopened below.
      if (isSnapshotOf(snap, matchId)) {
        install(snap)
        hydrateChoices(snap)
        if (roomStopped.value) return
        if (snapshot.value?.status === 'finished') {
          connectionState.value = 'connected'
          return
        }
      }
    } else {
      // A refused read can mean the browser's session ended or changed; the server says which, and the room stops
      // rather than keep a stream open for a guest the browser no longer holds.
      if (!(await confirmSessionOrStop())) return
      confirmed = true
    }
    // Reopen a live WebSocket if none is open or connecting, deciding now and asking the server first (once per
    // recovery), since a dropped stream can mean the session ended; keep `reconnecting` until `onopen` fires.
    const streamDown = () => !ws || (ws.readyState !== WebSocket.OPEN && ws.readyState !== WebSocket.CONNECTING)
    if (
      !isUnmounted &&
      snapshot.value?.status !== 'finished' &&
      streamDown() &&
      (confirmed || (await confirmSessionOrStop())) &&
      // Another path may have reopened the stream while the server was asked.
      streamDown()
    ) {
      connectMatchWebSocket(matchId)
    }
  } catch {
    // Keep reconnecting indicator and fallback polling active until WebSocket reconnects
  } finally {
    reconnectingInFlight = false
  }
}

async function joinAndSubscribe() {
  errorBanner.value = null
  try {
    // Opening the room checks the browser's session afresh, recovering its guest or creating one only if none exists.
    guestSession = await browserSession.ensure('Jade Player')
    const joinResp = await guestFetch(`/api/v1/invitations/${code.value}/join`, { method: 'POST' }, true)
    if (!joinResp.ok) {
      throw new Error(`Invitation not found or unavailable (${joinResp.status})`)
    }
    const joined = await joinResp.json()
    assignedSide.value = joined.assigned_side
    const first: unknown = joined.snapshot
    // The room opens only on a match it can show; otherwise it says so rather than wait on nothing.
    if (!isSnapshotOf(first, undefined) || (!install(first) && !snapshot.value)) {
      throw new Error('The match server sent a match this page cannot read; reload to try again')
    }
    hydrateChoices(first)
    if (assignedSide.value === 'black') {
      redBelow.value = false
    }
    const shown = snapshot.value ?? first
    connectionState.value = shown.status === 'finished' ? 'connected' : 'connecting'
    if (shown.status !== 'finished') {
      connectMatchWebSocket(shown.match_id)
    }
  } catch (err) {
    connectionState.value = 'error'
    errorBanner.value = err instanceof Error ? err.message : String(err)
  }
}

async function sendMatchCommand(payload: Record<string, unknown>) {
  // A stopped room sends nothing, and the reason it stopped stays in view.
  if (!snapshot.value || roomStopped.value) return
  errorBanner.value = null
  let resp: Response
  try {
    resp = await guestFetch(
      `/api/v1/matches/${snapshot.value.match_id}/commands`,
      { method: 'POST', body: JSON.stringify(payload) },
      true,
    )
  } catch (err) {
    // A failure after the room stopped leaves the reason it stopped in view.
    if (!roomStopped.value) errorBanner.value = err instanceof Error ? err.message : String(err)
    return
  }
  const data = await resp.json().catch(() => null)
  // A reply that arrives after the room stopped is for a session the room no longer holds; it is dropped.
  if (roomStopped.value) return
  if (!resp.ok) {
    errorBanner.value = data?.error?.message || `Command rejected (${resp.status})`
    return
  }
  const matchId = snapshot.value?.match_id
  if (!isSnapshotOf(data, matchId)) {
    // The command may have been played; the room is left as it is and the match is read again rather than guessed.
    errorBanner.value = 'The reply to that move could not be read; showing the match as the server has it.'
    if (matchId) void recoverGapAndReconnect(matchId)
    return
  }
  install(data)
  hydrateChoices(data)
  if (roomStopped.value) return
  await refreshBoard()
}

function coordTuple(uci: string): [number, number] {
  return [uci.charCodeAt(0) - 97, Number(uci[1])]
}

async function handleBoardPlayMove(fromUci: string, toUci: string) {
  selectedUci.value = null
  // Postgame exploration branch!
  if (snapshot.value?.status === 'finished' || replayPly.value !== null) {
    try {
      const res = await applyCoreMove({
        fen: STARTING_FEN,
        movesUci: effectiveMovesUci.value,
        token: `${fromUci}${toUci}`,
        traditionalGlyphs: traditionalGlyphs.value,
      })
      if (replayPly.value === null) {
        replayPly.value = snapshot.value?.moves_uci.length ?? 0
      }
      postgameBranchMoves.value = [...postgameBranchMoves.value, res.move_uci]
      await refreshBoard()
    } catch (err) {
      errorBanner.value = err instanceof Error ? err.message : String(err)
    }
    return
  }

  // A stopped room takes no live moves, and the reason it stopped stays in view.
  if (!snapshot.value || snapshot.value.status !== 'playing' || roomStopped.value) return
  if (assignedSide.value !== snapshot.value.side_to_move) {
    errorBanner.value = 'Waiting for opponent move'
    return
  }

  const commandId = `cmd_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`
  await sendMatchCommand({
    type: 'move',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: commandId,
    expected_seq: snapshot.value.seq,
    move: {
      from: coordTuple(fromUci),
      to: coordTuple(toUci),
    },
  })
}

async function offerDraw() {
  if (!snapshot.value) return
  await sendMatchCommand({
    type: 'offer_draw',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: `draw_offer_${Date.now()}`,
    expected_seq: snapshot.value.seq,
  })
}

async function acceptDraw() {
  if (!snapshot.value) return
  await sendMatchCommand({
    type: 'accept_draw',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: `draw_accept_${Date.now()}`,
    expected_seq: snapshot.value.seq,
  })
}

async function resignMatch() {
  if (!snapshot.value) return
  await sendMatchCommand({
    type: 'resign',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: `resign_${Date.now()}`,
    expected_seq: snapshot.value.seq,
  })
}

async function requestTakeback() {
  if (!snapshot.value) return
  await sendMatchCommand({
    type: 'request_takeback',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: `tb_req_${Date.now()}`,
    expected_seq: snapshot.value.seq,
  })
}

async function acceptTakeback() {
  if (!snapshot.value) return
  await sendMatchCommand({
    type: 'accept_takeback',
    protocol: 1,
    match_id: snapshot.value.match_id,
    command_id: `tb_acc_${Date.now()}`,
    expected_seq: snapshot.value.seq,
  })
}

async function exportMatchPgn() {
  if (!snapshot.value || typeof window === 'undefined') return
  const resp = await guestFetch(`/api/v1/matches/${snapshot.value.match_id}/export.pgn`).catch((err: unknown) => {
    errorBanner.value = err instanceof Error ? err.message : String(err)
    return null
  })
  if (!resp) return
  if (!resp.ok) {
    errorBanner.value = await refusalMessage(resp, 'The game could not be exported')
    return
  }
  const pgnText = await resp.text()
  const blob = new Blob([pgnText], { type: 'application/x-chess-pgn;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `jade-${snapshot.value.match_id}.pgn`
  document.body.appendChild(link)
  link.click()
  link.remove()
  URL.revokeObjectURL(url)
}

async function startRematch() {
  if (!snapshot.value) return
  if (snapshot.value.next_rematch_invitation_code) {
    await router.push(`/invite/${snapshot.value.next_rematch_invitation_code}`)
    return
  }
  const resp = await guestFetch(`/api/v1/matches/${snapshot.value.match_id}/rematch`, { method: 'POST' }, true).catch(
    (err: unknown) => {
      errorBanner.value = err instanceof Error ? err.message : String(err)
      return null
    },
  )
  if (resp && !resp.ok) {
    errorBanner.value = await refusalMessage(resp, 'A rematch could not be started')
    return
  }
  if (resp?.ok) {
    const created = await resp.json()
    await router.push(`/invite/${created.invitation_code}`)
  }
}

async function copyShareLink() {
  try {
    await navigator.clipboard.writeText(shareUrl.value)
    copiedLink.value = true
    setTimeout(() => {
      copiedLink.value = false
    }, 1800)
  } catch {
    // Clipboard unavailable
  }
}

onMounted(() => {
  window.addEventListener('beforeunload', warnWhileSaving)
  // A server render that could not compute the board leaves no data to hydrate, so the client computes it.
  if (!boardData.value) void refreshBoard()
  void joinAndSubscribe()
  clockTimer = setInterval(() => {
    pollTickCount = (pollTickCount + 1) % 4
    // Only poll HTTP when waiting for seat join or when WebSocket is reconnecting (never overwrite smooth live clocks every 250 ms!)
    if (
      snapshot.value &&
      pollTickCount === 0 &&
      (snapshot.value.status === 'waiting' || connectionState.value !== 'connected')
    ) {
      if (guestSession) {
        void recoverGapAndReconnect(snapshot.value.match_id)
      }
    }
    if (snapshot.value?.status === 'playing') {
      const nowPerf = typeof performance !== 'undefined' ? performance.now() : Date.now()
      const elapsed = Math.max(0, Math.floor(nowPerf - clockAnchorPerfMs))
      if (snapshot.value.side_to_move === 'red' && baseRedMs !== null) {
        displayRedMs.value = Math.max(0, baseRedMs - elapsed)
      } else if (snapshot.value.side_to_move === 'black' && baseBlackMs !== null) {
        displayBlackMs.value = Math.max(0, baseBlackMs - elapsed)
      }
    }
  }, 250)
})

watch(code, () => {
  replayPly.value = null
  postgameBranchMoves.value = []
  void joinAndSubscribe()
})

onBeforeUnmount(() => {
  window.removeEventListener('beforeunload', warnWhileSaving)
  isUnmounted = true
  if (clockTimer) clearInterval(clockTimer)
  if (reconnectTimer) clearTimeout(reconnectTimer)
  if (ws) {
    ws.onclose = null
    ws.close()
    ws = null
  }
})

// Side plates with the match clocks, as the home page and macOS show them.
const plates = computed(() => {
  const pieces = boardData.value?.snapshot.pieces ?? []
  const playing = snapshot.value?.status === 'playing'
  return plateOrder(redBelow.value).map((side) => {
    const toMove = playing && snapshot.value?.side_to_move === side
    const who = assignedSide.value === side ? 'You' : assignedSide.value ? 'Friend' : ''
    return {
      side,
      name: sideName(side, locale.value),
      caption: [who, toMove ? 'to move' : ''].filter(Boolean).join(' · '),
      toMove,
      clock: formatClock(side === 'red' ? displayRedMs.value : displayBlackMs.value),
      captured: capturedFaces(pieces, side, traditionalGlyphs.value),
    }
  })
})

const moveRows = computed(() => pairMoves(snapshot.value?.moves_uci ?? [], moveLabels.value))

function showPly(ply: number | null) {
  replayPly.value = ply
  postgameBranchMoves.value = []
}
</script>

<template>
  <div class="jade-table">
    <aside class="jade-plates" data-testid="match-clock-bar">
      <template v-for="(plate, i) in plates" :key="plate.side">
        <SidePlate :name="plate.name" :caption="plate.caption" :to-move="plate.toMove" :captured="plate.captured">
          <strong class="plate-clock" :data-testid="`clock-${plate.side}`">{{ plate.clock }}</strong>
        </SidePlate>
        <div
          v-if="i === 0"
          class="room-status"
          data-testid="match-status-badge"
          :data-connection-state="connectionState"
        >
          <template v-if="snapshot?.status === 'waiting'">Waiting for friend…</template>
          <template v-else-if="snapshot?.status === 'playing'">
            Live · {{ assignedSide ? `You play ${assignedSide}` : 'Spectating' }}
          </template>
          <template v-else-if="snapshot?.status === 'finished'">
            Finished · {{ snapshot.outcome }} ({{ snapshot.reason_code }})
          </template>
          <template v-else>{{ connectionState }}</template>
        </div>
      </template>
    </aside>

    <div class="jade-table-stage">
      <XiangqiBoard
        v-if="boardData"
        :snapshot="boardData.snapshot"
        :material="material"
        :traditional-glyphs="traditionalGlyphs"
        :dark-surface="darkSurface"
        :locale="locale"
        :interactive="assignedSide !== null || snapshot?.status === 'finished'"
        :last-announcement="lastAnnouncement"
        @select-square="(sq) => (selectedUci = sq)"
        @play-move="handleBoardPlayMove"
      />

      <!-- One glass capsule, as on macOS: flip always, then the actions the room's state allows. -->
      <div class="jade-capsule" role="toolbar" aria-label="Match">
        <IconButton :label="t('board.flip')" :paths="['M6 3v13M6 3 3 6M6 3l3 3M14 17V4M14 17l-3-3M14 17l3-3']" @click="redBelow = !redBelow" />

        <template v-if="snapshot?.status === 'playing' && assignedSide">
          <button
            v-if="snapshot.draw_offered_by && snapshot.draw_offered_by !== assignedSide"
            type="button"
            class="jade-btn jade-btn-primary"
            data-testid="btn-accept-draw"
            @click="acceptDraw"
          >
            Accept draw
          </button>
          <button v-else type="button" class="jade-btn" data-testid="btn-offer-draw" @click="offerDraw">
            {{ t('action.offer_draw') }}
          </button>

          <template v-if="snapshot.is_friendly && snapshot.moves_uci.length > 0">
            <button
              v-if="snapshot.takeback_requested_by && snapshot.takeback_requested_by !== assignedSide"
              type="button"
              class="jade-btn jade-btn-primary"
              data-testid="btn-accept-takeback"
              @click="acceptTakeback"
            >
              Accept takeback
            </button>
            <button v-else type="button" class="jade-btn" data-testid="btn-request-takeback" @click="requestTakeback">
              {{ t('board.undo') }}
            </button>
          </template>

          <button type="button" class="jade-btn jade-btn-danger" data-testid="btn-resign" @click="resignMatch">
            {{ t('action.resign') }}
          </button>
        </template>

        <template v-if="snapshot?.status === 'finished' || snapshot?.status === 'interrupted'">
          <button
            type="button"
            class="jade-btn jade-btn-primary"
            data-testid="btn-rematch"
            :disabled="savingChoice || reconciling"
            @click="startRematch"
          >
            {{ snapshot.next_rematch_invitation_code ? 'Join rematch' : t('action.rematch') }}
          </button>
          <button type="button" class="jade-btn" data-testid="btn-export-pgn" @click="exportMatchPgn">
            {{ t('export.pgn') }}
          </button>
          <NuxtLink :to="`/g/${snapshot.match_id}`" class="jade-btn" data-testid="link-open-replay">
            Replay &amp; study
          </NuxtLink>
        </template>
      </div>

      <p v-if="errorBanner" class="error-note" data-testid="match-error">{{ errorBanner }}</p>
    </div>

    <aside class="jade-ledger">
      <section v-if="snapshot?.status === 'waiting'" class="share" data-testid="invite-share-card" aria-label="Invitation link">
        <p class="share-note">Send this link to a friend. Your seat stays on this device; the link only offers the other seat.</p>
        <div class="share-row">
          <input type="text" readonly :value="shareUrl" class="share-input" aria-label="Invitation link" data-testid="share-link-input" />
          <button type="button" class="jade-btn jade-btn-primary" @click="copyShareLink">
            {{ copiedLink ? 'Copied' : 'Copy' }}
          </button>
        </div>
      </section>

      <section class="moves" data-testid="match-moves-card" aria-label="Moves">
        <div v-if="postgameBranchMoves.length > 0" class="branch-pill" data-testid="postgame-branch-banner">
          <span>Trying {{ postgameBranchMoves.join(' ') }}</span>
          <button type="button" class="jade-pill-btn" @click="showPly(null)">Back to the game</button>
        </div>

        <ol class="jade-move-list" data-testid="match-move-list">
          <li v-for="row in moveRows" :key="row.number" class="jade-move-row">
            <span class="jade-move-number">{{ row.number }}.</span>
            <button
              v-for="m in row.plies"
              :key="m.ply"
              type="button"
              class="jade-move"
              :class="{ current: replayPly === m.ply }"
              @click="showPly(m.ply)"
            >
              {{ m.label }}
            </button>
          </li>
        </ol>

        <div v-if="snapshot && snapshot.moves_uci.length > 0" class="scrub jade-rule">
          <button type="button" class="jade-pill-btn" data-testid="postgame-scrub-start" @click="showPly(0)">Start</button>
          <button type="button" class="jade-pill-btn" data-testid="postgame-scrub-live" @click="showPly(null)">Live</button>
        </div>
      </section>

      <EngineAnalysisCard
        v-if="snapshot?.status === 'finished'"
        :fen="STARTING_FEN"
        :moves-uci="effectiveMovesUci"
        :state-id="`match:${snapshot.match_id}:${effectiveMovesUci.join(',')}`"
        @play-engine-move="(uci) => handleBoardPlayMove(uci.slice(0, 2), uci.slice(2, 4))"
      />

      <!-- Dual-participant privacy and consent (handoff sections 3 and 16), folded so play stays the focus. -->
      <details v-if="assignedSide" class="privacy jade-rule" data-testid="privacy-consent-card">
        <!-- The match's effective visibility, which needs both players; this player's own choice is the control below. -->
        <summary data-testid="privacy-effective">
          Privacy ·
          {{
            !visibilityConfirmed
              ? 'Not confirmed'
              : snapshot?.visibility === 'public'
                ? 'Public'
                : snapshot?.visibility === 'unlisted'
                  ? 'Unlisted'
                  : snapshot?.visibility === 'private'
                    ? 'Private'
                    : 'Not confirmed'
          }}
        </summary>
        <p class="privacy-note">
          Matches are private by default. Sharing and any training or research use take effect only when both players agree.
        </p>
        <label class="priv-row">
          <span>Your visibility choice</span>
          <select
            v-model="desiredVisibility"
            class="jade-select"
            data-testid="privacy-visibility"
            :disabled="choicesLocked"
            @change="saveChoice('visibility', desiredVisibility)"
          >
            <option value="private">Private (default)</option>
            <option value="unlisted">Unlisted link</option>
            <option value="public">Public archive</option>
          </select>
        </label>
        <!-- Each choice saves as soon as it changes. -->
        <label class="priv-check">
          <input
            v-model="consentTraining"
            type="checkbox"
            :disabled="choicesLocked"
            @change="saveChoice('allow_internal_training', consentTraining)"
          />
          <span>Allow internal model training</span>
        </label>
        <label class="priv-check">
          <input
            v-model="consentResearch"
            type="checkbox"
            :disabled="choicesLocked"
            @change="saveChoice('allow_published_research', consentResearch)"
          />
          <span>Allow published research analysis</span>
        </label>
        <label class="priv-check">
          <input
            v-model="consentDataset"
            type="checkbox"
            :disabled="choicesLocked"
            @change="saveChoice('allow_dataset_release', consentDataset)"
          />
          <span>Allow open dataset release</span>
        </label>
        <p v-if="privacyStatusMsg" class="privacy-note" data-testid="privacy-status">
          {{ privacyStatusMsg }}
        </p>
        <p v-if="choicesUnconfirmed && !roomStopped && !savingChoice && !reconciling" class="privacy-note" data-testid="privacy-unconfirmed">
          Your saved choices are not confirmed yet, so they cannot be changed.
          <button type="button" class="jade-pill-btn" data-testid="privacy-recheck" @click="recheckChoices">
            Check again
          </button>
        </p>
      </details>
    </aside>
  </div>
</template>

<style scoped>
.plate-clock {
  font-family: var(--jade-display);
  font-weight: 400;
  font-size: clamp(2rem, 3.2vw, 2.8rem);
  line-height: 1;
  font-variant-numeric: tabular-nums;
  color: var(--jade-muted);
}

.to-move .plate-clock {
  color: var(--jade-paper);
}

.room-status {
  font-size: 0.8rem;
  color: var(--jade-muted);
}

.share {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.share-note,
.privacy-note {
  margin: 0;
  font-size: 0.8rem;
  color: var(--jade-muted);
}

.share-row {
  display: flex;
  gap: 8px;
}

.share-input {
  flex: 1;
  min-width: 0;
  padding: 7px 10px;
  border-radius: 8px;
  border: 1px solid var(--jade-hairline);
  background: var(--jade-surface);
  color: var(--jade-paper);
  font: inherit;
  font-size: 0.82rem;
}

.moves {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 10px;
  min-height: 160px;
}

.scrub {
  display: flex;
  gap: 6px;
}

.branch-pill {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 0.8rem;
  color: var(--jade-pale);
}

.privacy {
  display: flex;
  flex-direction: column;
  gap: 8px;
  font-size: 0.85rem;
}

.privacy summary {
  cursor: pointer;
  color: var(--jade-muted);
}

.privacy[open] {
  gap: 10px;
}

.priv-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin: 8px 0;
}

.priv-check {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 4px 0;
}

.error-note {
  margin: 0;
  font-size: 0.82rem;
  color: var(--jade-vermilion-text);
}

@media (max-width: 1100px) {
  .moves {
    justify-content: flex-start;
    min-height: 0;
  }
}
</style>
