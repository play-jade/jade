<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { applyCoreMove, computeBoardView } from '~/utils/core-wasm'
import { percussionPlayer } from '~/utils/audio'
import { capturedFaces, pairMoves, plateOrder, sideName } from '~/utils/side-plates'

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
let guestSession: { account_id: string; access_token: string; csrf_token: string } | null = null

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

async function ensureGuestSession() {
  if (guestSession) return guestSession
  const raw = sessionStorage.getItem('jade_guest_session')
  if (raw) {
    guestSession = JSON.parse(raw)
    return guestSession!
  }
  const resp = await fetch(`${config.public.matchServerUrl}/api/v1/auth/guest`, {
    method: 'POST',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ display_name: 'Jade Player', transport: 'web_cookie' }),
  })
  if (!resp.ok) throw new Error(`Guest session failed (${resp.status})`)
  guestSession = await resp.json()
  sessionStorage.setItem('jade_guest_session', JSON.stringify(guestSession))
  return guestSession!
}

function connectMatchWebSocket(matchId: string, token: string) {
  if (isUnmounted) return
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
  const httpBase = config.public.matchServerUrl
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
    try {
      const data = JSON.parse(String(ev.data))
      if (data.match_id) {
        const prevLen = snapshot.value?.moves_uci.length ?? 0
        snapshot.value = data as MatchSnapshot
        syncClocksFromSnapshot(snapshot.value)
        if (snapshot.value.moves_uci.length > prevLen) {
          void percussionPlayer.play(material.value, 'place', soundEnabled.value, false)
          const latestZh = snapshot.value.moves_chinese[snapshot.value.moves_chinese.length - 1]
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
          void recoverGapAndReconnect(matchId, token)
        }, 500)
      }
    }
  }
}

