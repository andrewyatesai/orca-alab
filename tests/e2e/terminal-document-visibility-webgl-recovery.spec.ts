import { test } from './helpers/orca-app'

// RETIRED: this spec drove xterm's @xterm/addon-webgl glyph-atlas recovery
// (patching webglAddon.clearTextureAtlas and asserting it fired on a document
// visibility resume). aterm — now the only terminal engine — has no shared
// WebGL glyph atlas to clear: its GPU drawer re-presents the engine grid every
// frame, and a lost WebGL2 context is handled by an automatic GPU→CPU swap.
// The real aterm GPU/context-loss behavior is covered end-to-end by
// tests/e2e/aterm-gpu-context-loss.spec.ts (forces a real webglcontextlost,
// asserts the pane swaps to the CPU 2d path and keeps rendering).
test.describe
  .skip('terminal document visibility WebGL recovery (retired — see aterm-gpu-context-loss.spec.ts)', () => {
  test('retired', () => {})
})
