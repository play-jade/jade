<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  computeBoardView,
  getSampleArchiveGame,
  querySamplePosition,
  searchSampleArchive,
} from '~/utils/core-wasm'

const route = useRoute()
const {
  locale,
  material,
  darkSurface,
  notationMode,
  traditionalGlyphs,
  t,
} = usePreferences()

const searchQuery = ref(typeof route.query.q === 'string' ? route.query.q : '')
const selectedGameId = ref('sample-game-central-cannon-001')
const replayPly = ref(0)

const { data: archiveData, refresh: refreshSearch } = await useAsyncData(
  () => `archive-directory-${searchQuery.value}`,
  async () => {
    const directory = await searchSampleArchive(searchQuery.value)
    const game = await getSampleArchiveGame(selectedGameId.value)
    const activeMoves = game.moves_uci.slice(0, replayPly.value)
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
    const explorer = await querySamplePosition(board.fen)
    return {
      directory,
      game,
      board,
      explorer,
    }
  },
  {
    watch: [selectedGameId, replayPly, traditionalGlyphs, material, darkSurface],
  },
)

const activeMovesUci = computed(() =>
  archiveData.value ? archiveData.value.game.moves_uci.slice(0, replayPly.value) : [],
)

function stepPly(next: number) {
  const max = archiveData.value?.game.moves_uci.length ?? 0
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
  <div v-if="archiveData" class="archive-page">
    <!-- Explicit Synthetic Fixture Provenance Banner (Section 9) -->
    <div
      v-if="archiveData.directory.is_synthetic_fixture"
      class="provenance-banner"
      data-testid="archive-provenance-banner"
    >
      <strong>Sample Provider ({{ archiveData.directory.dataset_id }}):</strong>
      {{ archiveData.directory.provenance_banner }}
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
            {{ t('game.ply') }} {{ replayPly }} / {{ archiveData.game.moves_uci.length }}
          </span>
          <button type="button" class="jade-btn" data-testid="archive-next" @click="stepPly(replayPly + 1)">
            Next &gt;
          </button>
          <button
            type="button"
            class="jade-btn"
            data-testid="archive-last"
            @click="stepPly(archiveData.game.moves_uci.length)"
          >
            End &gt;|
          </button>
        </div>

        <ol class="archive-moves" data-testid="archive-move-list">
          <li
            v-for="(u, idx) in archiveData.game.moves_uci"
            :key="`${idx}-${u}`"
            class="archive-move-chip"
            :class="{ current: replayPly === idx + 1 }"
            @click="stepPly(idx + 1)"
          >
            <span>{{ idx + 1 }}.</span>
            <strong>{{ notationMode === 'chinese' ? archiveData.game.moves_chinese[idx] || u : u }}</strong>
          </li>
        </ol>
      </div>

      <aside class="archive-side-col">
        <!-- Position Continuation Explorer (Deduplicated by PositionKey, Distinct Status Label!) -->
        <section class="jade-card" data-testid="position-explorer-card">
          <div class="explorer-head">
            <h2 class="card-heading">Position Explorer</h2>
            <span class="source-pill" data-testid="explorer-source-label">
              Sample Archive · {{ archiveData.explorer.sample_size_games }} game(s)
            </span>
          </div>

          <table v-if="archiveData.explorer.continuations.length > 0" class="explorer-table" data-testid="continuation-table">
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
</template>

<style scoped>
.archive-page {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.provenance-banner {
  padding: 10px 14px;
  border-radius: 8px;
  background: rgba(65, 84, 71, 0.12);
  border: 1px solid rgba(65, 84, 71, 0.3);
  font-size: 0.85rem;
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
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.archive-move-chip {
  padding: 4px 8px;
  border-radius: 5px;
  background: rgba(65, 84, 71, 0.1);
  font-size: 0.84rem;
  cursor: pointer;
  display: inline-flex;
  gap: 4px;
}

.archive-move-chip.current {
  background: #415447;
  color: #F3EEE4;
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
