import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'

// Dedicated config for the TS↔Rust differential parity suite. The base
// config/vitest.config.ts include globs cover only src/**, config/scripts/**,
// and tests/e2e/** — NOT tools/parity — so `pnpm test` never ran this suite and
// the documented `vitest --config config/vitest.config.ts tools/parity/...`
// silently matched nothing (a positional path is a filter over `include`, not an
// override). This config includes the parity driver so `pnpm parity` runs it.
export default defineConfig({
  resolve: {
    alias: {
      '@renderer': resolve('src/renderer/src'),
      '@': resolve('src/renderer/src')
    }
  },
  test: {
    environment: 'node',
    include: ['tools/parity/**/*.test.ts'],
    testTimeout: 30_000
  }
})
