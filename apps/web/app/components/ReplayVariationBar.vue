<script setup lang="ts">
import { computed } from 'vue'
import type { DraftSaveState, Variation } from '~/composables/useReplaySession'

/** The recorded game stays intact; this bar shows the visitor's variation and where it is kept. */
const props = defineProps<{
  variation: Variation | null
  parked: Variation | null
  saveState: DraftSaveState
  willReplaceDraft: boolean
  draftNotice: string | null
  feedback: string | null
  /** The recorded line the variation leaves, so its moves are notated from the right position. */
  recordedMovesUci: string[]
  rootFen: string
}>()

// The variation's moves in the reader's notation, notated along the recorded line up to its origin.
const line = computed(() => props.variation ?? props.parked)
const linePath = computed(() =>
  line.value ? [...props.recordedMovesUci.slice(0, line.value.originPly), ...line.value.movesUci] : [],
)
const { labels } = useNotatedMoves(linePath, computed(() => props.rootFen))
const lineMoves = computed(() => {
  const origin = line.value?.originPly ?? 0
  return labels.value.slice(origin).map((label, index) => ({
    ply: origin + index + 1,
    number: Math.floor((origin + index) / 2) + 1,
    black: (origin + index) % 2 === 1,
    label,
  }))
})

const emit = defineEmits<{
  (e: 'returnToGame'): void
  (e: 'resume'): void
  (e: 'export'): void
}>()
</script>

<template>
  <div
    v-if="props.variation || props.parked || props.draftNotice || props.feedback"
    class="variation-bar"
    data-testid="variation-bar"
    role="status"
  >
    <template v-if="props.variation">
      <span data-testid="variation-origin">Variation from ply {{ props.variation.originPly }}</span>
      <button type="button" class="jade-pill-btn" data-testid="variation-return" @click="emit('returnToGame')">
        Return to game
      </button>
    </template>
    <template v-else-if="props.parked">
      <span>{{ props.saveState.state === 'saved' ? 'Saved variation' : 'Variation' }} from ply {{ props.parked.originPly }}</span>
      <button type="button" class="jade-pill-btn" data-testid="variation-resume" @click="emit('resume')">
        Resume variation
      </button>
      <span v-if="props.willReplaceDraft" class="variation-note">A new variation replaces it.</span>
    </template>

    <!-- The save status and export describe a line; with none shown, only a notice says what happened. -->
    <template v-if="props.variation || props.parked">
      <span v-if="props.saveState.state === 'saved'" class="variation-note" data-testid="variation-saved">
        Saved on this device
      </span>
      <span v-else-if="props.saveState.state === 'pending'" class="variation-note" data-testid="variation-saving">
        Saving…
      </span>
      <template v-else-if="props.saveState.state === 'unsaved'">
        <span class="variation-warn" data-testid="variation-unsaved">Not saved: {{ props.saveState.reason }}</span>
      <button type="button" class="jade-pill-btn" data-testid="variation-export" @click="emit('export')">
          Export variation
        </button>
      </template>
    </template>

    <ol v-if="lineMoves.length > 0" class="variation-moves" data-testid="variation-moves" aria-label="Variation moves">
      <li v-for="(m, i) in lineMoves" :key="m.ply" :class="{ current: props.variation && i === lineMoves.length - 1 }">
        <span v-if="!m.black || i === 0" class="variation-number">{{ m.number }}{{ m.black ? '…' : '.' }}</span>
        <span class="variation-label">{{ m.label }}</span>
      </li>
    </ol>

    <span v-if="props.draftNotice" class="variation-warn" data-testid="variation-notice">{{ props.draftNotice }}</span>
    <span v-if="props.feedback" class="variation-warn" data-testid="variation-feedback">{{ props.feedback }}</span>
  </div>
</template>

<style scoped>
.variation-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
  font-size: 0.85rem;
}

.variation-note {
  color: var(--jade-muted);
}

.variation-warn {
  color: var(--jade-vermilion-text);
}

.variation-moves {
  flex-basis: 100%;
  display: flex;
  flex-wrap: wrap;
  gap: 2px 10px;
  margin: 0;
  padding: 0;
  list-style: none;
  color: var(--jade-pale);
}

.variation-moves .current {
  font-weight: 600;
  text-decoration: underline;
}

.variation-number {
  color: var(--jade-muted);
  margin-right: 2px;
}
</style>
