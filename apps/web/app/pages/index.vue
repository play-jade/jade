<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { computeBoardView } from '~/utils/core-wasm'
import { percussionPlayer } from '~/utils/audio'
import { loadOfflineGame, saveOfflineGame } from '~/utils/offline-store'
import { capturedFaces, plateOrder, sideName, type Side } from '~/utils/side-plates'

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
// Local play extends the mainline at its end; a move from an earlier ply starts a variation, which this
// device keeps beside the game and restores after a reload. New game clears both.
const session = useReplaySession({
  source: ref({ kind: 'local', id: 'active_board', revision: 'local' }),
  rootFen: ref(STARTING_FEN),
  rulesProfile: ref(config.public.rulesProfile),
  recordedMovesUci: mainlineMovesUci,
  traditionalGlyphs,
  extendAtEnd: true,
  persist: true,
})
const { cursor: cursorPly, variation, parked, activePath: activeMovesUci } = session

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
  // The kept variation reapplies only to the game it was made on, so it is read after the game.
  await nextTick()
  await session.restoreDraft()
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
    const result = await session.tryMove(token)
    if (result.kind === 'rejected') {
      moveError.value = result.message
      return
    }
    const applied = result.applied
    selectedUci.value = null

    if (result.kind === 'extend') {
      mainlineMovesUci.value = [...mainlineMovesUci.value, result.uci]
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
  if (variation.value) {
    session.undoVariationMove()
    return
  }
  if (mainlineMovesUci.value.length > 0) {
    mainlineMovesUci.value = mainlineMovesUci.value.slice(0, -1)
    cursorPly.value = mainlineMovesUci.value.length
    // The replay session stores a kept variation again against the shorter game, or removes it with a notice.
  }
}

/** New game clears the board and the variation this device kept for the old one. */
function handleReset() {
  selectedUci.value = null
  session.discardVariation()
  mainlineMovesUci.value = []
  cursorPly.value = 0
  moveError.value = null
}

function stepToPly(target: number) {
  selectedUci.value = null
  session.selectPly(target)
}

function returnToMainLine() {
  selectedUci.value = null
  session.returnToRecordedLine()
}

function resumeVariation() {
  selectedUci.value = null
  session.resumeDraft()
}

