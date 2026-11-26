<script setup lang="ts">
import { ArchiveRequestError, useArchiveProvider } from '~/utils/archive-provider'
import { pageLinks, queryOffset } from '~/utils/archive-paging'

const route = useRoute()
const { t } = usePreferences()
const provider = useArchiveProvider()
const playerId = computed(() => String(route.params.id || ''))
const gameOffset = computed(() => queryOffset(route.query.offset))

// The player and one page of their games through the archive contract. Only a missing player is a not-found
// page; an archive that does not answer is an error with a retry.
const { data: profile, error: profileError, refresh } = await useAsyncData(
  () => `player-profile-${playerId.value}-${gameOffset.value}`,
  async () => {
    const collection = await provider.collection()
    const found = await provider.playerProfile(playerId.value, { offset: gameOffset.value }).catch((error: unknown) => {
      if (error instanceof ArchiveRequestError && error.code === 'archive_not_found') return null
      throw error
    })
    return {
      datasetId: collection.release.dataset_id,
      provenanceBanner: collection.provenance_banner,
      isSynthetic: collection.release.is_synthetic_fixture,
      player: found?.player ?? null,
      games: found?.games ?? null,
    }
  },
  { watch: [gameOffset] },
)

const pages = computed(() => (profile.value?.games ? pageLinks(profile.value.games) : null))
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

      <h2>Games</h2>
      <ul class="player-games" data-testid="player-game-list">
        <li v-for="g in profile.games?.games ?? []" :key="g.game_id">
          <NuxtLink :to="`/archive/g/${g.game_id}`">{{ g.red_display_name }} – {{ g.black_display_name }}</NuxtLink>
          · {{ g.event_name }} · {{ g.date_display }}
        </li>
      </ul>
      <p v-if="pages?.pastEnd" class="disambig" data-testid="player-games-past-end">
        This page is past the last of this player's {{ pages.total }} games.
        <NuxtLink :to="{ query: { offset: pages.lastPage } }">Go to the last page</NuxtLink>
      </p>
      <nav v-else-if="pages && (pages.previous !== null || pages.next !== null)" class="profile-actions" aria-label="Game pages">
        <NuxtLink v-if="pages.previous !== null" :to="{ query: { offset: pages.previous } }">← Previous</NuxtLink>
        <NuxtLink v-if="pages.next !== null" :to="{ query: { offset: pages.next } }">Next →</NuxtLink>
      </nav>

      <div class="profile-actions">
        <NuxtLink to="/archive" class="jade-btn jade-btn-primary">
          ← Back to Archive
        </NuxtLink>
        <span class="correction-hint">{{ t('archive.identity_correction') }} (queued for review)</span>
      </div>
    </section>

    <section v-else-if="profileError" class="jade-card" role="alert" data-testid="player-profile-error">
      <h1>This player could not load</h1>
      <p>{{ profileError.message }}</p>
      <button type="button" class="jade-btn" @click="refresh()">Try again</button>
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
