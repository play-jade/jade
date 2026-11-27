<script setup lang="ts">
import { ref } from 'vue'

/** The archive's provenance notice for a synthetic sample collection; dismissing it hides it for this page view. */
defineProps<{
  datasetId: string
  text: string
}>()

const dismissed = ref(false)
</script>

<template>
  <div v-if="!dismissed" class="provenance-banner" role="note" data-testid="archive-provenance-banner">
    <p class="provenance-text">
      <strong>Sample Provider ({{ datasetId }}):</strong>
      {{ text }}
    </p>
    <button
      type="button"
      class="provenance-dismiss"
      aria-label="Dismiss sample provider notice"
      data-testid="archive-provenance-dismiss"
      @click="dismissed = true"
    >
      ×
    </button>
  </div>
</template>

<style scoped>
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
</style>
