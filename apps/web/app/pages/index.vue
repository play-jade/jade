<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { applyCoreMove, computeBoardView } from '~/utils/core-wasm'
import { percussionPlayer } from '~/utils/audio'
import { loadOfflineGame, saveOfflineGame } from '~/utils/offline-store'

const STARTING_FEN = 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1'

const config = useRuntimeConfig()
const router = useRouter()
const {
  locale,
  material,
  darkSurface,
  notationMode,
  soundEnabled,
  reducedMotion,
  traditionalGlyphs,
  t,
} = usePreferences()

const redBelow = ref(true)
const selectedUci = ref<string | null>(null)
const mainlineMovesUci = ref<string[]>([])
const cursorPly = ref(0)
// Optional exploration branch tried out from a replay cursor
const branchOriginPly = ref<number | null>(null)
const branchMovesUci = ref<string[]>([])

const lastAnnouncement = ref('')
const moveError = ref<string | null>(null)

// Friendly online invitation form
const clockPreset = ref<'10+5' | 'untimed'>('10+5')
const preferredSide = ref<'red' | 'black'>('red')
const creatingInvite = ref(false)
const inviteError = ref<string | null>(null)

const activeMovesUci = computed(() => {
  if (branchOriginPly.value !== null) {
    return [...mainlineMovesUci.value.slice(0, branchOriginPly.value), ...branchMovesUci.value]
  }
  return mainlineMovesUci.value.slice(0, cursorPly.value)
})

const { data: boardData, refresh: refreshBoard } = await useAsyncData(
  'home-board-view',
  () =>
    computeBoardView({
      fen: STARTING_FEN,
      movesUci: activeMovesUci.value,
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
    watch: [activeMovesUci, redBelow, selectedUci, traditionalGlyphs, material, darkSurface],
  },
)

onMounted(async () => {
  // A server render that could not compute the board leaves no data to hydrate, so the client computes it.
  if (!boardData.value) await refreshBoard()
  const saved = await loadOfflineGame('active_board')
  if (saved && saved.movesUci.length > 0 && mainlineMovesUci.value.length === 0) {
    mainlineMovesUci.value = saved.movesUci
    cursorPly.value = saved.movesUci.length
  }
})

watch(
  mainlineMovesUci,
  (moves) => {
    void saveOfflineGame({
      id: 'active_board',
      initialFen: STARTING_FEN,
      movesUci: [...moves],
      updatedAtMs: Date.now(),
    })
  },
  { deep: true },
)

async function handleSelectSquare(sq: string | null) {
  moveError.value = null
  selectedUci.value = sq
}

async function handlePlayMove(fromUci: string, toUci: string) {
  moveError.value = null
  const token = `${fromUci}${toUci}`
  const priorPieceCount = boardData.value?.snapshot.pieces.length ?? 32
  try {
    const applied = await applyCoreMove({
      fen: STARTING_FEN,
      movesUci: activeMovesUci.value,
      token,
      traditionalGlyphs: traditionalGlyphs.value,
    })
    selectedUci.value = null

    if (branchOriginPly.value !== null) {
      branchMovesUci.value = [...branchMovesUci.value, applied.move_uci]
    } else if (cursorPly.value < mainlineMovesUci.value.length) {
      // Playing from an earlier replay ply starts a non-destructive exploration branch!
      branchOriginPly.value = cursorPly.value
      branchMovesUci.value = [applied.move_uci]
    } else {
      mainlineMovesUci.value = [...mainlineMovesUci.value, applied.move_uci]
      cursorPly.value = mainlineMovesUci.value.length
    }

    await refreshBoard()
    const afterPieceCount = boardData.value?.snapshot.pieces.length ?? priorPieceCount
    const isCapture = afterPieceCount < priorPieceCount
    const soundKind = applied.in_check ? 'check' : isCapture ? 'capture' : 'place'
    void percussionPlayer.play(material.value, soundKind, soundEnabled.value, false)

    lastAnnouncement.value = `${applied.move_chinese} (${applied.move_uci})${
      applied.in_check ? `, ${t('state.check')}` : ''
    }`
  } catch (err) {
    moveError.value = err instanceof Error ? err.message : String(err)
  }
}

function handleUndo() {
  selectedUci.value = null
  if (branchOriginPly.value !== null) {
    if (branchMovesUci.value.length > 1) {
      branchMovesUci.value = branchMovesUci.value.slice(0, -1)
    } else {
      branchOriginPly.value = null
      branchMovesUci.value = []
    }
    return
  }
  if (mainlineMovesUci.value.length > 0) {
    mainlineMovesUci.value = mainlineMovesUci.value.slice(0, -1)
    cursorPly.value = mainlineMovesUci.value.length
  }
}

function handleReset() {
  selectedUci.value = null
  branchOriginPly.value = null
  branchMovesUci.value = []
  mainlineMovesUci.value = []
  cursorPly.value = 0
  moveError.value = null
}

function stepToPly(target: number) {
  selectedUci.value = null
  branchOriginPly.value = null
  branchMovesUci.value = []
  cursorPly.value = Math.max(0, Math.min(mainlineMovesUci.value.length, target))
}

function exitBranch() {
  branchOriginPly.value = null
  branchMovesUci.value = []
}

async function createFriendlyInvitation() {
  creatingInvite.value = true
  inviteError.value = null
  try {
    const base = config.public.matchServerUrl
    let session = null
    if (typeof sessionStorage !== 'undefined') {
      const raw = sessionStorage.getItem('jade_guest_session')
      if (raw) session = JSON.parse(raw)
    }
    if (!session) {
      const guestResp = await fetch(`${base}/api/v1/auth/guest`, {
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ display_name: 'Jade Guest', transport: 'web_cookie' }),
      })
      if (!guestResp.ok) throw new Error(`Guest session failed (${guestResp.status})`)
      session = await guestResp.json()
      sessionStorage?.setItem('jade_guest_session', JSON.stringify(session))
    }

    const invResp = await fetch(`${base}/api/v1/invitations`, {
      method: 'POST',
      credentials: 'include',
      headers: {
        'content-type': 'application/json',
        'authorization': `Bearer ${session.access_token}`,
        'x-csrf-token': session.csrf_token,
      },
      body: JSON.stringify({
        preferred_side: preferredSide.value,
        clock_initial_ms: clockPreset.value === '10+5' ? 600_000 : null,
        clock_increment_ms: clockPreset.value === '10+5' ? 5_000 : 0,
        is_friendly: true,
      }),
    })
    if (!invResp.ok) throw new Error(`Invitation creation failed (${invResp.status})`)
    const created = await invResp.json()
    await router.push(`/invite/${created.invitation_code}`)
  } catch (err) {
    inviteError.value = err instanceof Error ? err.message : String(err)
  } finally {
    creatingInvite.value = false
  }
}

