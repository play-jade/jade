<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  computeBoardView,
  getSampleArchiveGame,
  notateMoves,
  querySamplePosition,
  searchSampleArchive,
} from '~/utils/core-wasm'

const route = useRoute()
const {
  locale,
  material,
  darkSurface,
  notationDialect,
  traditionalGlyphs,
  t,
} = usePreferences()

const searchQuery = ref(typeof route.query.q === 'string' ? route.query.q : '')
const selectedGameId = ref('sample-game-central-cannon-001')
const replayPly = ref(0)
const bannerDismissed = ref(false)

const { data: archiveData, error: archiveError, refresh: refreshSearch } = await useAsyncData(
  () => `archive-directory-${searchQuery.value}`,
  async () => {
    const directory = await searchSampleArchive(searchQuery.value)
    const game = await getSampleArchiveGame(selectedGameId.value)
    // A record can hold a move that does not replay; the board stops at the last move that does.
    const replay = await notateMoves({
      fen: game.initial_fen,
      movesUci: game.moves_uci,
      wxf: notationDialect.value === 'wxf',
      traditionalGlyphs: traditionalGlyphs.value,
    })
    const replayable = replay.invalid_ply ?? game.moves_uci.length
    const activeMoves = game.moves_uci.slice(0, Math.min(replayPly.value, replayable))
    const board = await computeBoardView({
      fen: game.initial_fen,
      movesUci: activeMoves,
      width: 460,
      height: 511,
      padding: 18,
      redBelow: true,
      traditionalGlyphs: traditionalGlyphs.value,
      materialId: material.value,
      darkSurface: darkSurface.value,
    })
    const explorer = await querySamplePosition(board.fen).catch(() => null)
    return {
      directory,
      game,
      board,
      explorer,
      replayable,
      notations:
        notationDialect.value === 'uci'
          ? game.moves_uci
          : game.moves_uci.map((u, i) => replay.notations[i] ?? u),
      invalidMove: replay.invalid_ply === null ? null : game.moves_uci[replay.invalid_ply],
    }
  },
  {
    watch: [selectedGameId, replayPly, traditionalGlyphs, material, darkSurface],
  },
)

const activeMovesUci = computed(() =>
  archiveData.value ? archiveData.value.game.moves_uci.slice(0, replayPly.value) : [],
)

const gameMovesUci = computed(() => archiveData.value?.game.moves_uci ?? [])
const gameFen = computed(() => archiveData.value?.game.initial_fen)
const { labels: moveLabels } = useNotatedMoves(
  gameMovesUci,
  gameFen,
  computed(() => archiveData.value?.notations),
)

/** Moves paired by move number: Red's ply, then Black's. */
const moveRows = computed(() => {
  const rows: { number: number; plies: { ply: number; label: string }[] }[] = []
  gameMovesUci.value.forEach((u, idx) => {
    if (idx % 2 === 0) rows.push({ number: idx / 2 + 1, plies: [] })
    rows[rows.length - 1].plies.push({ ply: idx + 1, label: moveLabels.value[idx] ?? u })
  })
  return rows
})

function stepPly(next: number) {
  const max = archiveData.value?.replayable ?? 0
  replayPly.value = Math.max(0, Math.min(max, next))
}

async function applyContinuation(uci: string) {
  const game = archiveData.value?.game
  if (!game) return
  if (game.moves_uci[replayPly.value] === uci) {
    replayPly.value += 1
  }
}

/** Replay controls for the glass capsule, as on the home page. */
const scrubActions = computed(() => {
  const end = archiveData.value?.replayable ?? 0
  return [
    { testid: 'archive-first', label: 'First move', paths: ['M5 4v12M15 4l-7 6 7 6'], ply: 0 },
    { testid: 'archive-prev', label: 'Previous move', paths: ['M12.5 4 6.5 10l6 6'], ply: replayPly.value - 1 },
    { testid: 'archive-next', label: 'Next move', paths: ['m7.5 4 6 6-6 6'], ply: replayPly.value + 1 },
    { testid: 'archive-last', label: 'Last move', paths: ['M15 4v12M5 4l7 6-7 6'], ply: end },
  ]
})
</script>

