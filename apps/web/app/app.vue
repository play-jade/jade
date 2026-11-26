<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { SUPPORTED_LOCALES } from '~/utils/i18n'

const {
  locale,
  pieceScript,
  material,
  darkSurface,
  notationMode,
  soundEnabled,
  t,
} = usePreferences()

const settingsOpen = ref(false)
const settingsToggle = ref<HTMLButtonElement | null>(null)
const settingsSheet = ref<HTMLElement | null>(null)

function sheetControls(): HTMLElement[] {
  return Array.from(settingsSheet.value?.querySelectorAll<HTMLElement>('select, button') ?? [])
}

// A modal sheet: focus moves into it on opening, Tab stays inside it while the page behind it is inert, and focus
// returns to the Settings button on closing.
watch(settingsOpen, async (open) => {
  await nextTick()
  if (open) sheetControls()[0]?.focus()
  else settingsToggle.value?.focus()
})

function onKeydown(e: KeyboardEvent) {
  if (!settingsOpen.value) return
  if (e.key === 'Escape') {
    settingsOpen.value = false
    return
  }
  if (e.key !== 'Tab') return
  const controls = sheetControls()
  if (controls.length === 0) return
  const first = controls[0]
  const last = controls[controls.length - 1]
  if (e.shiftKey && document.activeElement === first) {
    e.preventDefault()
    last.focus()
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault()
    first.focus()
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

useHead(() => ({
  htmlAttrs: {
    lang: locale.value,
    'data-theme': darkSurface.value ? 'dark' : 'paper',
  },
}))
</script>

<template>
  <div class="jade-shell">
    <div class="jade-ground" aria-hidden="true" />

    <header class="jade-topbar">
      <NuxtLink to="/" class="jade-brand-link" data-testid="brand-home-link" :inert="settingsOpen">
        <img src="/assets/brand/mark.svg" alt="" width="26" height="26" class="jade-signet" />
        <!-- The brand name; the glossary's product.title is the descriptive title for metadata. -->
        <span class="jade-brand-title">Jade</span>
      </NuxtLink>

      <nav class="jade-nav" aria-label="Primary" :inert="settingsOpen">
        <NuxtLink to="/" class="jade-nav-item" data-testid="nav-board">
          {{ t('nav.play') }}
        </NuxtLink>
        <NuxtLink to="/archive" class="jade-nav-item" data-testid="nav-archive">
          {{ t('nav.archive') }}
        </NuxtLink>
      </nav>

      <button
        ref="settingsToggle"
        type="button"
        class="jade-icon-btn jade-settings-toggle"
        data-testid="settings-toggle"
        aria-label="Settings"
        aria-controls="jade-settings"
        :aria-expanded="settingsOpen"
        @click="settingsOpen = !settingsOpen"
      >
        <svg width="18" height="18" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
          <circle cx="10" cy="10" r="2.6" />
          <path
            d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4"
            stroke-linecap="round"
          />
        </svg>
      </button>
    </header>

    <!-- Preferences live in one sheet, as macOS keeps them in its Settings window. -->
    <div v-show="settingsOpen" class="jade-settings-scrim" @click="settingsOpen = false" />
    <section
      v-show="settingsOpen"
      id="jade-settings"
      ref="settingsSheet"
      class="jade-settings"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      data-testid="settings-sheet"
    >
      <label class="jade-setting">
        <span>Language</span>
        <select v-model="locale" data-testid="pref-locale" class="jade-select">
          <option v-for="loc in SUPPORTED_LOCALES" :key="loc.id" :value="loc.id">
            {{ loc.label }}
          </option>
        </select>
      </label>
      <label class="jade-setting">
        <span>Piece faces</span>
        <select v-model="pieceScript" data-testid="pref-script" class="jade-select">
          <option value="auto">Follow language</option>
          <option value="simplified">简体字</option>
          <option value="traditional">繁體字</option>
        </select>
      </label>
      <label class="jade-setting">
        <span>Piece set</span>
        <select v-model="material" data-testid="pref-material" class="jade-select">
          <option value="pale-jade">Pale Jade</option>
          <option value="boxwood">Honey Boxwood</option>
          <option value="ivory-stone">Ivory Stone</option>
        </select>
      </label>
      <label class="jade-setting">
        <span>Notation</span>
        <select v-model="notationMode" data-testid="pref-notation" class="jade-select">
          <option value="auto">Follow language</option>
          <option value="wxf">WXF</option>
          <option value="chinese">中文纵线</option>
          <option value="uci">Coordinates</option>
        </select>
      </label>
      <div class="jade-setting">
        <span>Board surface</span>
        <button
          type="button"
          class="jade-btn"
          data-testid="pref-theme"
          :aria-pressed="darkSurface"
          @click="darkSurface = !darkSurface"
        >
          {{ darkSurface ? 'Dark' : 'Paper' }}
        </button>
      </div>
      <div class="jade-setting">
        <span>Piece sounds</span>
        <button
          type="button"
          class="jade-btn"
          data-testid="pref-sound"
          :aria-pressed="soundEnabled"
          @click="soundEnabled = !soundEnabled"
        >
          {{ soundEnabled ? 'Sound On' : 'Muted' }}
        </button>
      </div>
    </section>

    <main class="jade-main" :inert="settingsOpen">
      <NuxtPage />
    </main>
  </div>
</template>

<style>
/* Lamplit-table tokens, shared in value with the macOS client's LamplitTheme. */
:root {
  --jade-ground: #1D2421;
  --jade-paper: #F3EEE4;
  --jade-pale: #DCE3CE;
  --jade-deep: #415447;
  --jade-ink: #252A25;
  --jade-vermilion: #A54837;
  /* Vermilion lightened for small text on the dark ground. */
  --jade-vermilion-text: #E69383;
  --jade-muted: rgba(243, 238, 228, 0.66);
  --jade-hairline: rgba(243, 238, 228, 0.12);
  --jade-surface: rgba(243, 238, 228, 0.045);
  --jade-surface-hover: rgba(243, 238, 228, 0.09);
  --jade-current-bg: #DCE3CE;
  --jade-current-ink: #1D2421;
  --jade-display: 'Jade Display', 'Noto Serif Display', 'New York', Georgia, serif;
}

@font-face {
  font-family: 'Jade Display';
  src: url('/assets/brand/source/fonts/NotoSerifDisplay-Regular.ttf') format('truetype');
  font-display: swap;
}

*,
*::before,
*::after {
  box-sizing: border-box;
}

html,
body {
  margin: 0;
  padding: 0;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans CJK SC", sans-serif;
  background: var(--jade-ground);
  color: var(--jade-paper);
  -webkit-font-smoothing: antialiased;
  color-scheme: dark;
}

.jade-shell {
  position: relative;
  min-height: 100vh;
  max-width: 100vw;
  overflow-x: clip;
  color: var(--jade-paper);
}

/* Dark grained ground under a warm pool of key light from the upper left, with a soft vignette. */
.jade-ground {
  position: fixed;
  inset: 0;
  z-index: -1;
  background-color: var(--jade-ground);
  background-image:
    radial-gradient(ellipse 70vw 70vh at 40% 34%, rgba(243, 238, 228, 0.085), rgba(243, 238, 228, 0) 70%),
    radial-gradient(ellipse 95vw 95vh at 50% 50%, rgba(0, 0, 0, 0) 55%, rgba(0, 0, 0, 0.55) 100%),
    url('/assets/board/materials/dark-grain.png');
  background-repeat: no-repeat, no-repeat, repeat;
}

.jade-topbar {
  max-width: 1440px;
  margin: 0 auto;
  padding: 14px 24px;
  display: flex;
  align-items: center;
  gap: 28px;
}

.jade-brand-link {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  text-decoration: none;
  color: inherit;
}

.jade-brand-title {
  font-family: var(--jade-display);
  font-size: 1.45rem;
  line-height: 1;
  letter-spacing: 0.01em;
}

.jade-signet {
  display: block;
  border-radius: 50%;
}

.jade-nav {
  display: inline-flex;
  gap: 4px;
  margin-right: auto;
}

.jade-nav-item {
  padding: 6px 10px;
  border-radius: 6px;
  text-decoration: none;
  color: var(--jade-muted);
  font-size: 0.9rem;
}

.jade-nav-item:hover {
  color: var(--jade-paper);
}

.jade-nav-item.router-link-exact-active,
.jade-nav-item[href='/archive'].router-link-active {
  color: var(--jade-paper);
  box-shadow: inset 0 -1px 0 var(--jade-pale);
  border-radius: 0;
}

.jade-icon-btn {
  appearance: none;
  width: 40px;
  height: 40px;
  border: 0;
  border-radius: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: rgba(243, 238, 228, 0.82);
  cursor: pointer;
}

.jade-icon-btn:hover,
.jade-icon-btn[aria-expanded='true'] {
  background: var(--jade-surface-hover);
  color: var(--jade-paper);
}

.jade-icon-btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.jade-icon-btn:focus-visible,
.jade-btn:focus-visible,
.jade-select:focus-visible {
  outline: 2px solid var(--jade-pale);
  outline-offset: 2px;
}

.jade-settings-scrim {
  position: fixed;
  inset: 0;
  z-index: 40;
}

.jade-settings {
  position: fixed;
  top: 60px;
  right: max(16px, calc((100vw - 1440px) / 2 + 24px));
  z-index: 41;
  width: min(340px, calc(100vw - 32px));
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  border-radius: 14px;
  background: rgba(29, 36, 33, 0.82);
  backdrop-filter: blur(24px) saturate(140%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  box-shadow: inset 0 0 0 1px var(--jade-hairline), 0 18px 40px rgba(0, 0, 0, 0.45);
}

.jade-setting {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 0.86rem;
  color: var(--jade-muted);
}

.jade-setting .jade-select,
.jade-setting .jade-btn {
  min-width: 160px;
}

.jade-select,
.jade-pill-btn,
.jade-btn {
  font: inherit;
  font-size: 0.86rem;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--jade-hairline);
  background: var(--jade-surface);
  color: var(--jade-paper);
  cursor: pointer;
}

.jade-select option {
  background: var(--jade-ground);
  color: var(--jade-paper);
}

.jade-btn:hover:not(:disabled),
.jade-pill-btn:hover:not(:disabled) {
  background: var(--jade-surface-hover);
}

.jade-btn:disabled,
.jade-pill-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.jade-btn-primary {
  background: var(--jade-pale);
  color: var(--jade-current-ink);
  border-color: var(--jade-pale);
  font-weight: 600;
}

.jade-btn-primary:hover:not(:disabled) {
  background: var(--jade-paper);
}

.jade-btn-danger {
  border-color: rgba(230, 147, 131, 0.45);
  color: var(--jade-vermilion-text);
}

.jade-main {
  max-width: 1440px;
  margin: 0 auto;
  padding: 8px 24px 32px;
}

.jade-card {
  border-radius: 12px;
  padding: 16px;
  background: var(--jade-surface);
  box-shadow: inset 0 0 0 1px var(--jade-hairline);
}

/* Room below an archive page's content for its overlaid sample banner, kept after dismissal so the page height
   never changes. */
.has-archive-banner {
  padding-bottom: 96px;
}

/* The lamplit table, shared by every page where a game is played: side plates, the board as the lit object,
   and a quiet ledger of moves. */
.jade-table {
  display: grid;
  grid-template-columns: minmax(180px, 240px) minmax(0, 720px) minmax(240px, 300px);
  justify-content: center;
  gap: 40px;
  align-items: stretch;
  min-height: calc(100vh - 110px);
}

.jade-plates {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 16px;
  padding: 24px 0 72px;
}

/* The slab fits the window height with the capsule below it, as macOS fits it to the window. */
.jade-table-stage {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 18px;
  padding-top: 8px;
  min-width: 0;
  width: 100%;
  max-width: min(720px, calc((100vh - 170px) / 1.11));
  justify-self: center;
}

.jade-capsule {
  display: inline-flex;
  flex-wrap: wrap;
  justify-content: center;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border-radius: 999px;
  background: rgba(29, 36, 33, 0.62);
  backdrop-filter: blur(24px) saturate(140%);
  -webkit-backdrop-filter: blur(24px) saturate(140%);
  box-shadow: inset 0 0 0 1px var(--jade-hairline), 0 10px 30px rgba(0, 0, 0, 0.4);
}

.jade-capsule .jade-icon-btn {
  width: 44px;
  height: 44px;
  border-radius: 22px;
}

.jade-capsule .jade-btn {
  border-radius: 999px;
  border-color: transparent;
  background: transparent;
}

.jade-capsule .jade-btn-primary {
  background: var(--jade-pale);
}

.jade-ledger {
  display: flex;
  flex-direction: column;
  gap: 18px;
  padding: 24px 0 72px;
  min-width: 0;
}

.jade-move-list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 46vh;
  overflow-y: auto;
  font-variant-numeric: tabular-nums;
}

.jade-move-row {
  display: grid;
  grid-template-columns: 2.2em minmax(0, 1fr) minmax(0, 1fr);
  align-items: baseline;
}

.jade-move-number {
  font-size: 0.75rem;
  color: var(--jade-muted);
}

.jade-move {
  appearance: none;
  border: 0;
  border-radius: 6px;
  padding: 6px 8px;
  background: transparent;
  color: rgba(243, 238, 228, 0.82);
  font: inherit;
  font-size: 0.95rem;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
}

.jade-move:hover:not(:disabled):not(.current) {
  background: var(--jade-surface-hover);
}

.jade-move.current {
  color: var(--jade-paper);
  font-weight: 600;
  text-decoration: underline;
  text-decoration-color: rgba(220, 227, 206, 0.6);
  text-underline-offset: 6px;
}

.jade-move:disabled {
  cursor: default;
}

.jade-rule {
  padding-top: 14px;
  border-top: 1px solid var(--jade-hairline);
}

/* Narrow windows: the board stays the main object; plates sit above it and the ledger follows. */
@media (max-width: 1100px) {
  .jade-table {
    grid-template-columns: minmax(0, 1fr);
    gap: 14px;
    min-height: 0;
  }

  .jade-plates {
    flex-direction: row;
    padding: 0;
  }

  .jade-ledger {
    padding: 0;
  }
}

@media (max-width: 640px) {
  .jade-topbar {
    padding: 10px 16px;
    gap: 14px;
  }

  .jade-main {
    padding: 4px 16px 24px;
  }
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
</style>
