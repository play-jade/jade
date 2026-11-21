<script setup lang="ts">
const route = useRoute()
const matchId = computed(() => String(route.params.id || ''))
</script>

<template>
  <section class="jade-card">
    <h1>Match Record · {{ matchId }}</h1>
    <p>
      Personal matches are private by default unless both participants explicitly share them.
      Open your invitation link or return to the <NuxtLink to="/">Xiangqi board</NuxtLink>.
    </p>
  </section>
</template>