<template>
  <div
    v-if="archiveData"
    class="jade-table archive-page"
    :class="{ 'has-banner': archiveData.directory.is_synthetic_fixture }"
  >
    <!-- Explicit Synthetic Fixture Provenance Banner (Section 9), overlaid so it never moves the page -->
    <div
      v-if="archiveData.directory.is_synthetic_fixture && !bannerDismissed"
      class="provenance-banner"
      role="note"
      data-testid="archive-provenance-banner"
    >
      <p class="provenance-text">
        <strong>Sample Provider ({{ archiveData.directory.dataset_id }}):</strong>
        {{ archiveData.directory.provenance_banner }}
      </p>
      <button
        type="button"
        class="provenance-dismiss"
        aria-label="Dismiss sample provider notice"
        data-testid="archive-provenance-dismiss"
        @click="bannerDismissed = true"
      >
        ×
      </button>
    </div>

    <!-- The game's identity, where the play table shows the side plates. -->
    <aside class="jade-plates game-side">
      <header class="game-header">
        <span v-if="archiveData.game.opening_label" class="opening-tag">
          {{ t('archive.opening') }} · {{ archiveData.game.opening_label }}
        </span>
        <h1 class="game-title">{{ archiveData.game.event_name }}</h1>
        <span class="game-date">{{ archiveData.game.date_display }}</span>
      </header>

      <section class="players" data-testid="player-directory-card" aria-label="Players">
        <form method="get" action="/archive" class="search-form" @submit.prevent="refreshSearch()">
          <input
            v-model="searchQuery"
            name="q"
            type="search"
            aria-label="Search players"
            placeholder="赵汝权, 趙汝權, Chiu, 林明…"
            class="search-input"
            data-testid="player-search-input"
          />
          <button type="submit" class="jade-btn" data-testid="player-search-btn">Search</button>
        </form>
        <ul class="player-list" data-testid="player-list">
          <li
            v-for="player in archiveData.directory.players"
            :key="player.player_id"
            class="player-item"
            :data-player-id="player.player_id"
          >
            <NuxtLink :to="`/players/${player.player_id}`" class="player-link">
              {{ player.native_display_name }} · {{ player.primary_display_name }}
            </NuxtLink>
            <span class="disambig-note">{{ player.disambiguation_note }}</span>
          </li>
        </ul>
      </section>
    </aside>

    <div class="jade-table-stage">
      <XiangqiBoard
        :snapshot="archiveData.board.snapshot"
        :material="material"
        :traditional-glyphs="traditionalGlyphs"
        :dark-surface="darkSurface"
        :locale="locale"
        :interactive="false"
      />

      <div class="jade-capsule" role="toolbar" aria-label="Replay">
        <IconButton
          v-for="(a, i) in scrubActions.slice(0, 2)"
          :key="i"
          :data-testid="a.testid"
          :label="a.label"
          :paths="a.paths"
          @click="stepPly(a.ply)"
        />
        <span class="ply-indicator" data-testid="archive-ply">
          {{ t('game.ply') }} {{ replayPly }} / {{ archiveData.replayable }}
        </span>
        <IconButton
          v-for="(a, i) in scrubActions.slice(2)"
          :key="i + 2"
          :data-testid="a.testid"
          :label="a.label"
          :paths="a.paths"
          @click="stepPly(a.ply)"
        />
      </div>
    </div>

    <aside class="jade-ledger">
      <section class="moves" aria-label="Moves">
        <ol class="jade-move-list" data-testid="archive-move-list">
          <li v-for="row in moveRows" :key="row.number" class="jade-move-row">
            <span class="jade-move-number">{{ row.number }}.</span>
            <button
              v-for="m in row.plies"
              :key="m.ply"
              type="button"
              class="jade-move"
              :class="{ current: replayPly === m.ply }"
              :disabled="m.ply > archiveData.replayable"
              :aria-current="replayPly === m.ply ? 'step' : undefined"
              :data-ply="m.ply"
              data-testid="archive-move"
              @click="stepPly(m.ply)"
            >
              {{ m.label }}
            </button>
          </li>
        </ol>
        <p v-if="archiveData.invalidMove" class="record-break" data-testid="archive-record-break">
          The record stops replaying at ply {{ archiveData.replayable + 1 }}: {{ archiveData.invalidMove }} is not a
          legal move in that position.
        </p>
      </section>

      <!-- Position Continuation Explorer (Deduplicated by PositionKey, Distinct Status Label!) -->
      <section class="explorer jade-rule" data-testid="position-explorer-card" aria-label="Position explorer">
        <div class="explorer-head">
          <span class="explorer-title">Recorded continuations</span>
          <span class="source-pill" data-testid="explorer-source-label">
            Sample Archive · {{ archiveData.explorer?.sample_size_games ?? 0 }} game(s)
          </span>
        </div>
        <p v-if="!archiveData.explorer" class="muted-note" data-testid="explorer-unavailable">
          This position could not be looked up in the sample archive.
        </p>
        <table
          v-else-if="archiveData.explorer.continuations.length > 0"
          class="explorer-table"
          data-testid="continuation-table"
        >
          <thead>
            <tr>
              <th>Move</th>
              <th>Games</th>
              <th>R / D / B</th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-for="cont in archiveData.explorer.continuations"
              :key="cont.move_uci"
              class="cont-row"
              @click="applyContinuation(cont.move_uci)"
            >
              <td><strong>{{ cont.move_chinese }}</strong> <code>{{ cont.move_uci }}</code></td>
              <td>{{ cont.deduplicated_game_count }}</td>
              <td>{{ cont.red_wins }} / {{ cont.draws }} / {{ cont.black_wins }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="muted-note">No further continuations recorded in the sample.</p>
      </section>

      <!-- Local Browser Web Worker Engine Analysis (Explicitly Distinct from Archive Counts!) -->
      <EngineAnalysisCard
        :fen="archiveData.game.initial_fen"
        :moves-uci="activeMovesUci"
        :state-id="`archive:${selectedGameId}:${replayPly}`"
      />
    </aside>
  </div>
  <div v-else class="archive-error" role="alert" data-testid="archive-error">
    <p>The archive could not load{{ archiveError ? `: ${archiveError.message}` : '' }}.</p>
    <button type="button" class="jade-btn" @click="refreshSearch()">Try again</button>
  </div>
</template>

<style scoped>
/* Room below the content for the overlaid banner, kept after dismissal so the page height never changes. */
.archive-page.has-banner {
  padding-bottom: 96px;
}

/* Overlaid at the bottom of the viewport so showing or dismissing it never moves the page. */
.provenance-banner {
  position: fixed;
  left: 50%;
  bottom: 16px;
  z-index: 20;
  transform: translateX(-50%);
  width: min(720px, calc(100vw - 32px));
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 10px 10px 10px 16px;
  border-radius: 12px;
  background: rgba(29, 36, 33, 0.8);
  backdrop-filter: blur(24px) saturate(140%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  box-shadow: inset 0 0 0 1px var(--jade-hairline), 0 10px 30px rgba(0, 0, 0, 0.4);
  font-size: 0.82rem;
  line-height: 1.4;
}

.provenance-text {
  margin: 0;
  flex: 1;
}

.provenance-dismiss {
  flex: none;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: 14px;
  background: transparent;
  color: inherit;
  font-size: 1.1rem;
  line-height: 1;
  cursor: pointer;
}

.provenance-dismiss:hover,
.provenance-dismiss:focus-visible {
  background: var(--jade-surface-hover);
}

.game-side {
  justify-content: flex-start;
  gap: 28px;
}

.game-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.opening-tag {
  font-size: 0.72rem;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--jade-pale);
}

.game-title {
  margin: 0;
  font-family: var(--jade-display);
  font-weight: 400;
  font-size: clamp(1.8rem, 2.6vw, 2.4rem);
  line-height: 1.1;
}

.game-date {
  font-size: 0.85rem;
  color: var(--jade-muted);
}

.players {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.search-form {
  display: flex;
  gap: 6px;
}

.search-input {
  flex: 1;
  min-width: 0;
  padding: 7px 10px;
  border-radius: 8px;
  border: 1px solid var(--jade-hairline);
  background: var(--jade-surface);
  color: var(--jade-paper);
  font: inherit;
  font-size: 0.85rem;
}

.player-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.player-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.player-link {
  color: var(--jade-paper);
  text-decoration: none;
  font-size: 0.92rem;
}

.player-link:hover {
  text-decoration: underline;
}

.disambig-note,
.muted-note {
  margin: 0;
  font-size: 0.78rem;
  color: var(--jade-muted);
}

.ply-indicator {
  padding: 0 8px;
  font-size: 0.85rem;
  font-variant-numeric: tabular-nums;
  color: var(--jade-muted);
}

.moves {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.record-break {
  margin: 0;
  font-size: 0.82rem;
  color: var(--jade-vermilion-text);
}

.explorer {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.explorer-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
}

.explorer-title {
  font-size: 0.72rem;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--jade-muted);
}

.source-pill {
  font-size: 0.72rem;
  color: var(--jade-muted);
}

.explorer-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.85rem;
}

.explorer-table th {
  text-align: left;
  font-weight: 500;
  font-size: 0.72rem;
  color: var(--jade-muted);
  padding: 4px 6px;
}

.explorer-table td {
  padding: 6px;
  border-top: 1px solid var(--jade-hairline);
}

.explorer-table code {
  font-size: 0.75rem;
  color: var(--jade-muted);
}

.cont-row {
  cursor: pointer;
}

.cont-row:hover {
  background: var(--jade-surface-hover);
}

.archive-error {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  padding: 24px 0;
}
</style>
