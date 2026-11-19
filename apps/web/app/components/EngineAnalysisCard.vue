<script setup lang="ts">
import { computed, watch } from 'vue'

const props = defineProps<{
  fen: string
  movesUci: string[]
  stateId: string
}>()

const emit = defineEmits<{
  (e: 'playEngineMove', uci: string): void
}>()

const { t } = usePreferences()
const {
  isAnalyzing,
  engineName,
  depth,
  nodes,
  nps,
  scoreRedCp,
  mateRed,
  pvUci,
  bestMoveUci,
  errorMsg,
  startAnalysis,
  stopAnalysis,
  invalidateForState,
} = useLocalEngine()

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
  <section class="jade-card engine-card" data-testid="engine-analysis-card">
    <div class="engine-header">
      <div>
        <span class="engine-badge">Local Engine · Browser WASM</span>
        <h3 class="engine-title">{{ t('engine.title') }}</h3>
      </div>

      <button
        type="button"
        class="jade-btn jade-btn-primary"
        data-testid="engine-toggle-btn"
        @click="onToggleEngine"
      >
        {{ isAnalyzing ? t('engine.stop') : t('engine.start') }}
      </button>
    </div>

    <div class="engine-metrics">
      <div>
        <span class="metric-label">Eval (Red)</span>
        <strong data-testid="engine-eval">{{ formattedEval }}</strong>
      </div>
      <div>
        <span class="metric-label">Depth</span>
        <strong data-testid="engine-depth">{{ depth }}</strong>
      </div>
      <div>
        <span class="metric-label">Nodes</span>
        <strong>{{ nodes.toLocaleString() }}</strong>
      </div>
      <div>
        <span class="metric-label">NPS</span>
        <strong>{{ nps.toLocaleString() }}</strong>
      </div>
    </div>

    <div v-if="pvUci.length > 0 || bestMoveUci" class="engine-pv" data-testid="engine-pv">
      <span class="metric-label">Principal variation:</span>
      <code>{{ pvUci.join(' ') || bestMoveUci }}</code>
      <button
        v-if="bestMoveUci"
        type="button"
        class="jade-pill-btn"
        data-testid="engine-play-best"
        @click="emit('playEngineMove', bestMoveUci)"
      >
        Try {{ bestMoveUci }}
      </button>
    </div>

    <p class="engine-caption">
      {{ engineName }} — runs locally in a Web Worker without sending positions to a server.
    </p>
    <p v-if="errorMsg" class="engine-error">{{ errorMsg }}</p>
  </section>
</template>

<style scoped>
.engine-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.engine-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.engine-badge {
  display: inline-block;
  font-size: 0.72rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: #415447;
}

.theme-dark .engine-badge {
  color: #DCE3CE;
}

.engine-title {
  margin: 2px 0 0;
  font-size: 0.98rem;
}

.engine-metrics {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 8px;
  font-size: 0.86rem;
}

.metric-label {
  display: block;
  font-size: 0.74rem;
  opacity: 0.72;
}

.engine-pv {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-size: 0.85rem;
}

.engine-caption {
  margin: 0;
  font-size: 0.76rem;
  opacity: 0.72;
}

.engine-error {
  margin: 0;
  font-size: 0.8rem;
  color: #A54837;
}
</style>