const displayedMoves = computed(() => {
  const zh = boardData.value?.moves_chinese ?? []
  const uci = activeMovesUci.value
  return uci.map((u, idx) => ({
    ply: idx + 1,
    label: notationMode.value === 'chinese' ? zh[idx] || u : u,
    uci: u,
  }))
})

const verdictBanner = computed(() => {
  const adj = boardData.value?.snapshot.adjudication
  if (!adj) return null
  if (adj.reason === 'checkmate') return `${t('outcome.checkmate')} — ${adj.detail}`
  if (adj.reason === 'stalemate') return `${t('outcome.no_legal_moves_loss')} — ${adj.detail}`
  if (adj.reason === 'perpetual_check') return `${t('outcome.perpetual_check_loss')} — ${adj.detail}`
  if (adj.reason === 'perpetual_chase') return `${t('outcome.perpetual_chase_loss')} — ${adj.detail}`
  return `${adj.outcome}: ${adj.detail}`
})
</script>

<template>
  <div class="home-grid">
    <div class="board-column">
      <div class="board-status-strip" data-testid="board-status-strip">
        <span class="turn-indicator" :class="`side-${boardData?.snapshot.side_to_move}`">
          {{ boardData?.snapshot.side_to_move === 'black' ? t('turn.black') : t('turn.red') }}
        </span>
        <span v-if="boardData?.snapshot.in_check_side" class="check-pill" data-testid="check-badge">
          {{ t('state.check') }}
        </span>
        <span class="rules-pill">Rules: {{ config.public.rulesProfile }}</span>
      </div>

      <div v-if="verdictBanner" class="verdict-banner" data-testid="verdict-banner">
        {{ verdictBanner }}
      </div>

      <XiangqiBoard
        v-if="boardData"
        :snapshot="boardData.snapshot"
        :material="material"
        :traditional-glyphs="traditionalGlyphs"
        :dark-surface="darkSurface"
        :locale="locale"
        :last-announcement="lastAnnouncement"
        @select-square="handleSelectSquare"
        @play-move="handlePlayMove"
      />

      <div class="board-toolbar">
        <button type="button" class="jade-btn" data-testid="btn-flip" @click="redBelow = !redBelow">
          {{ t('board.flip') }}
        </button>
        <button
          type="button"
          class="jade-btn"
          data-testid="btn-undo"
          :disabled="activeMovesUci.length === 0"
          @click="handleUndo"
        >
          {{ t('board.undo') }}
        </button>
        <button
          type="button"
          class="jade-btn"
          data-testid="btn-reset"
          :disabled="mainlineMovesUci.length === 0 && branchOriginPly === null"
          @click="handleReset"
        >
          {{ t('board.new_local') }}
        </button>
      </div>

      <p v-if="moveError" class="error-note">{{ moveError }}</p>
    </div>

    <aside class="side-column">
      <!-- Friendly Online Match Invitation Card -->
      <section class="jade-card invite-card" data-testid="play-friend-card">
        <h2 class="section-heading">{{ t('action.play_friend') }}</h2>
        <p class="section-sub">{{ t('product.tagline') }}</p>

        <div class="invite-options">
          <label class="option-row">
            <span>Clock</span>
            <select v-model="clockPreset" data-testid="invite-clock-select" class="jade-select">
              <option value="10+5">{{ t('clock.10_plus_5') }}</option>
              <option value="untimed">{{ t('clock.untimed') }}</option>
            </select>
          </label>

          <label class="option-row">
            <span>Your seat</span>
            <select v-model="preferredSide" data-testid="invite-side-select" class="jade-select">
              <option value="red">Red (红方先手)</option>
              <option value="black">Black (黑方后手)</option>
            </select>
          </label>
        </div>

        <button
          type="button"
          class="jade-btn jade-btn-primary full-width-btn"
          data-testid="create-invite-btn"
          :disabled="creatingInvite"
          @click="createFriendlyInvitation"
        >
          {{ creatingInvite ? 'Creating link…' : `${t('action.play_friend')} →` }}
        </button>
        <p v-if="inviteError" class="error-note">{{ inviteError }}</p>
      </section>

      <!-- Move Record, Replay Scrub & Variation Tryout -->
      <section class="jade-card moves-card" data-testid="move-list-card">
        <div class="moves-header">
          <h2 class="section-heading">{{ t('study.title') }} · Offline Board</h2>
          <div class="scrub-controls">
            <button type="button" class="jade-pill-btn" data-testid="scrub-start" @click="stepToPly(0)">
              |&lt;
            </button>
            <button
              type="button"
              class="jade-pill-btn"
              data-testid="scrub-prev"
              @click="stepToPly(cursorPly - 1)"
            >
              &lt;
            </button>
            <button
              type="button"
              class="jade-pill-btn"
              data-testid="scrub-next"
              @click="stepToPly(cursorPly + 1)"
            >
              &gt;
            </button>
            <button
              type="button"
              class="jade-pill-btn"
              data-testid="scrub-end"
              @click="stepToPly(mainlineMovesUci.length)"
            >
              &gt;|
            </button>
          </div>
        </div>

        <div v-if="branchOriginPly !== null" class="branch-banner" data-testid="branch-banner">
          <span>Exploring alternative line from ply {{ branchOriginPly }}</span>
          <button type="button" class="jade-pill-btn" data-testid="exit-branch-btn" @click="exitBranch">
            Return to main line
          </button>
        </div>

        <ol v-if="displayedMoves.length > 0" class="move-list" data-testid="move-list">
          <li
            v-for="m in displayedMoves"
            :key="`${m.ply}-${m.uci}`"
            class="move-chip"
            @click="branchOriginPly === null && stepToPly(m.ply)"
          >
            <span class="ply-num">{{ m.ply }}.</span>
            <span>{{ m.label }}</span>
          </li>
        </ol>
        <p v-else class="empty-moves">
          Tap or drag any Red piece to play same-device or offline. Progress saves locally on this device.
        </p>
      </section>

      <!-- Local Browser Web Worker Engine Analysis -->
      <EngineAnalysisCard
        :fen="STARTING_FEN"
        :moves-uci="activeMovesUci"
        :state-id="`home:${activeMovesUci.join(',')}`"
        @play-engine-move="(uci) => handlePlayMove(uci.slice(0, 2), uci.slice(2, 4))"
      />
    </aside>
  </div>
