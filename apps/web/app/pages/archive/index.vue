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
</script>

<template>
  <div
    v-if="archiveData"
    class="archive-page"
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

    <div class="archive-grid">
      <div class="archive-board-col">
        <header class="game-header">
          <span class="opening-tag">{{ t('archive.opening') }}: {{ archiveData.game.opening_label }}</span>
          <h1 class="game-title">{{ archiveData.game.event_name }} ({{ archiveData.game.date_display }})</h1>
        </header>

        <XiangqiBoard
          :snapshot="archiveData.board.snapshot"
          :material="material"
          :traditional-glyphs="traditionalGlyphs"
          :dark-surface="darkSurface"
          :locale="locale"
          :interactive="false"
        />

        <div class="replay-bar">
          <button type="button" class="jade-btn" data-testid="archive-first" @click="stepPly(0)">
            |&lt; Start
          </button>
          <button type="button" class="jade-btn" data-testid="archive-prev" @click="stepPly(replayPly - 1)">
            &lt; Prev
          </button>
          <span class="ply-indicator" data-testid="archive-ply">
            {{ t('game.ply') }} {{ replayPly }} / {{ archiveData.replayable }}
          </span>
          <button type="button" class="jade-btn" data-testid="archive-next" @click="stepPly(replayPly + 1)">
            Next &gt;
          </button>
          <button
            type="button"
            class="jade-btn"
            data-testid="archive-last"
            @click="stepPly(archiveData.replayable)"
          >
            End &gt;|
          </button>
        </div>

        <ol class="archive-moves" data-testid="archive-move-list">
          <li v-for="row in moveRows" :key="row.number" class="archive-move-row">
            <span class="archive-move-number">{{ row.number }}.</span>
            <button
              v-for="m in row.plies"
              :key="m.ply"
              type="button"
              class="archive-move"
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
      </div>

      <aside class="archive-side-col">
        <!-- Position Continuation Explorer (Deduplicated by PositionKey, Distinct Status Label!) -->
        <section class="jade-card" data-testid="position-explorer-card">
          <div class="explorer-head">
            <h2 class="card-heading">Position Explorer</h2>
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
                <th>Coord</th>
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
                <td><strong>{{ cont.move_chinese }}</strong></td>
                <td><code>{{ cont.move_uci }}</code></td>
                <td>{{ cont.deduplicated_game_count }}</td>
                <td>{{ cont.red_wins }} / {{ cont.draws }} / {{ cont.black_wins }}</td>
              </tr>
            </tbody>
          </table>
          <p v-else class="muted-note">No further continuations recorded in sample fixture.</p>
        </section>

        <!-- Local Browser Web Worker Engine Analysis (Explicitly Distinct from Archive Counts!) -->
        <EngineAnalysisCard
          :fen="archiveData.game.initial_fen"
          :moves-uci="activeMovesUci"
          :state-id="`archive:${selectedGameId}:${replayPly}`"
        />

        <!-- Disambiguated Player Search & Directory -->
        <section class="jade-card" data-testid="player-directory-card">
          <h2 class="card-heading">Players &amp; Alias Search</h2>
          <form method="get" action="/archive" class="search-form" @submit.prevent="refreshSearch()">
            <input
              v-model="searchQuery"
              name="q"
              type="search"
              placeholder="Search 赵汝权, 趙汝權, Chiu, 林明…"
              class="search-input"
              data-testid="player-search-input"
            />
            <button type="submit" class="jade-btn jade-btn-primary" data-testid="player-search-btn">
              Search
            </button>
          </form>

          <ul class="player-list" data-testid="player-list">
            <li
              v-for="player in archiveData.directory.players"
              :key="player.player_id"
              class="player-item"
              :data-player-id="player.player_id"
            >
              <NuxtLink :to="`/players/${player.player_id}`" class="player-link">
                <strong>{{ player.native_display_name }} · {{ player.primary_display_name }}</strong>
              </NuxtLink>
              <span class="disambig-note">{{ player.disambiguation_note }}</span>
            </li>
          </ul>
        </section>
      </aside>
    </div>
  </div>
  <div v-else class="archive-error" role="alert" data-testid="archive-error">
    <p>The archive could not load{{ archiveError ? `: ${archiveError.message}` : '' }}.</p>
    <button type="button" class="jade-btn" @click="refreshSearch()">Try again</button>
  </div>
</template>

<style scoped>
.archive-page {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

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
  box-sizing: border-box;
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 10px 10px 10px 16px;
  border-radius: 10px;
  background: rgba(29, 36, 33, 0.92);
  color: #F3EEE4;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.25);
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
  background: rgba(243, 238, 228, 0.14);
}

.archive-grid {
  display: grid;
  grid-template-columns: minmax(300px, 520px) minmax(300px, 1fr);
  gap: 24px;
  align-items: start;
}

@media (max-width: 880px) {
  .archive-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.archive-board-col,
.archive-side-col {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.opening-tag {
  font-size: 0.78rem;
  font-weight: 600;
  color: #415447;
  text-transform: uppercase;
}

.theme-dark .opening-tag {
  color: #DCE3CE;
}

.game-title {
  margin: 4px 0 0;
  font-size: 1.15rem;
}

.replay-bar {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: 8px;
}

.ply-indicator {
  font-size: 0.86rem;
  font-weight: 600;
}

.archive-moves {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 2px 16px;
  font-variant-numeric: tabular-nums;
}

.archive-move-row {
  display: grid;
  grid-template-columns: 2.2em minmax(0, 1fr) minmax(0, 1fr);
  align-items: center;
  gap: 4px;
}

.archive-move-number {
  font-size: 0.78rem;
  opacity: 0.7;
}

.archive-move {
  appearance: none;
  border: 0;
  border-radius: 5px;
  padding: 5px 8px;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 0.9rem;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
}

.archive-move:hover:not(:disabled):not(.current) {
  background: rgba(65, 84, 71, 0.1);
}

.archive-move.current {
  background: #415447;
  color: #F3EEE4;
}

.archive-move:disabled {
  cursor: default;
  opacity: 0.45;
  text-decoration: line-through;
}

.record-break {
  margin: 0;
  font-size: 0.82rem;
  color: #A54837;
}

.archive-error {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  padding: 24px 0;
}

.explorer-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 10px;
}

.card-heading {
  margin: 0;
  font-size: 1rem;
}

.source-pill {
  font-size: 0.75rem;
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(65, 84, 71, 0.14);
  font-weight: 600;
}

.explorer-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.86rem;
}

.explorer-table th,
.explorer-table td {
  text-align: left;
  padding: 6px 8px;
  border-bottom: 1px solid rgba(65, 84, 71, 0.16);
}

.cont-row {
  cursor: pointer;
}

.search-form {
  display: flex;
  gap: 8px;
  margin: 10px 0;
}

.search-input {
  flex: 1;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid rgba(65, 84, 71, 0.3);
  font: inherit;
  font-size: 0.86rem;
}

.player-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.player-item {
  display: flex;
  flex-direction: column;
  padding: 8px 10px;
  border-radius: 6px;
  background: rgba(65, 84, 71, 0.08);
}

.player-link {
  color: inherit;
  text-decoration: none;
}

.disambig-note {
  font-size: 0.78rem;
  opacity: 0.78;
}

.muted-note {
  margin: 0;
  font-size: 0.84rem;
  opacity: 0.75;
}
</style>
