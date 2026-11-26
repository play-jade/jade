import { resolve } from 'node:path'

const enableCrossOriginIsolation = process.env.JADE_COOP_COEP === '1'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: true,
  telemetry: false,
  devtools: { enabled: false },
  runtimeConfig: {
    // The hosted archive's base URL stays server-side (`NUXT_ARCHIVE_API_URL`); the browser reads it
    // through the same-origin `/api/archive` routes.
    archiveApiUrl: '',
    public: {
      matchServerUrl: process.env.NUXT_PUBLIC_MATCH_SERVER_URL || 'http://127.0.0.1:4180',
      rulesProfile: 'program-2023.11-v1',
      // Selects what `useArchiveProvider` reads (`NUXT_PUBLIC_ARCHIVE_SOURCE`): `sample` serves the embedded
      // synthetic collection; `hosted` reads the configured archive and reports it unavailable rather than
      // substituting sample records. Any other value is a configuration error.
      archiveSource: 'sample',
    },
  },
  app: {
    head: {
      title: 'Jade',
      htmlAttrs: { lang: 'en-US' },
      meta: [
        { charset: 'utf-8' },
        { name: 'viewport', content: 'width=device-width, initial-scale=1, viewport-fit=cover' },
        { name: 'description', content: 'Jade is a calm, tactile place to play and explore Xiangqi.' },
      ],
      link: [
        { rel: 'icon', type: 'image/svg+xml', href: '/assets/brand/mark.svg' },
        { rel: 'stylesheet', href: '/assets/brand/palette.css' },
      ],
    },
  },
  nitro: {
    publicAssets: [
      {
        dir: resolve(__dirname, '../../assets'),
        baseURL: '/assets',
        maxAge: 3600,
      },
    ],
    routeRules: enableCrossOriginIsolation
      ? {
          '/**': {
            headers: {
              'Cross-Origin-Opener-Policy': 'same-origin',
              'Cross-Origin-Embedder-Policy': 'require-corp',
            },
          },
        }
      : {},
  },
})
