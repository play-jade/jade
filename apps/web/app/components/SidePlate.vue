<script setup lang="ts">
/** One side's plate beside the board, as on macOS: a caption with the to-move light, the side's name in the display
 *  face, an optional clock in the default slot, and the pieces the side has lost. */
defineProps<{
  name: string
  caption: string
  toMove: boolean
  captured: string[]
}>()
</script>

<template>
  <div class="side-plate" :class="{ 'to-move': toMove }">
    <div class="side-plate-caption">
      <span class="side-plate-dot" aria-hidden="true" />
      <span>{{ caption || ' ' }}</span>
    </div>
    <div class="side-plate-name">{{ name }}</div>
    <slot />
    <div v-if="captured.length" class="side-plate-captured" :aria-label="`${name}: ${captured.length} captured`">
      {{ captured.join(' ') }}
    </div>
  </div>
</template>

<style scoped>
.side-plate {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.side-plate-caption {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 0.72rem;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--jade-muted);
}

.side-plate-dot {
  width: 8px;
  height: 8px;
  border-radius: 4px;
  background: var(--jade-hairline);
  transition: background 180ms ease, box-shadow 180ms ease;
}

.to-move .side-plate-caption {
  color: var(--jade-pale);
}

.to-move .side-plate-dot {
  background: var(--jade-pale);
  box-shadow: 0 0 12px 2px rgba(220, 227, 206, 0.55);
}

.side-plate-name {
  font-family: var(--jade-display);
  font-size: clamp(2.6rem, 4.2vw, 3.6rem);
  line-height: 1;
  color: var(--jade-muted);
  transition: color 180ms ease;
}

.to-move .side-plate-name {
  color: var(--jade-paper);
}

.side-plate-captured {
  font-family: 'Noto Serif SC', 'Songti SC', serif;
  font-size: 1.05rem;
  letter-spacing: 0.08em;
  color: var(--jade-muted);
}

@media (max-width: 1100px) {
  .side-plate-name {
    font-size: 2rem;
  }
}
</style>
