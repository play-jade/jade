<script setup lang="ts">
import type { DraftSaveState, Variation } from '~/composables/useReplaySession'

/** The recorded game stays intact; this bar shows the visitor's variation and where it is kept. */
const props = defineProps<{
  variation: Variation | null
  parked: Variation | null
  saveState: DraftSaveState
  willReplaceDraft: boolean
  draftNotice: string | null
  feedback: string | null
}>()

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
</style>