async function recoverGapAndReconnect(matchId: string, token: string) {
  if (isUnmounted || reconnectingInFlight) return
  reconnectingInFlight = true
  try {
    const since = snapshot.value?.seq ?? 0
    const resp = await fetch(
      `${config.public.matchServerUrl}/api/v1/matches/${matchId}?since_seq=${since}`,
      {
        credentials: 'include',
        headers: { authorization: `Bearer ${token}` },
      },
    )
    if (resp.ok) {
      const snap = (await resp.json()) as MatchSnapshot
      snapshot.value = snap
      syncClocksFromSnapshot(snap)
      if (snap.status === 'finished') {
        connectionState.value = 'connected'
        return
      }
    }
    // Reopen a live WebSocket if none is currently open or connecting; keep `reconnecting` until `onopen` fires!
    if (
      !isUnmounted &&
      snapshot.value?.status !== 'finished' &&
      (!ws || (ws.readyState !== WebSocket.OPEN && ws.readyState !== WebSocket.CONNECTING))
    ) {
      connectMatchWebSocket(matchId, token)
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
    const sess = await ensureGuestSession()
    const joinResp = await fetch(
      `${config.public.matchServerUrl}/api/v1/invitations/${code.value}/join`,
      {
        method: 'POST',
        credentials: 'include',
        headers: {
          authorization: `Bearer ${sess.access_token}`,
          'x-csrf-token': sess.csrf_token,
        },
      },
    )
    if (!joinResp.ok) {
      throw new Error(`Invitation not found or unavailable (${joinResp.status})`)
    }
    const joined = await joinResp.json()
    assignedSide.value = joined.assigned_side
    snapshot.value = joined.snapshot as MatchSnapshot
    if (assignedSide.value === 'black') {
      redBelow.value = false
    }
    syncClocksFromSnapshot(snapshot.value)
    connectionState.value = snapshot.value.status === 'finished' ? 'connected' : 'connecting'
    if (snapshot.value.status !== 'finished') {
      connectMatchWebSocket(snapshot.value.match_id, sess.access_token)
    }
  } catch (err) {
    connectionState.value = 'error'
    errorBanner.value = err instanceof Error ? err.message : String(err)
  }
}

async function sendMatchCommand(payload: Record<string, unknown>) {
  if (!snapshot.value) return
  errorBanner.value = null
  const sess = await ensureGuestSession()
  const resp = await fetch(
    `${config.public.matchServerUrl}/api/v1/matches/${snapshot.value.match_id}/commands`,
    {
      method: 'POST',
      credentials: 'include',
      headers: {
        'content-type': 'application/json',
        authorization: `Bearer ${sess.access_token}`,
        'x-csrf-token': sess.csrf_token,
      },
      body: JSON.stringify(payload),
    },
  )
  const data = await resp.json()
  if (!resp.ok) {
    errorBanner.value = data?.error?.message || `Command rejected (${resp.status})`
    return
  }
  snapshot.value = data as MatchSnapshot
  syncClocksFromSnapshot(snapshot.value)
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

  if (!snapshot.value || snapshot.value.status !== 'playing') return
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
  const sess = await ensureGuestSession()
  const resp = await fetch(
    `${config.public.matchServerUrl}/api/v1/matches/${snapshot.value.match_id}/export.pgn`,
    {
      credentials: 'include',
      headers: {
        authorization: `Bearer ${sess.access_token}`,
      },
    },
  )
  if (!resp.ok) return
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
  const sess = await ensureGuestSession()
  const resp = await fetch(
    `${config.public.matchServerUrl}/api/v1/matches/${snapshot.value.match_id}/rematch`,
    {
      method: 'POST',
      credentials: 'include',
      headers: {
        authorization: `Bearer ${sess.access_token}`,
        'x-csrf-token': sess.csrf_token,
      },
    },
  )
  if (resp.ok) {
    const created = await resp.json()
    await router.push(`/invite/${created.invitation_code}`)
  }
}

async function savePrivacyPreferences() {
  if (!snapshot.value) return
  const sess = await ensureGuestSession()
  const resp = await fetch(
    `${config.public.matchServerUrl}/api/v1/matches/${snapshot.value.match_id}/privacy`,
    {
      method: 'POST',
      credentials: 'include',
      headers: {
        'content-type': 'application/json',
        authorization: `Bearer ${sess.access_token}`,
        'x-csrf-token': sess.csrf_token,
      },
      body: JSON.stringify({
        visibility: desiredVisibility.value,
        allow_internal_training: consentTraining.value,
        allow_published_research: consentResearch.value,
        allow_dataset_release: consentDataset.value,
      }),
    },
  )
  if (resp.ok) {
    const res = await resp.json()
    privacyStatusMsg.value = `Effective visibility: ${res.effective_visibility} (requires both players for public/unlisted)`
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
        void recoverGapAndReconnect(snapshot.value.match_id, guestSession.access_token)
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
          <button type="button" class="jade-btn jade-btn-primary" data-testid="btn-rematch" @click="startRematch">
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
        <summary>Privacy · {{ desiredVisibility === 'private' ? 'Private' : desiredVisibility === 'unlisted' ? 'Unlisted' : 'Public' }}</summary>
        <p class="privacy-note">
          Matches are private by default. Sharing and any training or research use take effect only when both players agree.
        </p>
        <label class="priv-row">
          <span>Your visibility choice</span>
          <select v-model="desiredVisibility" class="jade-select" data-testid="privacy-visibility">
            <option value="private">Private (default)</option>
            <option value="unlisted">Unlisted link</option>
            <option value="public">Public archive</option>
          </select>
        </label>
        <label class="priv-check"><input v-model="consentTraining" type="checkbox" /> <span>Allow internal model training</span></label>
        <label class="priv-check"><input v-model="consentResearch" type="checkbox" /> <span>Allow published research analysis</span></label>
        <label class="priv-check"><input v-model="consentDataset" type="checkbox" /> <span>Allow open dataset release</span></label>
        <button type="button" class="jade-btn" data-testid="save-privacy-btn" @click="savePrivacyPreferences">
          Save privacy preferences
        </button>
        <p v-if="privacyStatusMsg" class="privacy-note" data-testid="privacy-status">{{ privacyStatusMsg }}</p>
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
