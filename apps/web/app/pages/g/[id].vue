<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { parsePlyQuery } from '~/composables/useReplaySession'
import { computeBoardView } from '~/utils/core-wasm'
import { capturedFaces, pairMoves, plateOrder, sideName } from '~/utils/side-plates'
import { matchServerBase, useBrowserSession } from '~/composables/useBrowserSession'

const STARTING_FEN = 'rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1'

interface MatchSnapshot {
  match_id: string
  invitation_code: string
  rules_profile: string
  status: string
  seq: number
  fen: string
  side_to_move: 'red' | 'black'
  moves_uci: string[]
  moves_chinese: string[]
  outcome: string | null
  reason_code: string | null
  visibility: 'private' | 'unlisted' | 'public'
  next_rematch_invitation_code?: string | null
}

const route = useRoute()
const router = useRouter()
const config = useRuntimeConfig()
const {
  locale,
  material,
  darkSurface,
  reducedMotion,
  traditionalGlyphs,
  t,
} = usePreferences()

const matchId = computed(() => String(route.params.id || ''))
const snapshot = ref<MatchSnapshot | null>(null)
const { labels: moveLabels } = useNotatedMoves(computed(() => snapshot.value?.moves_uci ?? []))
const accessDenied = ref(false)
const loadError = ref<string | null>(null)

const redBelow = ref(true)
const selectedUci = ref<string | null>(null)
// The guest who owns this private match's local variation; set from the browser session on mount.
const ownerScope = ref<string | null>(null)
/** Why this replay keeps no variation, when it keeps none: no session, an ended one, or one that could not be checked. */
const noSessionReason = ref<string | null>('This browser has no current guest session, so the variation is not kept on this device')
/** False once the recovered session's expiry passes: later variations are then reported as not kept. */
const sessionLive = ref(false)
let sessionEnd: ReturnType<typeof setTimeout> | null = null
onBeforeUnmount(() => {
  if (sessionEnd) clearTimeout(sessionEnd)
})

const session = useReplaySession({
  source: computed(() =>
    snapshot.value ? { kind: 'match', id: snapshot.value.match_id, revision: String(snapshot.value.seq) } : null,
  ),
  rootFen: ref(STARTING_FEN),
  rulesProfile: computed(() => snapshot.value?.rules_profile ?? config.public.rulesProfile),
  recordedMovesUci: computed(() => snapshot.value?.moves_uci ?? []),
  ownerScope,
  traditionalGlyphs,
  // A private match's variation is kept only for a known guest; otherwise it stays in this tab.
  persist: computed(() => ownerScope.value !== null && sessionLive.value),
  unpersistedReason: computed(() => (ownerScope.value !== null && sessionLive.value ? null : noSessionReason.value)),
})
const { cursor: cursorPly, variation, activePath: activeMovesUci } = session

const { data: boardData, refresh: refreshBoard } = await useAsyncData(
  () => `replay-match-${matchId.value}`,
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
  try {
    // The guest this browser's cookie names, checked afresh and never created: a replay makes no new guest. A check
    // that fails keeps no variation, rather than claiming a save for a guest it could not confirm.
    const current = await useBrowserSession()
      .recover({ fresh: true })
      .catch(() => {
        noSessionReason.value = 'The match server could not confirm your guest session, so the variation is not kept'
        return null
      })
    ownerScope.value = current?.account_id ?? null
    if (current) {
      sessionLive.value = true
      // Saving stops when the session ends; the line on screen stays, and later variations say they were not kept.
      sessionEnd = setTimeout(() => {
        noSessionReason.value = 'Your guest session has ended, so the variation is no longer kept on this device'
        sessionLive.value = false
      }, Math.max(0, current.expires_at_ms - Date.now()))
    }
    const resp = await fetch(`${matchServerBase()}/api/v1/matches/${matchId.value}`, {
      credentials: 'include',
    })
    if (resp.status === 403 || resp.status === 401) {
      accessDenied.value = true
      return
    }
    if (!resp.ok) {
      loadError.value = `Match not found (${resp.status})`
      return
    }
    const data = (await resp.json()) as MatchSnapshot
    snapshot.value = data
    const linkedPly = parsePlyQuery(route.query.ply)
    await nextTick()
    session.selectPly(linkedPly ?? data.moves_uci.length)
    await session.restoreDraft({ keepPosition: linkedPly !== null })
    await refreshBoard()
  } catch (err) {
    loadError.value = err instanceof Error ? err.message : String(err)
  }
})

