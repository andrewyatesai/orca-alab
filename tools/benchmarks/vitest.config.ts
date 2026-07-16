import { defineConfig } from 'vitest/config'

// The root config's include globs cover src/** and config/scripts/** — NOT
// tools/benchmarks — so the perf-proof lane's unit tests get their own config
// (same pattern as config/vitest.parity.config.ts for tools/parity):
//   node_modules/.bin/vitest run --config tools/benchmarks/vitest.config.ts
export default defineConfig({
  test: {
    environment: 'node',
    include: ['tools/benchmarks/**/*.test.mjs']
  }
})