async function exportVariation() {
  const blob = await session.exportDraft()
  if (!blob) return
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = 'jade-local-variation.json'
  link.click()
  URL.revokeObjectURL(url)
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

const sideToMove = computed<Side>(() => (boardData.value?.snapshot.side_to_move === 'black' ? 'black' : 'red'))
const plates = computed(() => {
  return plateOrder(redBelow.value).map((side) => ({
    side,
    name: sideName(side, locale.value),
    toMove: side === sideToMove.value && !boardData.value?.snapshot.adjudication,
    captured: capturedFaces(boardData.value?.snapshot.pieces ?? [], side, traditionalGlyphs.value),
  }))
})

interface IconAction {
  testid: string
  label: string
  /** SVG path data on a 20 × 20 grid. */
  paths: string[]
  disabled?: boolean
  run: () => void
}

/** The glass capsule's controls, as on macOS. */
const capsuleActions = computed<IconAction[]>(() => [
  { testid: 'btn-flip', label: t('board.flip'), paths: ['M6 3v13M6 3 3 6M6 3l3 3M14 17V4M14 17l-3-3M14 17l3-3'], run: () => (redBelow.value = !redBelow.value) },
  { testid: 'btn-undo', label: t('board.undo'), paths: ['M7 5 3.5 8.5 7 12', 'M3.5 8.5H12a4.5 4.5 0 0 1 0 9H9'], disabled: activeMovesUci.value.length === 0, run: handleUndo },
  { testid: 'btn-reset', label: t('board.new_local'), paths: ['M4 10a6 6 0 1 0 1.8-4.3', 'M4 3.5v3h3'], disabled: mainlineMovesUci.value.length === 0 && variation.value === null, run: handleReset },
  { testid: 'btn-export-offline-pgn', label: t('export.pgn'), paths: ['M10 3v9M6.5 8.5 10 12l3.5-3.5M4 14.5v2h12v-2'], disabled: activeMovesUci.value.length === 0, run: exportOfflinePgn },
])

const scrubActions = computed<IconAction[]>(() => [
  { testid: 'scrub-start', label: 'First move', paths: ['M5 4v12M15 4l-7 6 7 6'], run: () => stepToPly(0) },
  { testid: 'scrub-prev', label: 'Previous move', paths: ['M12.5 4 6.5 10l6 6'], run: () => stepToPly(cursorPly.value - 1) },
  { testid: 'scrub-next', label: 'Next move', paths: ['m7.5 4 6 6-6 6'], run: () => stepToPly(cursorPly.value + 1) },
  { testid: 'scrub-end', label: 'Last move', paths: ['M15 4v12M5 4l7 6-7 6'], run: () => stepToPly(mainlineMovesUci.value.length) },
])

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
  <div class="jade-table">
    <aside
      class="jade-plates"
      data-testid="board-status-strip"
      :aria-label="sideToMove === 'black' ? t('turn.black') : t('turn.red')"
    >
      <SidePlate
        v-for="plate in plates"
        :key="plate.side"
        :name="plate.name"
        :caption="plate.toMove ? (plate.side === 'black' ? t('turn.black') : t('turn.red')) : ''"
        :to-move="plate.toMove"
        :captured="plate.captured"
      />
      <span v-if="boardData?.snapshot.in_check_side" class="check-note" data-testid="check-badge">
        {{ t('state.check') }}
      </span>
    </aside>

    <div class="jade-table-stage">
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
      <div class="jade-capsule" role="toolbar" aria-label="Board">
        <IconButton
          v-for="a in capsuleActions"
          :key="a.testid"
          :data-testid="a.testid"
          :label="a.label"
          :paths="a.paths"
          :disabled="a.disabled"
          @click="a.run"
        />
      </div>

      <p v-if="moveError" class="error-note">{{ moveError }}</p>
    </div>

    <aside class="jade-ledger">
      <section class="moves" data-testid="move-list-card" aria-label="Moves">
        <ReplayVariationBar
          :variation="variation"
          :parked="parked"
          :save-state="session.saveState.value"
          :will-replace-draft="session.willReplaceDraft.value"
          :draft-notice="session.draftNotice.value"
          :feedback="session.feedback.value"
          :recorded-moves-uci="mainlineMovesUci"
          :root-fen="STARTING_FEN"
          @return-to-game="returnToMainLine"
          @resume="resumeVariation"
          @export="exportVariation"
        />

        <ol v-if="displayedMoves.length > 0" class="jade-move-list" data-testid="move-list">
          <li v-for="row in moveRows" :key="row.number" class="jade-move-row">
            <span class="jade-move-number">{{ row.number }}</span>
            <button
              v-for="m in row.plies"
              :key="`${m.ply}-${m.uci}`"
              type="button"
              class="jade-move"
              :class="{ current: m.ply === activeMovesUci.length }"
              :disabled="variation !== null"
              @click="stepToPly(m.ply)"
            >
              {{ m.label }}
            </button>
          </li>
        </ol>
        <p v-else class="empty-moves">Your moves will appear here.</p>

        <div class="scrub jade-rule" aria-label="Replay">
          <IconButton
            v-for="a in scrubActions"
            :key="a.testid"
            :data-testid="a.testid"
            :label="a.label"
            :paths="a.paths"
            :size="16"
            @click="a.run"
          />
        </div>
      </section>

      <EngineAnalysisCard
        :fen="STARTING_FEN"
        :moves-uci="activeMovesUci"
        :state-id="session.stateId.value"
        @play-engine-move="(uci, stateId) => stateId === session.stateId.value && handlePlayMove(uci.slice(0, 2), uci.slice(2, 4))"
      />

      <section class="invite jade-rule" data-testid="play-friend-card" aria-label="Play a friend">
        <div class="invite-options">
          <label>
            <span class="sr-only">Clock</span>
            <select v-model="clockPreset" data-testid="invite-clock-select" class="jade-select">
              <option value="5+3">{{ t('clock.5_plus_3') }}</option>
              <option value="10+5">{{ t('clock.10_plus_5') }}</option>
              <option value="15+10">{{ t('clock.15_plus_10') }}</option>
              <option value="untimed">{{ t('clock.untimed') }}</option>
            </select>
          </label>
          <label>
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
.check-note {
  font-size: 0.8rem;
  color: var(--jade-vermilion-text);
}

.verdict-banner {
  align-self: stretch;
  text-align: center;
  font-family: var(--jade-display);
  font-size: 1.25rem;
}

.moves {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 10px;
  min-height: 160px;
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
}

.invite {
  display: flex;
  flex-direction: column;
  gap: 8px;
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

@media (max-width: 1100px) {
  .moves {
    justify-content: flex-start;
    min-height: 0;
  }
}
</style>
