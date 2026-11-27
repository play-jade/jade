import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, devices } from '@playwright/test'

const currentDir = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(currentDir, '../..')

// The journeys start their own match server and web application on ports of their own, and never attach to one that is
// already running: the documented local match server listens on 4180 and the dev web app on 3000, and a test run must
// not drive the instance someone is playing on. A port that is taken stops the run.
const matchServerOrigin = 'http://127.0.0.1:4190'
const webOrigin = 'http://127.0.0.1:3190'
// Playwright lays each server's environment over the caller's, so a setting left out here is still inherited. The match
// server reads few settings, and each one that decides where it listens, what it stores, or whom it answers is pinned
// below. The web application reads any NUXT_ or NITRO_ setting, so it starts with all of the caller's removed and only
// these applied. Each server counts as started only once its own output says it is listening: a URL probe could be
// answered by some other process, and a server that cannot take its port exits, which stops the run.
const webSettings = {
  NITRO_PORT: new URL(webOrigin).port,
  NITRO_HOST: new URL(webOrigin).hostname,
  NITRO_PUBLIC_MATCH_SERVER_URL: matchServerOrigin,
  // The embedded sample collection and no hosted archive behind the proxy routes.
  NITRO_PUBLIC_ARCHIVE_SOURCE: 'sample',
  NITRO_ARCHIVE_API_URL: '',
  // Served at the root of this origin, with its assets, whatever the build was given.
  NITRO_APP_BASE_URL: '/',
  NITRO_APP_CDN_URL: '',
}
// The script is passed in double quotes and holds none itself, nor anything a shell expands inside them, so the same
// command line reads alike in a POSIX shell and in cmd.exe. Names are compared in upper case, as Windows reads a
// setting whatever its case.
const startWebApplication = [
  "for (const name of Object.keys(process.env)) if (/^(NUXT|NITRO)_/.test(name.toUpperCase())) delete process.env[name]",
  'Object.assign(process.env, JSON.parse(process.env.JADE_E2E_WEB_SETTINGS))',
  "import('./apps/web/.output/server/index.mjs')",
].join('; ')

// With PLAYWRIGHT_BASE_URL set, the journeys run against that deployed origin (its match server and web application
// behind one host) and start no local servers. A set value that is not an http(s) URL is refused rather than read as unset,
// so a qualification run never falls back to local servers.
const deployedOrigin = process.env.PLAYWRIGHT_BASE_URL
if (deployedOrigin !== undefined && !['http:', 'https:'].includes(URL.parse(deployedOrigin)?.protocol ?? '')) {
  throw new Error(`PLAYWRIGHT_BASE_URL must be the deployed origin's http(s) URL, not ${JSON.stringify(deployedOrigin)}`)
}

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 45_000,
  use: {
    baseURL: deployedOrigin ?? webOrigin,
    trace: 'off',
  },
  webServer: deployedOrigin !== undefined ? [] : [
    {
      command: 'cargo run -p jade-match-server',
      cwd: repoRoot,
      env: {
        JADE_BIND_ADDR: new URL(matchServerOrigin).host,
        // A database of the run's own, gone when the server stops. A database file setting is read first, so it is
        // blanked, which the server takes as unset.
        DATABASE_URL_FILE: '',
        DATABASE_URL: 'sqlite::memory:',
        JADE_ALLOWED_ORIGINS: webOrigin,
        RUST_LOG: 'jade_match_server=info',
      },
      wait: { stdout: new RegExp(`listening on ${matchServerOrigin}`) },
      reuseExistingServer: false,
      timeout: 90_000,
    },
    {
      // The server starts from the repository root, as a deployment starts it outside apps/web, so a render that
      // resolves files against the working directory fails here too.
      command: `node --input-type=module -e "${startWebApplication}"`,
      cwd: repoRoot,
      env: { JADE_E2E_WEB_SETTINGS: JSON.stringify(webSettings) },
      wait: { stdout: new RegExp(`Listening on ${webOrigin}`) },
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
})
