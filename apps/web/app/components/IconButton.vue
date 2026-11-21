<script setup lang="ts">
/** A round, icon-only control for the glass capsule and replay rows; `label` names it for assistive technology. */
withDefaults(
  defineProps<{
    label: string
    /** SVG path data on a 20 × 20 grid. */
    paths: string[]
    size?: number
    disabled?: boolean
  }>(),
  { size: 20, disabled: false },
)
</script>

<template>
  <button type="button" class="jade-icon-btn" :aria-label="label" :title="label" :disabled="disabled">
    <svg
      :width="size"
      :height="size"
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path v-for="d in paths" :key="d" :d="d" />
    </svg>
  </button>
</template>
