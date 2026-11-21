<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
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

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') settingsOpen.value = false
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
      <NuxtLink to="/" class="jade-brand-link" data-testid="brand-home-link">
        <img src="/assets/brand/mark.svg" alt="" width="26" height="26" class="jade-signet" />
        <!-- The brand name; the glossary's product.title is the descriptive title for metadata. -->
        <span class="jade-brand-title">Jade</span>
      </NuxtLink>

      <nav class="jade-nav" aria-label="Primary">
        <NuxtLink to="/" class="jade-nav-item" data-testid="nav-board">
          {{ t('nav.play') }}
        </NuxtLink>
        <NuxtLink to="/archive" class="jade-nav-item" data-testid="nav-archive">
          {{ t('nav.archive') }}
        </NuxtLink>
      </nav>

      <button
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
      class="jade-settings"
      role="dialog"
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

    <main class="jade-main">
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
