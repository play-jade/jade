<script setup lang="ts">
import { searchSampleArchive } from '~/utils/core-wasm'

const route = useRoute()
const { t } = usePreferences()
const playerId = computed(() => String(route.params.id || ''))

const { data: profile } = await useAsyncData(
  () => `player-profile-${playerId.value}`,
  async () => {
    const res = await searchSampleArchive('')
    const found = res.players.find((p) => p.player_id === playerId.value) ?? null
    return {
      datasetId: res.dataset_id,
      provenanceBanner: res.provenance_banner,
      isSynthetic: res.is_synthetic_fixture,
      player: found,
    }
  },
)
</script>

<template>
  <div class="player-page" data-testid="player-profile-page">
    <div v-if="profile?.isSynthetic" class="provenance-note">
      <strong>{{ profile.datasetId }}:</strong> {{ profile.provenanceBanner }}
    </div>

    <section v-if="profile?.player" class="jade-card">
      <p class="canonical-id">Canonical ID: <code>{{ profile.player.player_id }}</code></p>
      <h1 class="player-heading">
        {{ profile.player.native_display_name }} · {{ profile.player.primary_display_name }}
      </h1>
      <p class="disambig">{{ profile.player.disambiguation_note }}</p>

      <h2>Verified &amp; Fixture Aliases</h2>
      <table class="alias-table">
        <thead>
          <tr>
            <th>Name / Alias</th>
            <th>Script</th>
            <th>Source</th>
            <th>Review status</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(al, idx) in profile.player.aliases" :key="idx">
            <td><strong>{{ al.text }}</strong></td>
            <td>{{ al.script }}</td>
            <td><code>{{ al.source }}</code></td>
            <td>{{ al.verification_state }}</td>
          </tr>
        </tbody>
      </table>

      <div class="profile-actions">
        <NuxtLink to="/archive" class="jade-btn jade-btn-primary">
          ← Back to Archive
        </NuxtLink>
        <span class="correction-hint">{{ t('archive.identity_correction') }} (queued for review)</span>
      </div>
    </section>

    <section v-else class="jade-card">
      <h1>Player not found</h1>
      <NuxtLink to="/archive">Return to Sample Archive</NuxtLink>
    </section>
  </div>
</template>

<style scoped>
.player-page {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.provenance-note {
  padding: 10px 14px;
  border-radius: 8px;
  background: rgba(243, 238, 228, 0.12);
  font-size: 0.85rem;
}

.canonical-id {
  margin: 0;
  font-size: 0.8rem;
  opacity: 0.72;
}

.player-heading {
  margin: 6px 0;
  font-size: 1.4rem;
}

.disambig {
  margin: 0 0 16px;
  font-size: 0.92rem;
}

.alias-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.88rem;
  margin-bottom: 16px;
}

.alias-table th,
.alias-table td {
  text-align: left;
  padding: 6px 8px;
  border-bottom: 1px solid rgba(243, 238, 228, 0.18);
}

.profile-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 14px;
}

.correction-hint {
  font-size: 0.82rem;
  opacity: 0.76;
}
</style>
