import { test } from './helpers/orca-app'

// RETIRED: this spec reproduced cross-terminal corruption from @xterm/addon-webgl's
// module-global shared glyph atlas (terminals with identical font configs shared
// one texture atlas; clearing it through one terminal garbled the others). aterm
// — now the only terminal engine — has no shared module-global glyph atlas: each
// pane's GPU drawer owns its own WebGL2 surface and re-presents the engine grid
// every frame, so there is no cross-pane atlas to share or corrupt. The real
// aterm GPU behavior (per-pane WebGL2 draw path + GPU→CPU context-loss swap with
// canvas-pixel correctness) is covered by tests/e2e/aterm-gpu-context-loss.spec.ts.
test.describe
  .skip('floating workspace shared glyph atlas (retired — see aterm-gpu-context-loss.spec.ts)', () => {
  test('retired', () => {})
})
