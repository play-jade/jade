<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useArchiveProvider } from '~/utils/archive-provider'
import { pageLinks, queryOffset } from '~/utils/archive-paging'

const route = useRoute()
const provider = useArchiveProvider()
/** The submitted player search lives in the URL, so paging, reload, and the form without scripts agree. */
const submittedQuery = computed(() => (typeof route.query.q === 'string' ? route.query.q : ''))
const searchQuery = ref(submittedQuery.value)
// A query restored by history, or set by a link, replaces what the box shows; typing alone looks nothing up.
watch(submittedQuery, (q) => (searchQuery.value = q))
/** The game page's first game, from a `?offset=` link; the server renders any page. */
const gameOffset = computed(() => queryOffset(route.query.offset))

// The collection's labels and one page of its games, through the archive contract.
const { data: directory, error: directoryError, refresh } = await useAsyncData(
  () => `archive-directory-${gameOffset.value}`,
  async () => {
    const [collection, games] = await Promise.all([
      provider.collection(),
      provider.browseGames({ offset: gameOffset.value }),
    ])
    return { collection, games }
  },
  { watch: [gameOffset] },
)

// Player search loads on its own, so a search neither refetches the directory nor removes it when it fails.
const { data: players, error: playersError, refresh: refreshPlayers } = await useAsyncData(
  () => `archive-players-${submittedQuery.value}`,
  () => provider.findPlayers(submittedQuery.value),
  { watch: [submittedQuery] },
)

const router = useRouter()
function searchPlayers() {
  if (searchQuery.value === submittedQuery.value) void refreshPlayers()
  // Each submitted search is a history entry, as the form without scripts makes one.
  else void router.push({ query: { ...route.query, q: searchQuery.value || undefined } })
}

const pages = computed(() => (directory.value ? pageLinks(directory.value.games) : null))
</script>

<template>
  <div
    v-if="directory"
    class="archive-directory"
    :class="{ 'has-archive-banner': directory.collection.release.is_synthetic_fixture }"
    data-testid="archive-directory"
  >
    <ArchiveProvenanceBanner
      v-if="directory.collection.release.is_synthetic_fixture"
      :dataset-id="directory.collection.release.dataset_id"
      :text="directory.collection.provenance_banner"
    />

    <header class="collection-header">
      <h1 class="collection-title">{{ directory.collection.title }}</h1>
      <p class="muted-note">{{ directory.collection.scope }}</p>
      <p class="muted-note" data-testid="archive-collection-count">
        {{ directory.collection.game_count }}
        {{ directory.collection.release.is_synthetic_fixture ? 'synthetic sample games' : 'games' }} ·
        {{ directory.collection.source.label }}
      </p>
    </header>

    <section class="games" aria-label="Games">
      <h2 class="section-title">Games</h2>
      <p v-if="directory.games.status === 'no_games_found' && !pages?.pastEnd" class="muted-note">No games found.</p>
      <template v-else>
      <ul v-if="!pages?.pastEnd" class="game-list" data-testid="archive-game-list">
        <li v-for="g in directory.games.games" :key="g.game_id" class="game-item">
          <NuxtLink :to="`/archive/g/${g.game_id}`" class="game-link" :data-game-id="g.game_id">
            {{ g.red_display_name }} – {{ g.black_display_name }}
          </NuxtLink>
          <span class="muted-note">{{ g.event_name }} · {{ g.date_display }} · {{ g.recorded_result.replace(/_/g, ' ') }}</span>
        </li>
      </ul>
      <p v-if="pages?.pastEnd" class="muted-note" data-testid="games-past-end">
        This page is past the last of {{ pages.total }} games.
        <NuxtLink :to="{ query: { ...route.query, offset: pages.lastPage } }">Go to the last page</NuxtLink>
      </p>
      <nav
        v-else-if="pages && !pages.pastEnd && (pages.previous !== null || pages.next !== null)"
        class="pager"
        aria-label="Game pages"
      >
        <NuxtLink v-if="pages.previous !== null" :to="{ query: { ...route.query, offset: pages.previous } }">
          ← Previous
        </NuxtLink>
        <span class="muted-note">{{ pages.first }}–{{ pages.last }} of {{ pages.total }}</span>
        <NuxtLink v-if="pages.next !== null" :to="{ query: { ...route.query, offset: pages.next } }" data-testid="games-next">
          Next →
        </NuxtLink>
      </nav>
      </template>
    </section>

    <section class="players" data-testid="player-directory-card" aria-label="Players">
      <h2 class="section-title">Players</h2>
      <form method="get" action="/archive" class="search-form" @submit.prevent="searchPlayers()">
        <input
          v-model="searchQuery"
          name="q"
          type="search"
          aria-label="Search players"
          placeholder="赵汝权, 趙汝權, Chiu, 林明…"
          class="search-input"
          data-testid="player-search-input"
        />
        <!-- Without scripts the form keeps the game page shown, as the scripted search does. -->
        <input v-if="gameOffset > 0" type="hidden" name="offset" :value="gameOffset" />
        <button type="submit" class="jade-btn" data-testid="player-search-btn">Search</button>
      </form>
      <div v-if="playersError" class="muted-note" role="alert" data-testid="player-search-error">
        The player search could not run: {{ playersError.message }}
        <button type="button" class="jade-pill-btn" @click="refreshPlayers()">Try again</button>
      </div>
      <ul v-else class="player-list" data-testid="player-list">
        <li
          v-for="player in players?.players ?? []"
          :key="player.player_id"
          class="player-item"
          :data-player-id="player.player_id"
        >
          <NuxtLink :to="`/players/${player.player_id}`" class="player-link">
            {{ player.native_display_name }} · {{ player.primary_display_name }}
          </NuxtLink>
          <span class="muted-note">{{ player.disambiguation_note }}</span>
        </li>
      </ul>
    </section>
  </div>
  <div v-else class="archive-error" role="alert" data-testid="archive-error">
    <p>The archive could not load{{ directoryError ? `: ${directoryError.message}` : '' }}.</p>
    <button type="button" class="jade-btn" @click="refresh()">Try again</button>
  </div>
</template>

<style scoped>
.archive-directory {
  max-width: 760px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 28px;
}

.collection-title {
  margin: 0 0 6px;
  font-family: var(--jade-display);
  font-weight: 400;
  font-size: 2.2rem;
}

.section-title {
  margin: 0 0 10px;
  font-size: 0.8rem;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--jade-muted);
}

.game-list,
.player-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.game-item,
.player-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.game-link {
  color: var(--jade-paper);
  text-decoration: none;
  font-size: 1rem;
}

.game-link:hover {
  text-decoration: underline;
}

.pager {
  display: flex;
  gap: 14px;
  align-items: center;
  margin-top: 12px;
}

.players {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.search-form {
  display: flex;
  gap: 6px;
}

.search-input {
  flex: 1;
  min-width: 0;
  padding: 7px 10px;
  border-radius: 8px;
  border: 1px solid var(--jade-hairline);
  background: var(--jade-surface);
  color: var(--jade-paper);
  font: inherit;
  font-size: 0.85rem;
}


.player-link {
  color: var(--jade-paper);
  text-decoration: none;
  font-size: 0.92rem;
}

.player-link:hover {
  text-decoration: underline;
}

.muted-note {
  margin: 0;
  font-size: 0.78rem;
  color: var(--jade-muted);
}

.archive-error {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 12px;
}
</style>
