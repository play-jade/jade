<script setup lang="ts">
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

useHead(() => ({
  htmlAttrs: {
    lang: locale.value,
    'data-theme': darkSurface.value ? 'dark' : 'paper',
  },
}))
</script>

<template>
  <div class="jade-shell" :class="{ 'theme-dark': darkSurface }">
    <header class="jade-topbar">
      <div class="jade-brand-group">
        <NuxtLink to="/" class="jade-brand-link" data-testid="brand-home-link">
          <img
            src="/assets/brand/mark.svg"
            alt="Jade signet"
            width="30"
            height="30"
            class="jade-signet"
          />
          <span class="jade-brand-title">{{ t('product.title') }}</span>
        </NuxtLink>

        <nav class="jade-nav" aria-label="Primary">
          <NuxtLink to="/" class="jade-nav-item" data-testid="nav-board">
            {{ t('nav.play') }}
          </NuxtLink>
          <NuxtLink to="/archive" class="jade-nav-item" data-testid="nav-archive">
            {{ t('nav.archive') }}
          </NuxtLink>
        </nav>
      </div>

      <div class="jade-prefs-bar" role="group" aria-label="Presentation preferences">
        <label class="jade-pref-field">
          <span class="sr-only">Locale</span>
          <select v-model="locale" data-testid="pref-locale" class="jade-select">
            <option v-for="loc in SUPPORTED_LOCALES" :key="loc.id" :value="loc.id">
              {{ loc.label }}
            </option>
          </select>
        </label>

        <label class="jade-pref-field">
          <span class="sr-only">Piece script</span>
          <select v-model="pieceScript" data-testid="pref-script" class="jade-select">
            <option value="auto">Script: Auto</option>
            <option value="simplified">简体字</option>
            <option value="traditional">繁體字</option>
          </select>
        </label>

        <label class="jade-pref-field">
          <span class="sr-only">Piece material</span>
          <select v-model="material" data-testid="pref-material" class="jade-select">
            <option value="pale-jade">Pale Jade</option>
            <option value="boxwood">Boxwood</option>
            <option value="ivory-stone">Ivory Stone</option>
          </select>
        </label>

        <label class="jade-pref-field">
          <span class="sr-only">Notation</span>
          <select v-model="notationMode" data-testid="pref-notation" class="jade-select">
            <option value="chinese">中文纵线</option>
            <option value="uci">Coord (UCI)</option>
          </select>
        </label>

        <button
          type="button"
          class="jade-pill-btn"
          data-testid="pref-theme"
          :aria-pressed="darkSurface"
          @click="darkSurface = !darkSurface"
        >
          {{ darkSurface ? 'Dark' : 'Paper' }}
        </button>

        <button
          type="button"
          class="jade-pill-btn"
          data-testid="pref-sound"
          :aria-pressed="soundEnabled"
          @click="soundEnabled = !soundEnabled"
        >
          {{ soundEnabled ? 'Sound On' : 'Muted' }}
        </button>
      </div>
    </header>

    <main class="jade-main">
      <NuxtPage />
    </main>
  </div>
</template>

<style>
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
  background: #F3EEE4;
  color: #252A25;
  -webkit-font-smoothing: antialiased;
}

.jade-shell {
  min-height: 100vh;
  max-width: 100vw;
  overflow-x: clip;
  background: #F3EEE4;
  color: #252A25;
  transition: background-color 140ms ease, color 140ms ease;
}

.jade-shell.theme-dark {
  background: #1D2421;
  color: #F3EEE4;
}

.jade-topbar {
  max-width: 1180px;
  margin: 0 auto;
  padding: 12px 18px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid rgba(65, 84, 71, 0.18);
}

.theme-dark .jade-topbar {
  border-bottom-color: rgba(220, 227, 206, 0.16);
}

.jade-brand-group {
  display: flex;
  align-items: center;
  gap: 18px;
}

.jade-brand-link {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  text-decoration: none;
  color: inherit;
  font-weight: 650;
  letter-spacing: 0.01em;
}

.jade-signet {
  border-radius: 7px;
  display: block;
}

.jade-nav {
  display: inline-flex;
  gap: 8px;
}

.jade-nav-item {
  padding: 6px 12px;
  border-radius: 6px;
  text-decoration: none;
  color: inherit;
  font-size: 0.92rem;
  font-weight: 500;
}

.jade-nav-item.router-link-active {
  background: rgba(65, 84, 71, 0.14);
  color: #415447;
  font-weight: 600;
}

.theme-dark .jade-nav-item.router-link-active {
  background: rgba(220, 227, 206, 0.16);
  color: #DCE3CE;
}

.jade-prefs-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.jade-select,
.jade-pill-btn,
.jade-btn {
  font: inherit;
  font-size: 0.84rem;
  padding: 6px 10px;
  border-radius: 6px;
  border: 1px solid rgba(65, 84, 71, 0.28);
  background: rgba(255, 255, 255, 0.56);
  color: inherit;
  cursor: pointer;
}

.theme-dark .jade-select,
.theme-dark .jade-pill-btn,
.theme-dark .jade-btn {
  background: rgba(37, 42, 37, 0.72);
  border-color: rgba(220, 227, 206, 0.24);
  color: #F3EEE4;
}

.jade-btn-primary {
  background: #415447;
  color: #F3EEE4;
  border-color: #415447;
  font-weight: 600;
}

.theme-dark .jade-btn-primary {
  background: #DCE3CE;
  color: #1D2421;
  border-color: #DCE3CE;
}

.jade-btn-danger {
  border-color: rgba(165, 72, 55, 0.55);
  color: #A54837;
}

.theme-dark .jade-btn-danger {
  color: #E69383;
}

.jade-main {
  max-width: 1180px;
  margin: 0 auto;
  padding: 18px;
}

.jade-card {
  border-radius: 10px;
  padding: 16px;
  background: rgba(255, 255, 255, 0.52);
  border: 1px solid rgba(65, 84, 71, 0.18);
}

.theme-dark .jade-card {
  background: rgba(37, 42, 37, 0.65);
  border-color: rgba(220, 227, 206, 0.16);
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
