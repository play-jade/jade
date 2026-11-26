<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { parsePlyQuery } from '~/composables/useReplaySession'
import { useArchiveProvider } from '~/utils/archive-provider'
import { computeBoardView, notateMoves } from '~/utils/core-wasm'
import { webHref } from '~/utils/web-href'

const route = useRoute()
const router = useRouter()
const config = useRuntimeConfig()
const provider = useArchiveProvider()
const { locale, material, darkSurface, notationDialect, traditionalGlyphs, t } = usePreferences()

const gameId = computed(() => String(route.params.id || ''))
const selectedUci = ref<string | null>(null)
const lastAnnouncement = ref('')

// The game, its collection's labels, and how far its record replays under the rules.
const { data: game, error: gameError, refresh: refreshGame } = await useAsyncData(
  () => `archive-game-${gameId.value}`,
  async () => {
    const [detail, collection] = await Promise.all([provider.gameDetail(gameId.value), provider.collection()])
    // One pass over the record finds where it stops replaying and notates it with the current notation settings;
    // the server render and the first browser render share these labels.
    const moves = detail.record.moves_uci
    // The settings are read once, so the labels and the settings they are tagged with always agree.
    const dialect = notationDialect.value
    const glyphs = traditionalGlyphs.value
    const check = await notateMoves({
      fen: detail.record.initial_fen,
      movesUci: moves,
      wxf: dialect === 'wxf',
      traditionalGlyphs: glyphs,
    })
    const replayable = check.invalid_ply ?? moves.length
    return {
      detail,
      collection,
      replayable,
      invalidMove: check.invalid_ply === null ? null : moves[check.invalid_ply],
      labels: dialect === 'uci' ? moves : moves.map((u, i) => check.notations[i] ?? u),
      // Where the record stops replaying is the core's finding, whatever notation the labels use.
      labelSettings: { dialect, glyphs, invalidPly: check.invalid_ply },
    }
  },
  { watch: [gameId] },
)

const rootFen = computed(() => game.value?.detail.record.initial_fen ?? '')
const recordMovesUci = computed(() => game.value?.detail.record.moves_uci ?? [])
const replayableMovesUci = computed(() => recordMovesUci.value.slice(0, game.value?.replayable ?? 0))

// One kept variation per game and release, on this device. The game is public, so the draft has no guest owner.
const session = useReplaySession({
  source: computed(() =>
    game.value
      ? { kind: 'archive', id: game.value.detail.summary.game_id, revision: game.value.detail.release.release_id }
      : null,
  ),
  rootFen,
  rulesProfile: ref(config.public.rulesProfile),
  recordedMovesUci: replayableMovesUci,
  traditionalGlyphs,
  persist: true,
})
const { cursor, variation, parked, activePath } = session

// A `?ply=` link opens at that position, on the server render too; the kept variation is read in the browser
// once the game is available, which is also after a failed first load is retried.
const linkedPly = parsePlyQuery(route.query.ply)
if (game.value) session.selectPly(linkedPly ?? 0)
let openedGame: string | null = null

async function openGame() {
  const detail = game.value?.detail
  if (!detail) return
  const key = `${detail.summary.game_id}@${detail.release.release_id}`
  if (openedGame === key) return
  openedGame = key
  session.selectPly(linkedPly ?? 0)
  await session.restoreDraft({ keepPosition: linkedPly !== null })
}

onMounted(() => {
  void openGame()
  watch(game, () => void openGame())
})

const { data: board } = await useAsyncData(
  () => `archive-board-${gameId.value}`,
  async () => {
    if (!game.value) return null
    // The board remembers the replay state it shows, so actions read from it apply only while that state holds.
    const stateId = session.stateId.value
    const view = await computeBoardView({
      fen: rootFen.value,
      movesUci: activePath.value,
      width: 460,
      height: 511,
      padding: 18,
      redBelow: true,
      selectedUci: selectedUci.value,
      traditionalGlyphs: traditionalGlyphs.value,
      materialId: material.value,
      darkSurface: darkSurface.value,
    })
    return { ...view, stateId }
  },
  { watch: [activePath, selectedUci, traditionalGlyphs, material, darkSurface] },
)

