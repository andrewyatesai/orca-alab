import { availableParallelism } from 'node:os'
import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'

// Why: a Vitest worker is not this suite's unit of concurrency. Many files also
// start git, HTTP, watcher, and native-addon child processes, so using Vitest's
// CPU-count default can exhaust process/file-descriptor headroom on developer
// machines and CI. Four workers keeps those nested fixtures concurrent without
// the nine-fork startup/termination failures seen on a 10-core host.
const maxTestWorkers = Math.max(1, Math.min(4, availableParallelism()))

export default defineConfig({
  define: {
    ORCA_FEATURE_WALL_ENABLED: 'true'
  },
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@': resolve('src/renderer/src')
    }
  },
  test: {
    environment: 'node',
    // Ensure DOM tests have a working Web Storage API on Node 26 (see the setup).
    // The seam setup binds the Rust dispatch core so cut-over src/shared modules
    // work without each surface's production bootstrap.
    setupFiles: [
      resolve('config/vitest-warning-filter.ts'),
      resolve('config/vitest-dom-storage-polyfill.ts'),
      resolve('config/vitest-orca-dispatch-seam.ts')
    ],
    include: [
      'src/**/*.test.ts',
      'src/**/*.test.tsx',
      'config/scripts/**/*.test.mjs',
      'tests/e2e/**/*.unit.test.ts'
    ],
    // Why: the full suite runs heavy TS transforms plus real git/http fixtures;
    // the Vitest 5s defaults are too tight for the slowest integration cases.
    hookTimeout: 60_000,
    testTimeout: 30_000,
    maxWorkers: maxTestWorkers
  }
})
