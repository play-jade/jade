import { resolve } from 'node:path'

const enableCrossOriginIsolation = process.env.JADE_COOP_COEP === '1'

export default defineNuxtConfig({
  compatibilityDate: '2025-07-15',
  ssr: true,
  telemetry: false,
  devtools: { enabled: false },
  runtimeConfig: {
    public: {
      matchServerUrl: process.env.NUXT_PUBLIC_MATCH_SERVER_URL || 'http://127.0.0.1:4180',
      rulesProfile: 'program-2023.11-v1',
    },
  },
  app: {
    head: {
      title: 'Jade — Xiangqi · 玉枰象棋',
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