// The archive's recorded continuations from the position shown. Each answer keeps the position it is for, so
// rows from an earlier position never act on a later one, and a failed lookup stays distinct from an empty one.
const { data: explorerAnswer, refresh: refreshExplorer } = await useAsyncData(
  () => `archive-explorer-${gameId.value}`,
  async () => {
    const shown = board.value
    if (!shown) return null
    const { fen, stateId } = shown
    try {
      return { stateId, response: await provider.explorePosition(fen), error: null }
    } catch (error) {
      return { stateId, response: null, error: error instanceof Error ? error.message : String(error) }
    }
  },
  // The state, not only the placement: one position reached by another move order is another replay state.
  { watch: [() => board.value?.stateId] },
)
const explorer = computed(() => {
  const answer = explorerAnswer.value
  // Rows act only on the replay state they were looked up for, not merely on the board drawn last.
  return answer && answer.stateId === session.stateId.value ? answer : null
})
const EMPTY_EXPLORER: Record<string, string> = {
  no_games_found: 'No further continuations are recorded here.',
  not_yet_parsed: 'This position has not been indexed in this archive release yet.',
  excluded_from_dataset: 'This position is excluded from this archive release.',
  not_analysed: 'This position has not been analysed in this archive release.',
}

/** A source link only for a web address; anything else from the archive is shown as text. */
const sourceHref = computed(() => webHref(game.value?.detail.source.url))

const { labels: moveLabels } = useNotatedMoves(
  recordMovesUci,
  rootFen,
  computed(() => game.value?.labels),
  computed(() => game.value?.labelSettings),
  computed(() => (game.value ? { fen: rootFen.value, movesUci: recordMovesUci.value } : undefined)),
)

/** Pieces on the board in a FEN, to tell a capture from a quiet move. */
const pieceCount = (fen: string) => (fen.split(' ')[0]?.match(/[a-zA-Z]/g) ?? []).length

/** The record's moves paired by move number: Red's ply, then Black's. */
const moveRows = computed(() => {
  const rows: { number: number; plies: { ply: number; label: string }[] }[] = []
  recordMovesUci.value.forEach((u, idx) => {
    if (idx % 2 === 0) rows.push({ number: idx / 2 + 1, plies: [] })
    rows[rows.length - 1]!.plies.push({ ply: idx + 1, label: moveLabels.value[idx] ?? u })
  })
  return rows
})

const resultLine = computed(() => {
  const record = game.value?.detail.record
  if (!record) return ''
  return [record.recorded_result, record.recorded_result_reason].filter(Boolean).join(' · ').replace(/_/g, ' ')
})

function stepToPly(ply: number) {
  selectedUci.value = null
  session.selectPly(ply)
  void router.replace({ query: { ...route.query, ply: String(cursor.value) } })
}

/** A move from the board, a recorded continuation, or the engine: the recorded move follows the game. */
async function tryMove(uci: string) {
  selectedUci.value = null
  // A move is taken only from the board drawn for the current replay state, so what it shows (a capture included)
  // is what the move does; input from a board still drawing an earlier state waits for the new one.
  if (board.value?.stateId !== session.stateId.value) return
  const before = board.value.fen
  const result = await session.tryMove(uci)
  if (result.kind === 'rejected') return
  const applied = result.applied
  // Move arrivals, captures, check, and adjudication reach the live region (docs/rendering.md).
  const notes = [
    pieceCount(applied.next_fen) < pieceCount(before) ? 'capture' : null,
    applied.in_check ? t('state.check') : null,
    applied.adjudication
      ? `${applied.adjudication.outcome.replace(/_/g, ' ')}, ${applied.adjudication.reason.replace(/_/g, ' ')}`
      : null,
  ].filter(Boolean)
  lastAnnouncement.value = [`${applied.move_chinese} (${applied.move_uci})`, ...notes].join(', ')
  if (result.kind === 'followed') void router.replace({ query: { ...route.query, ply: String(cursor.value) } })
}

