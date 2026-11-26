<script setup lang="ts">
import { computed, watch } from 'vue'

const props = defineProps<{
  fen: string
  movesUci: string[]
  stateId: string
}>()

const emit = defineEmits<{
  /** A suggestion for the position `stateId` names; the receiver applies it only to that position. */
  (e: 'playEngineMove', uci: string, stateId: string): void
}>()

const { t } = usePreferences()
const {
  isAnalyzing,
  engineName,
  depth,
  scoreRedCp,
  mateRed,
  pvUci,
  bestMoveUci,
  errorMsg,
  startAnalysis,
  stopAnalysis,
  invalidateForState,
} = useLocalEngine()

// Engine moves show in the reader's notation, notated from the analysed position; UCI stays internal.
// With no engine line there is nothing to notate, so the game is not replayed for an empty card.
const pvPath = computed(() => (pvUci.value.length > 0 ? [...props.movesUci, ...pvUci.value] : []))
const { labels: pvPathLabels } = useNotatedMoves(pvPath, computed(() => props.fen))
const pvLabels = computed(() =>
  pvPathLabels.value.length > props.movesUci.length ? pvPathLabels.value.slice(props.movesUci.length) : [],
)
const bestMoveLabel = computed(() =>
  bestMoveUci.value && pvUci.value[0] === bestMoveUci.value ? (pvLabels.value[0] ?? bestMoveUci.value) : bestMoveUci.value,
)

watch(
  () => props.stateId,
  (nextId) => {
    void invalidateForState(nextId)
  },
)

const formattedEval = computed(() => {
  if (mateRed.value !== null) {
    return mateRed.value >= 0 ? `Red M${mateRed.value}` : `Black M${Math.abs(mateRed.value)}`
  }
  if (scoreRedCp.value !== null) {
    const pawns = (scoreRedCp.value / 100).toFixed(2)
    return scoreRedCp.value >= 0 ? `+${pawns} (Red)` : `${pawns} (Red)`
  }
  return '—'
})

function onToggleEngine() {
  if (isAnalyzing.value) {
    void stopAnalysis()
  } else {
    void startAnalysis({
      fen: props.fen,
      movesUci: props.movesUci,
      stateId: props.stateId,
      movetimeMs: 900,
    })
  }
}
</script>

<template>
  <section class="engine-card" data-testid="engine-analysis-card" :aria-label="t('engine.title')">
    <div class="engine-header">
      <button
        type="button"
        class="jade-btn"
        data-testid="engine-toggle-btn"
        :title="`${engineName} runs in this browser; positions stay on this device.`"
        @click="onToggleEngine"
      >
        {{ isAnalyzing ? t('engine.stop') : t('engine.start') }}
      </button>
      <span v-if="isAnalyzing || depth > 0" class="engine-metrics">
        <span data-testid="engine-eval">{{ formattedEval }}</span>
        <span class="metric-label">depth <span data-testid="engine-depth">{{ depth }}</span></span>
      </span>
    </div>

    <div v-if="pvUci.length > 0 || bestMoveUci" class="engine-pv" data-testid="engine-pv">
      <code>{{ pvLabels.join(' ') || bestMoveLabel }}</code>
      <button
        v-if="bestMoveUci"
        type="button"
        class="jade-pill-btn"
        data-testid="engine-play-best"
        @click="emit('playEngineMove', bestMoveUci, stateId)"
      >
        Try {{ bestMoveLabel }}
      </button>
    </div>

    <p v-if="errorMsg" class="engine-error">{{ errorMsg }}</p>
  </section>
</template>

<style scoped>
.engine-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.engine-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.engine-metrics {
  display: inline-flex;
  align-items: baseline;
  gap: 10px;
  font-size: 0.95rem;
  font-variant-numeric: tabular-nums;
}

.metric-label {
  font-size: 0.78rem;
  color: var(--jade-muted);
}

.engine-pv {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-size: 0.85rem;
  color: var(--jade-muted);
}

.engine-error {
  margin: 0;
  font-size: 0.8rem;
  color: var(--jade-vermilion-text);
}
</style>
