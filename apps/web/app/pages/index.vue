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
const clockPreset = ref<'5+3' | '10+5' | '15+10' | 'untimed'>('10+5')
const preferredSide = ref<'red' | 'black' | 'random'>('red')
const creatingInvite = ref(false)
const inviteError = ref<string | null>(null)

function resolveClockConfig(preset: '5+3' | '10+5' | '15+10' | 'untimed') {
  switch (preset) {
    case '5+3':
      return { initial: 300_000, inc: 3_000 }
    case '10+5':
      return { initial: 600_000, inc: 5_000 }
    case '15+10':
      return { initial: 900_000, inc: 10_000 }
    case 'untimed':
      return { initial: null, inc: 0 }
  }
}

function exportOfflinePgn() {
  if (typeof window === 'undefined') return
  const zh = boardData.value?.moves_chinese ?? []
  const lines = [
    '[Event "Jade Offline Xiangqi Study"]',
    `[RulesProfile "${config.public.rulesProfile}"]`,
    `[FEN "${STARTING_FEN}"]`,
    '[Result "*"]',
    '',
  ]
  const bodyTokens: string[] = []
  activeMovesUci.value.forEach((u, idx) => {
    const z = zh[idx] || ''
    if (idx % 2 === 0) bodyTokens.push(`${Math.floor(idx / 2) + 1}. ${u} {${z}}`)
    else bodyTokens.push(`${u} {${z}}`)
  })
  bodyTokens.push('*')
  lines.push(bodyTokens.join(' '))
  const blob = new Blob([lines.join('\n') + '\n'], { type: 'application/x-chess-pgn;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = 'jade-offline-game.pgn'
  a.click()
  URL.revokeObjectURL(url)
}

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
        preferred_side: preferredSide.value === 'random' ? undefined : preferredSide.value,
        preferred_side_mode: preferredSide.value,
        clock_initial_ms: resolveClockConfig(clockPreset.value).initial,
        clock_increment_ms: resolveClockConfig(clockPreset.value).inc,
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

const { labels: moveLabels } = useNotatedMoves(activeMovesUci)

const displayedMoves = computed(() => {
  const uci = activeMovesUci.value
  return uci.map((u, idx) => ({
    ply: idx + 1,
    label: moveLabels.value[idx] ?? u,
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

// Side plates: the large side name, whose turn it is, and the pieces each side has lost (as on macOS).
type Side = 'red' | 'black'
const SIDE_NAMES: Record<Side, { en: string; hans: string; hant: string }> = {
  red: { en: 'Red', hans: '红方', hant: '紅方' },
  black: { en: 'Black', hans: '黑方', hant: '黑方' },
}
function sideName(side: Side): string {
  const names = SIDE_NAMES[side]
  if (locale.value === 'zh-Hans-CN') return names.hans
  if (locale.value.startsWith('zh')) return names.hant
  return names.en
}

const STARTING_COUNTS: Record<string, number> = {
  chariot: 2, horse: 2, cannon: 2, elephant: 2, advisor: 2, soldier: 5, general: 1,
}
const FACES: Record<'simplified' | 'traditional', Record<Side, Record<string, string>>> = {
  simplified: {
    red: { chariot: '车', horse: '马', cannon: '炮', elephant: '相', advisor: '仕', soldier: '兵', general: '帅' },
    black: { chariot: '车', horse: '马', cannon: '炮', elephant: '象', advisor: '士', soldier: '卒', general: '将' },
  },
  traditional: {
    red: { chariot: '車', horse: '馬', cannon: '炮', elephant: '相', advisor: '仕', soldier: '兵', general: '帥' },
    black: { chariot: '車', horse: '馬', cannon: '砲', elephant: '象', advisor: '士', soldier: '卒', general: '將' },
  },
}

function capturedFaces(side: Side): string[] {
  const pieces = boardData.value?.snapshot.pieces ?? []
  const faces = FACES[traditionalGlyphs.value ? 'traditional' : 'simplified'][side]
  const out: string[] = []
  for (const role of Object.keys(STARTING_COUNTS)) {
    const left = pieces.filter((p) => p.side === side && p.role === role).length
    for (let i = left; i < STARTING_COUNTS[role]; i++) out.push(faces[role])
  }
  return out
}

const sideToMove = computed<Side>(() => (boardData.value?.snapshot.side_to_move === 'black' ? 'black' : 'red'))
const plates = computed(() => {
  const order: Side[] = redBelow.value ? ['black', 'red'] : ['red', 'black']
  return order.map((side) => ({
    side,
    name: sideName(side),
    toMove: side === sideToMove.value && !boardData.value?.snapshot.adjudication,
    captured: capturedFaces(side),
  }))
})

/** Moves paired by move number, Red's ply then Black's. */
const moveRows = computed(() => {
  const rows: { number: number; plies: typeof displayedMoves.value }[] = []
  displayedMoves.value.forEach((m, idx) => {
    if (idx % 2 === 0) rows.push({ number: idx / 2 + 1, plies: [] })
    rows[rows.length - 1].plies.push(m)
  })
  return rows
})
</script>

<template>
  <div class="table-layout">
    <aside class="plates" data-testid="board-status-strip" :aria-label="sideToMove === 'black' ? t('turn.black') : t('turn.red')">
      <div v-for="plate in plates" :key="plate.side" class="plate" :class="{ 'to-move': plate.toMove }">
        <div class="plate-caption">
          <span class="plate-dot" aria-hidden="true" />
          <span>{{ plate.toMove ? (plate.side === 'black' ? t('turn.black') : t('turn.red')) : ' ' }}</span>
        </div>
        <div class="plate-name">{{ plate.name }}</div>
        <div v-if="plate.captured.length" class="plate-captured" :aria-label="`${plate.name}: ${plate.captured.length} captured`">
          {{ plate.captured.join(' ') }}
        </div>
      </div>
      <span v-if="boardData?.snapshot.in_check_side" class="check-note" data-testid="check-badge">
        {{ t('state.check') }}
      </span>
    </aside>

    <div class="board-stage">
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

      <!-- The only on-board controls: one glass capsule, as on macOS. -->
      <div class="capsule" role="toolbar" aria-label="Board">
        <button type="button" class="jade-icon-btn" data-testid="btn-flip" :aria-label="t('board.flip')" :title="t('board.flip')" @click="redBelow = !redBelow">
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3v13M6 3 3 6M6 3l3 3M14 17V4M14 17l-3-3M14 17l3-3" /></svg>
        </button>
        <button
          type="button"
          class="jade-icon-btn"
          data-testid="btn-undo"
          :aria-label="t('board.undo')"
          :title="t('board.undo')"
          :disabled="activeMovesUci.length === 0"
          @click="handleUndo"
        >
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 5 3.5 8.5 7 12" /><path d="M3.5 8.5H12a4.5 4.5 0 0 1 0 9H9" /></svg>
        </button>
        <button
          type="button"
          class="jade-icon-btn"
          data-testid="btn-reset"
          :aria-label="t('board.new_local')"
          :title="t('board.new_local')"
          :disabled="mainlineMovesUci.length === 0 && branchOriginPly === null"
          @click="handleReset"
        >
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 10a6 6 0 1 0 1.8-4.3" /><path d="M4 3.5v3h3" /></svg>
        </button>
        <button
          type="button"
          class="jade-icon-btn"
          data-testid="btn-export-offline-pgn"
          :aria-label="t('export.pgn')"
          :title="t('export.pgn')"
          :disabled="activeMovesUci.length === 0"
          @click="exportOfflinePgn"
        >
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3v9M6.5 8.5 10 12l3.5-3.5M4 14.5v2h12v-2" /></svg>
        </button>
      </div>

      <p v-if="moveError" class="error-note">{{ moveError }}</p>
    </div>

    <aside class="ledger">
      <section class="moves" data-testid="move-list-card" aria-label="Moves">
        <div v-if="branchOriginPly !== null" class="branch-banner" data-testid="branch-banner">
          <span>Trying a line from ply {{ branchOriginPly }}</span>
          <button type="button" class="jade-pill-btn" data-testid="exit-branch-btn" @click="exitBranch">
            Return to main line
          </button>
        </div>

        <ol v-if="displayedMoves.length > 0" class="move-list" data-testid="move-list">
          <li v-for="row in moveRows" :key="row.number" class="move-row">
            <span class="move-number">{{ row.number }}</span>
            <button
              v-for="m in row.plies"
              :key="`${m.ply}-${m.uci}`"
              type="button"
              class="move"
              :class="{ current: m.ply === activeMovesUci.length }"
              :disabled="branchOriginPly !== null"
              @click="stepToPly(m.ply)"
            >
              {{ m.label }}
            </button>
          </li>
        </ol>
        <p v-else class="empty-moves">Your moves will appear here.</p>

        <div class="scrub" aria-label="Replay">
          <button type="button" class="jade-icon-btn" data-testid="scrub-start" aria-label="First move" @click="stepToPly(0)">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 4v12M15 4l-7 6 7 6" /></svg>
          </button>
          <button type="button" class="jade-icon-btn" data-testid="scrub-prev" aria-label="Previous move" @click="stepToPly(cursorPly - 1)">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12.5 4 6.5 10l6 6" /></svg>
          </button>
          <button type="button" class="jade-icon-btn" data-testid="scrub-next" aria-label="Next move" @click="stepToPly(cursorPly + 1)">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m7.5 4 6 6-6 6" /></svg>
          </button>
          <button type="button" class="jade-icon-btn" data-testid="scrub-end" aria-label="Last move" @click="stepToPly(mainlineMovesUci.length)">
            <svg width="16" height="16" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 4v12M5 4l7 6-7 6" /></svg>
          </button>
        </div>
      </section>

      <EngineAnalysisCard
        :fen="STARTING_FEN"
        :moves-uci="activeMovesUci"
        :state-id="`home:${activeMovesUci.join(',')}`"
        @play-engine-move="(uci) => handlePlayMove(uci.slice(0, 2), uci.slice(2, 4))"
      />

      <section class="invite" data-testid="play-friend-card" aria-label="Play a friend">
        <div class="invite-options">
          <label class="option">
            <span class="sr-only">Clock</span>
            <select v-model="clockPreset" data-testid="invite-clock-select" class="jade-select">
              <option value="5+3">{{ t('clock.5_plus_3') }}</option>
              <option value="10+5">{{ t('clock.10_plus_5') }}</option>
              <option value="15+10">{{ t('clock.15_plus_10') }}</option>
              <option value="untimed">{{ t('clock.untimed') }}</option>
            </select>
          </label>
          <label class="option">
            <span class="sr-only">Your side</span>
            <select v-model="preferredSide" data-testid="invite-side-select" class="jade-select">
              <option value="red">Play Red</option>
              <option value="black">Play Black</option>
              <option value="random">Random side</option>
            </select>
          </label>
        </div>
        <button
          type="button"
          class="jade-btn jade-btn-primary invite-btn"
          data-testid="create-invite-btn"
          :disabled="creatingInvite"
          @click="createFriendlyInvitation"
        >
          {{ creatingInvite ? 'Creating link…' : t('action.play_friend') }}
        </button>
        <p v-if="inviteError" class="error-note">{{ inviteError }}</p>
      </section>
    </aside>
  </div>
</template>

<style scoped>
/* Lamplit table: side plates, the board as the lit object, and a quiet ledger of moves. */
.table-layout {
  display: grid;
  grid-template-columns: minmax(180px, 240px) minmax(0, 720px) minmax(240px, 300px);
  justify-content: center;
  gap: 40px;
  align-items: stretch;
  min-height: calc(100vh - 110px);
}

.plates {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  padding: 24px 0 72px;
}

.plate {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.plate-caption {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 0.72rem;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--jade-muted);
}

.plate-dot {
  width: 8px;
  height: 8px;
  border-radius: 4px;
  background: var(--jade-hairline);
  transition: background 180ms ease, box-shadow 180ms ease;
}

.plate.to-move .plate-caption {
  color: var(--jade-pale);
}

.plate.to-move .plate-dot {
  background: var(--jade-pale);
  box-shadow: 0 0 12px 2px rgba(220, 227, 206, 0.55);
}

.plate-name {
  font-family: var(--jade-display);
  font-size: clamp(2.6rem, 4.2vw, 3.6rem);
  line-height: 1;
  color: var(--jade-muted);
  transition: color 180ms ease;
}

.plate.to-move .plate-name {
  color: var(--jade-paper);
}

.plate-captured {
  font-family: 'Noto Serif SC', 'Songti SC', serif;
  font-size: 1.05rem;
  letter-spacing: 0.08em;
  color: var(--jade-muted);
}

.check-note {
  font-size: 0.8rem;
  color: var(--jade-vermilion-text);
}

/* The slab fits the window height with the capsule below it, as macOS fits it to the window. */
.board-stage {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 18px;
  padding-top: 8px;
  min-width: 0;
  width: 100%;
  max-width: min(720px, calc((100vh - 170px) / 1.11));
  justify-self: center;
}

.capsule {
  display: inline-flex;
  gap: 4px;
  padding: 4px 8px;
  border-radius: 999px;
  background: rgba(29, 36, 33, 0.62);
  backdrop-filter: blur(24px) saturate(140%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  box-shadow: inset 0 0 0 1px var(--jade-hairline), 0 10px 30px rgba(0, 0, 0, 0.4);
}

.capsule .jade-icon-btn {
  width: 44px;
  height: 44px;
  border-radius: 22px;
}

.verdict-banner {
  align-self: stretch;
  text-align: center;
  font-family: var(--jade-display);
  font-size: 1.25rem;
  color: var(--jade-paper);
}

.ledger {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 24px 0 72px;
  min-width: 0;
}

.moves {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 10px;
  min-height: 160px;
}

.move-list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 46vh;
  overflow-y: auto;
  font-variant-numeric: tabular-nums;
}

.move-row {
  display: grid;
  grid-template-columns: 2.2em minmax(0, 1fr) minmax(0, 1fr);
  align-items: baseline;
}

.move-number {
  font-size: 0.75rem;
  color: var(--jade-muted);
}

.move {
  appearance: none;
  border: 0;
  border-radius: 6px;
  padding: 6px 8px;
  background: transparent;
  color: rgba(243, 238, 228, 0.82);
  font: inherit;
  font-size: 0.95rem;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
}

.move:hover:not(:disabled):not(.current) {
  background: var(--jade-surface-hover);
}

.move.current {
  color: var(--jade-paper);
  font-weight: 600;
  text-decoration: underline;
  text-decoration-color: rgba(220, 227, 206, 0.6);
  text-underline-offset: 6px;
}

.move:disabled {
  cursor: default;
}

.empty-moves {
  margin: 0;
  font-size: 0.85rem;
  color: var(--jade-muted);
}

.scrub {
  display: flex;
  gap: 2px;
  padding-top: 8px;
  border-top: 1px solid var(--jade-hairline);
}

.branch-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 0.8rem;
  color: var(--jade-pale);
}

.invite {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 14px;
  border-top: 1px solid var(--jade-hairline);
}

.invite-options {
  display: grid;
  grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
  gap: 8px;
}

.invite-options .jade-select {
  width: 100%;
}

.invite-btn {
  width: 100%;
  padding: 10px 14px;
  font-size: 0.95rem;
}

.error-note {
  margin: 0;
  font-size: 0.82rem;
  color: var(--jade-vermilion-text);
}

/* Narrow windows: the board stays the main object; plates sit above and below it and the ledger follows. */
@media (max-width: 1100px) {
  .table-layout {
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    min-height: 0;
  }

  .plates {
    flex-direction: row;
    padding: 0;
    gap: 16px;
  }

  .plate-name {
    font-size: 2rem;
  }

  .ledger {
    padding: 0;
  }

  .moves {
    justify-content: flex-start;
    min-height: 0;
  }
}
</style>
