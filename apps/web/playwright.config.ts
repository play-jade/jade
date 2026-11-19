import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { defineConfig, devices } from '@playwright/test'

const currentDir = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(currentDir, '../..')
const fastTargetDir = existsSync('/tmp/jade-target-core') ? '/tmp/jade-target-core' : undefined

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: false,
  workers: 1,
  timeout: 45_000,
  use: {
    baseURL: process.env.PLAYWRIGHT_BASE_URL || 'http://127.0.0.1:3180',
    trace: 'off',
  },
  webServer: [
    {
      command: 'cargo run -p jade-match-server',
      cwd: repoRoot,
      env: {
        ...process.env,
        PATH: `/Users/andrew/.cargo/bin:${process.env.PATH || ''}`,
        ...(fastTargetDir ? { CARGO_TARGET_DIR: fastTargetDir } : {}),
        JADE_BIND_ADDR: '127.0.0.1:4180',
        JADE_SQLITE_URL: 'sqlite::memory:',
      },
      url: 'http://127.0.0.1:4180/healthz',
      reuseExistingServer: true,
      timeout: 90_000,
    },
    {
      command: 'node .output/server/index.mjs',
      cwd: currentDir,
      env: {
        ...process.env,
        PORT: '3180',
        HOST: '127.0.0.1',
        NUXT_PUBLIC_MATCH_SERVER_URL: 'http://127.0.0.1:4180',
      },
      url: 'http://127.0.0.1:3180',
      reuseExistingServer: true,
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