function handleEngineMove(uci: string, stateId: string) {
  if (stateId === session.stateId.value) void tryMove(uci)
}

function returnToGame() {
  selectedUci.value = null
  session.returnToRecordedLine()
  void router.replace({ query: { ...route.query, ply: String(cursor.value) } })
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
  link.download = `jade-${gameId.value}-variation.json`
  link.click()
  URL.revokeObjectURL(url)
}

/** Replay controls for the glass capsule, as on the home page. */
const scrubActions = computed(() => {
  const end = game.value?.replayable ?? 0
  return [
    { testid: 'archive-first', label: 'First move', paths: ['M5 4v12M15 4l-7 6 7 6'], ply: 0 },
    { testid: 'archive-prev', label: 'Previous move', paths: ['M12.5 4 6.5 10l6 6'], ply: cursor.value - 1 },
    { testid: 'archive-next', label: 'Next move', paths: ['m7.5 4 6 6-6 6'], ply: cursor.value + 1 },
    { testid: 'archive-last', label: 'Last move', paths: ['M15 4v12M5 4l7 6-7 6'], ply: end },
  ]
})
</script>

<template>
  <div v-if="game" class="jade-table archive-page" :class="{ 'has-archive-banner': game.detail.release.is_synthetic_fixture }">
    <ArchiveProvenanceBanner
      v-if="game.detail.release.is_synthetic_fixture"
      :dataset-id="game.detail.release.dataset_id"
      :text="game.collection.provenance_banner"
    />

    <!-- The game's identity, where the play table shows the side plates. -->
    <aside class="jade-plates game-side">
      <header class="game-header" data-testid="archive-game-header">
        <NuxtLink to="/archive" class="opening-tag">← {{ game.collection.title }}</NuxtLink>
        <h1 class="game-title">{{ game.detail.summary.red_display_name }} – {{ game.detail.summary.black_display_name }}</h1>
        <span class="game-date">{{ game.detail.summary.event_name }} · {{ game.detail.summary.date_display }}</span>
        <span class="game-date" data-testid="archive-recorded-result">Recorded result: {{ resultLine || 'not recorded' }}</span>
        <span class="game-date">Recorded under: {{ game.detail.record.original_rules_provenance }}</span>
        <span class="game-date">Replay rules: {{ config.public.rulesProfile }}</span>
        <span class="game-date" data-testid="archive-source">
          Source:
          <a v-if="sourceHref" :href="sourceHref" rel="noopener noreferrer">{{ game.detail.source.label }}</a>
          <template v-else>{{ game.detail.source.label }}</template>
        </span>
      </header>
    </aside>

    <div class="jade-table-stage">
      <XiangqiBoard
        v-if="board"
        :snapshot="board.snapshot"
        :material="material"
        :traditional-glyphs="traditionalGlyphs"
        :dark-surface="darkSurface"
        :locale="locale"
        :last-announcement="lastAnnouncement"
        @select-square="(sq) => (selectedUci = sq)"
        @play-move="(from, to) => tryMove(`${from}${to}`)"
      />

      <div class="jade-capsule" role="toolbar" aria-label="Replay">
        <IconButton
          v-for="(a, i) in scrubActions.slice(0, 2)"
          :key="i"
          :data-testid="a.testid"
          :label="a.label"
          :paths="a.paths"
          @click="stepToPly(a.ply)"
        />
        <span class="ply-indicator" data-testid="archive-ply">
          {{ t('game.ply') }} {{ cursor }} / {{ game.replayable }}
        </span>
        <IconButton
          v-for="(a, i) in scrubActions.slice(2)"
          :key="i + 2"
          :data-testid="a.testid"
          :label="a.label"
          :paths="a.paths"
          @click="stepToPly(a.ply)"
        />
      </div>
    </div>

    <aside class="jade-ledger">
      <section class="moves" aria-label="Moves">
        <ReplayVariationBar
          :variation="variation"
          :parked="parked"
          :save-state="session.saveState.value"
          :will-replace-draft="session.willReplaceDraft.value"
          :draft-notice="session.draftNotice.value"
          :feedback="session.feedback.value"
          :recorded-moves-uci="replayableMovesUci"
          :root-fen="rootFen"
          @return-to-game="returnToGame"
          @resume="resumeVariation"
          @export="exportVariation"
        />
        <ol class="jade-move-list" data-testid="archive-move-list">
          <li v-for="row in moveRows" :key="row.number" class="jade-move-row">
            <span class="jade-move-number">{{ row.number }}.</span>
            <button
              v-for="m in row.plies"
              :key="m.ply"
              type="button"
              class="jade-move"
              :class="{ current: variation === null && cursor === m.ply }"
              :disabled="m.ply > game.replayable"
              :aria-current="variation === null && cursor === m.ply ? 'step' : undefined"
              :data-ply="m.ply"
              data-testid="archive-move"
              @click="stepToPly(m.ply)"
            >
              {{ m.label }}
            </button>
          </li>
        </ol>
        <p v-if="game.invalidMove" class="record-break" data-testid="archive-record-break">
          The record stops replaying at ply {{ game.replayable + 1 }}: {{ game.invalidMove }} is not a legal move in
          that position.
        </p>
      </section>

      <!-- The archive's recorded continuations, labelled apart from the engine's analysis below. -->
      <section class="explorer jade-rule" data-testid="position-explorer-card" aria-label="Position explorer">
        <div class="explorer-head">
          <span class="explorer-title">Recorded continuations</span>
          <span v-if="explorer?.response" class="source-pill" data-testid="explorer-source-label">
            {{ explorer.response.release.is_synthetic_fixture ? 'Sample Archive' : 'Archive' }} ·
            {{ explorer.response.sample_size_games }} game(s) ·
            <span data-testid="explorer-release">{{ explorer.response.release.release_id }}</span>
          </span>
        </div>
        <p v-if="!explorer" class="muted-note">Looking up this position…</p>
        <div v-else-if="explorer.error" class="muted-note" data-testid="explorer-unavailable">
          This position could not be looked up in the archive: {{ explorer.error }}
          <button type="button" class="jade-pill-btn" data-testid="explorer-retry" @click="refreshExplorer()">
            Try again
          </button>
        </div>
        <table
          v-else-if="explorer.response && explorer.response.continuations.length > 0"
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
            <tr v-for="cont in explorer.response.continuations" :key="cont.move_uci" class="cont-row">
              <td>
                <button
                  type="button"
                  class="cont-move"
                  :data-move="cont.move_uci"
                  data-testid="continuation-row"
                  :aria-label="`Try ${cont.move_chinese} (${cont.move_uci})`"
                  @click="tryMove(cont.move_uci)"
                >
                  <strong>{{ cont.move_chinese }}</strong> <code>{{ cont.move_uci }}</code>
                </button>
              </td>
              <td>{{ cont.deduplicated_game_count }}</td>
              <td>{{ cont.red_wins }} / {{ cont.draws }} / {{ cont.black_wins }}</td>
            </tr>
          </tbody>
        </table>
        <p v-else class="muted-note" data-testid="explorer-empty">
          {{ EMPTY_EXPLORER[explorer.response?.status ?? ''] ?? EMPTY_EXPLORER.no_games_found }}
        </p>
      </section>

      <!-- Local browser engine analysis, distinct from the archive's counts. -->
      <EngineAnalysisCard
        :fen="rootFen"
        :moves-uci="activePath"
        :state-id="session.stateId.value"
        @play-engine-move="handleEngineMove"
      />
    </aside>
  </div>
  <div v-else class="archive-error" role="alert" data-testid="archive-error">
    <p>This archive game could not load{{ gameError ? `: ${gameError.message}` : '' }}.</p>
    <button type="button" class="jade-btn" @click="refreshGame()">Try again</button>
  </div>
</template>

<style scoped>
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

.cont-move {
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.cont-move:focus-visible {
  outline: 2px solid var(--jade-pale);
  outline-offset: 2px;
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
