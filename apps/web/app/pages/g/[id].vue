<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { applyCoreMove, computeBoardView } from '~/utils/core-wasm'

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
const cursorPly = ref(0)
const branchOriginPly = ref<number | null>(null)
const branchMovesUci = ref<string[]>([])

const activeMovesUci = computed(() => {
  const main = snapshot.value?.moves_uci ?? []
  if (branchOriginPly.value !== null) {
    return [...main.slice(0, branchOriginPly.value), ...branchMovesUci.value]
  }
  return main.slice(0, cursorPly.value)
})

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
    let tokenHeader: Record<string, string> = {}
    const raw = sessionStorage.getItem('jade_guest_session')
    if (raw) {
      const sess = JSON.parse(raw)
      if (sess?.access_token) {
        tokenHeader = { authorization: `Bearer ${sess.access_token}` }
      }
    }
    const resp = await fetch(`${config.public.matchServerUrl}/api/v1/matches/${matchId.value}`, {
      credentials: 'include',
      headers: tokenHeader,
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
    const qPly = Number(route.query.ply)
    cursorPly.value = Number.isFinite(qPly)
      ? Math.max(0, Math.min(data.moves_uci.length, qPly))
      : data.moves_uci.length
    await refreshBoard()
  } catch (err) {
    loadError.value = err instanceof Error ? err.message : String(err)
  }
})

function stepToPly(ply: number) {
  if (!snapshot.value) return
  selectedUci.value = null
  branchOriginPly.value = null
  branchMovesUci.value = []
  const clamped = Math.max(0, Math.min(snapshot.value.moves_uci.length, ply))
  cursorPly.value = clamped
  void router.replace({ query: { ...route.query, ply: String(clamped) } })
}

async function handleTryMove(fromUci: string, toUci: string) {
  selectedUci.value = null
  try {
    const applied = await applyCoreMove({
      fen: STARTING_FEN,
      movesUci: activeMovesUci.value,
      token: `${fromUci}${toUci}`,
      traditionalGlyphs: traditionalGlyphs.value,
    })
    if (branchOriginPly.value === null) {
      branchOriginPly.value = cursorPly.value
    }
    branchMovesUci.value = [...branchMovesUci.value, applied.move_uci]
    await refreshBoard()
  } catch {
    // Ignore illegal variation move
  }
}

async function exportMatchPgn() {
  if (!snapshot.value || typeof window === 'undefined') return
  let tokenHeader: Record<string, string> = {}
  const raw = sessionStorage.getItem('jade_guest_session')
  if (raw) {
    const sess = JSON.parse(raw)
    if (sess?.access_token) {
      tokenHeader = { authorization: `Bearer ${sess.access_token}` }
    }
  }
  const resp = await fetch(
    `${config.public.matchServerUrl}/api/v1/matches/${snapshot.value.match_id}/export.pgn`,
    {
      credentials: 'include',
      headers: tokenHeader,
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
</script>

<template>
  <div class="replay-wrap" data-testid="match-replay-page">
    <section v-if="accessDenied" class="jade-card">
      <h1>Private Match Record · {{ matchId }}</h1>
      <p>
        Personal games are private by default. Creating or copying a position link does not alter access;
        both participants must opt into unlisted or public sharing before others can view this replay.
      </p>
      <NuxtLink to="/" class="jade-btn">Return to Board</NuxtLink>
    </section>

    <section v-else-if="loadError" class="jade-card">
      <h1>Match Replay</h1>
      <p>{{ loadError }}</p>
      <NuxtLink to="/" class="jade-btn">Return to Board</NuxtLink>
    </section>

    <div v-else-if="snapshot" class="replay-grid">
      <div class="board-col">
        <div class="status-row">
          <strong>{{ snapshot.outcome || snapshot.status }}</strong>
          <span v-if="snapshot.reason_code">({{ snapshot.reason_code }})</span>
          <span class="vis-pill">Visibility: {{ snapshot.visibility }}</span>
        </div>

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

        <div class="toolbar">
          <button type="button" class="jade-btn" @click="redBelow = !redBelow">{{ t('board.flip') }}</button>
          <button type="button" class="jade-btn" @click="stepToPly(0)">|&lt;</button>
          <button type="button" class="jade-btn" @click="stepToPly(cursorPly - 1)">&lt;</button>
          <button type="button" class="jade-btn" @click="stepToPly(cursorPly + 1)">&gt;</button>
          <button type="button" class="jade-btn" @click="stepToPly(snapshot.moves_uci.length)">&gt;|</button>
          <button type="button" class="jade-btn" data-testid="btn-export-match-pgn" @click="exportMatchPgn">
            {{ t('export.pgn') }}
          </button>
        </div>
      </div>

      <aside class="side-col">
        <section class="jade-card">
          <h2>Move List · Ply {{ cursorPly }} / {{ snapshot.moves_uci.length }}</h2>
          <div v-if="branchOriginPly !== null" class="branch-note">
            <span>Exploring variation from ply {{ branchOriginPly }} ({{ branchMovesUci.join(' ') }})</span>
            <button type="button" class="jade-pill-btn" @click="stepToPly(branchOriginPly)">
              Restore original line
            </button>
          </div>
          <ol class="moves">
            <li
              v-for="(u, i) in snapshot.moves_uci"
              :key="`${i}-${u}`"
              :class="{ current: branchOriginPly === null && cursorPly === i + 1 }"
              @click="stepToPly(i + 1)"
            >
              {{ i + 1 }}. {{ moveLabels[i] ?? u }}
            </li>
          </ol>
          <div class="links-row">
            <NuxtLink :to="`/invite/${snapshot.invitation_code}`" class="jade-btn">
              Open Room / Rematch
            </NuxtLink>
          </div>
        </section>

        <EngineAnalysisCard
          :fen="STARTING_FEN"
          :moves-uci="activeMovesUci"
          :state-id="`replay:${matchId}:${activeMovesUci.join(',')}`"
          @play-engine-move="(uci) => handleTryMove(uci.slice(0, 2), uci.slice(2, 4))"
        />
      </aside>
    </div>
  </div>
</template>

<style scoped>
.replay-grid {
  display: grid;
  grid-template-columns: minmax(300px, 540px) minmax(290px, 1fr);
  gap: 24px;
  align-items: start;
}

@media (max-width: 880px) {
  .replay-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.board-col,
.side-col {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.status-row,
.toolbar,
.links-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.vis-pill {
  font-size: 0.8rem;
  opacity: 0.75;
}

.branch-note {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px;
  border-radius: 6px;
  background: rgba(243, 238, 228, 0.14);
  font-size: 0.82rem;
  margin-bottom: 8px;
}

.moves {
  list-style: none;
  padding: 0;
  margin: 0 0 12px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.moves li {
  padding: 4px 8px;
  border-radius: 5px;
  background: rgba(243, 238, 228, 0.1);
  cursor: pointer;
  font-size: 0.85rem;
}

.moves li.current {
  background: var(--jade-current-bg);
  color: var(--jade-current-ink);
}
</style>