</template>

<style scoped>
.home-grid {
  display: grid;
  grid-template-columns: minmax(300px, 540px) minmax(290px, 1fr);
  gap: 24px;
  align-items: start;
}

@media (max-width: 880px) {
  .home-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.board-column {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.board-status-strip {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 0.9rem;
}

.turn-indicator {
  font-weight: 650;
}

.turn-indicator.side-red {
  color: #A54837;
}

.check-pill {
  background: #A54837;
  color: #F3EEE4;
  padding: 2px 9px;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 650;
}

.rules-pill {
  font-size: 0.78rem;
  opacity: 0.72;
}

.verdict-banner {
  padding: 10px 14px;
  border-radius: 8px;
  background: rgba(165, 72, 55, 0.14);
  border: 1px solid rgba(165, 72, 55, 0.45);
  font-weight: 600;
}

.board-toolbar {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: 8px;
}

.side-column {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.section-heading {
  margin: 0;
  font-size: 1.05rem;
}

.section-sub {
  margin: 4px 0 12px;
  font-size: 0.88rem;
  opacity: 0.8;
}

.invite-options {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 12px;
}

.option-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 0.88rem;
}

.full-width-btn {
  width: 100%;
  padding: 9px 14px;
}

.moves-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}

.scrub-controls {
  display: inline-flex;
  gap: 4px;
}

.branch-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  margin-bottom: 8px;
  border-radius: 6px;
  background: rgba(65, 84, 71, 0.14);
  font-size: 0.82rem;
}

.move-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-height: 190px;
  overflow-y: auto;
}

.move-chip {
  padding: 4px 8px;
  border-radius: 5px;
  background: rgba(65, 84, 71, 0.1);
  font-size: 0.85rem;
  cursor: pointer;
}

.ply-num {
  opacity: 0.65;
  margin-right: 4px;
}

.empty-moves {
  margin: 0;
  font-size: 0.85rem;
  opacity: 0.75;
}

.error-note {
  margin: 6px 0 0;
  font-size: 0.82rem;
  color: #A54837;
}
</style>