function stepToPly(ply: number) {
  if (!snapshot.value) return
  selectedUci.value = null
  session.selectPly(ply)
  void router.replace({ query: { ...route.query, ply: String(cursorPly.value) } })
}

// Replay exploration is local: it validates through the core and never sends a match command.
async function handleTryMove(fromUci: string, toUci: string) {
  selectedUci.value = null
  await session.tryMove(`${fromUci}${toUci}`)
}

function handleEngineMove(uci: string, stateId: string) {
  if (stateId !== session.stateId.value) return
  selectedUci.value = null
  void session.tryMove(uci)
}

function returnToGame() {
  selectedUci.value = null
  session.returnToRecordedLine()
  void router.replace({ query: { ...route.query, ply: String(cursorPly.value) } })
}

function resumeVariation() {
  selectedUci.value = null
  session.resumeDraft()
}

async function exportVariation() {
  const blob = await session.exportDraft()
  if (!blob || !snapshot.value) return
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `jade-${snapshot.value.match_id}-variation.json`
  link.click()
  URL.revokeObjectURL(url)
}

async function exportMatchPgn() {
  if (!snapshot.value || typeof window === 'undefined') return
  const resp = await fetch(
    `${matchServerBase()}/api/v1/matches/${snapshot.value.match_id}/export.pgn`,
    { credentials: 'include' },
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

// The replay table: side plates, the capsule's replay controls, and paired moves, as on the home page.
const plates = computed(() => {
  const pieces = boardData.value?.snapshot.pieces ?? []
  const toMove = boardData.value?.snapshot.side_to_move
  return plateOrder(redBelow.value).map((side) => ({
    side,
    name: sideName(side, locale.value),
    toMove: variation.value === null && toMove === side && cursorPly.value < (snapshot.value?.moves_uci.length ?? 0),
    captured: capturedFaces(pieces, side, traditionalGlyphs.value),
  }))
})

const replayActions = computed(() => {
  const end = snapshot.value?.moves_uci.length ?? 0
  return [
    { key: 'first', label: 'First move', paths: ['M5 4v12M15 4l-7 6 7 6'], ply: 0 },
    { key: 'prev', label: 'Previous move', paths: ['M12.5 4 6.5 10l6 6'], ply: cursorPly.value - 1 },
    { key: 'next', label: 'Next move', paths: ['m7.5 4 6 6-6 6'], ply: cursorPly.value + 1 },
    { key: 'last', label: 'Last move', paths: ['M15 4v12M5 4l7 6-7 6'], ply: end },
  ]
})

const moveRows = computed(() => pairMoves(snapshot.value?.moves_uci ?? [], moveLabels.value))
</script>

<template>
  <div class="replay-wrap" data-testid="match-replay-page">
    <section v-if="accessDenied" class="notice">
      <h1 class="notice-title">This game is private</h1>
      <p>
        Personal games are private by default. Copying a position link does not change that; both players must
        choose unlisted or public sharing before others can view this replay.
      </p>
      <NuxtLink to="/" class="jade-btn">Back to the board</NuxtLink>
    </section>

    <section v-else-if="loadError" class="notice">
      <h1 class="notice-title">This replay could not load</h1>
      <p>{{ loadError }}</p>
      <NuxtLink to="/" class="jade-btn">Back to the board</NuxtLink>
    </section>

    <div v-else-if="snapshot" class="jade-table">
      <aside class="jade-plates">
        <template v-for="(plate, i) in plates" :key="plate.side">
          <SidePlate
            :name="plate.name"
            :caption="plate.toMove ? (plate.side === 'black' ? t('turn.black') : t('turn.red')) : ''"
            :to-move="plate.toMove"
            :captured="plate.captured"
          />
          <div v-if="i === 0" class="result">
            <span>{{ snapshot.outcome || snapshot.status }}</span>
            <span v-if="snapshot.reason_code"> · {{ snapshot.reason_code.replace(/_/g, ' ') }}</span>
            <span class="visibility"> · {{ snapshot.visibility }}</span>
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
          last-announcement=""
          @select-square="(sq) => (selectedUci = sq)"
          @play-move="handleTryMove"
        />

        <div class="jade-capsule" role="toolbar" aria-label="Replay">
          <IconButton :label="t('board.flip')" :paths="['M6 3v13M6 3 3 6M6 3l3 3M14 17V4M14 17l-3-3M14 17l3-3']" @click="redBelow = !redBelow" />
          <IconButton
            v-for="a in replayActions.slice(0, 2)"
            :key="a.key"
            :label="a.label"
            :paths="a.paths"
            @click="stepToPly(a.ply)"
          />
          <span class="ply-indicator">{{ cursorPly }} / {{ snapshot.moves_uci.length }}</span>
          <IconButton
            v-for="a in replayActions.slice(2)"
            :key="a.key"
            :label="a.label"
            :paths="a.paths"
            @click="stepToPly(a.ply)"
          />
          <IconButton
            data-testid="btn-export-match-pgn"
            :label="t('export.pgn')"
            :paths="['M10 3v9M6.5 8.5 10 12l3.5-3.5M4 14.5v2h12v-2']"
            @click="exportMatchPgn"
          />
        </div>
      </div>

      <aside class="jade-ledger">
        <section class="moves" aria-label="Moves">
          <ReplayVariationBar
            :variation="session.variation.value"
            :parked="session.parked.value"
            :save-state="session.saveState.value"
            :will-replace-draft="session.willReplaceDraft.value"
            :draft-notice="session.draftNotice.value"
            :feedback="session.feedback.value"
            :recorded-moves-uci="snapshot.moves_uci"
            :root-fen="STARTING_FEN"
            @return-to-game="returnToGame"
            @resume="resumeVariation"
            @export="exportVariation"
          />
          <ol class="jade-move-list" data-testid="recorded-move-list">
            <li v-for="row in moveRows" :key="row.number" class="jade-move-row">
              <span class="jade-move-number">{{ row.number }}.</span>
              <button
                v-for="m in row.plies"
                :key="m.ply"
                type="button"
                class="jade-move"
                :class="{ current: variation === null && cursorPly === m.ply }"
                @click="stepToPly(m.ply)"
              >
                {{ m.label }}
              </button>
            </li>
          </ol>
          <NuxtLink :to="`/invite/${snapshot.invitation_code}`" class="jade-btn room-link">
            Open the room · Rematch
          </NuxtLink>
        </section>

        <EngineAnalysisCard
          :fen="STARTING_FEN"
          :moves-uci="activeMovesUci"
          :state-id="session.stateId.value"
          @play-engine-move="handleEngineMove"
        />
      </aside>
    </div>
  </div>
</template>

<style scoped>
.notice {
  max-width: 560px;
  margin: 48px auto;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 12px;
  color: var(--jade-muted);
}

.notice-title {
  margin: 0;
  font-family: var(--jade-display);
  font-weight: 400;
  font-size: 2.2rem;
  color: var(--jade-paper);
}

.result {
  font-size: 0.85rem;
  color: var(--jade-paper);
  text-transform: capitalize;
}

.visibility {
  color: var(--jade-muted);
}

.ply-indicator {
  padding: 0 8px;
  font-size: 0.85rem;
  font-variant-numeric: tabular-nums;
  color: var(--jade-muted);
}

.moves {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 10px;
}


.room-link {
  align-self: flex-start;
  text-decoration: none;
}
</style>
