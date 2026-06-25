import { test } from './helpers/orca-app'

// RETIRED: this spec drove xterm's @xterm/addon-webgl glyph-atlas recovery after
// an image clipboard paste (patching webglAddon.clearTextureAtlas and asserting
// it fired). aterm — now the only terminal engine — has no shared WebGL glyph
// atlas to clear: its GPU drawer re-presents the engine grid every frame, and a
// lost WebGL2 context is handled by an automatic GPU→CPU swap. The real aterm
// GPU/context-loss behavior is covered end-to-end by
// tests/e2e/aterm-gpu-context-loss.spec.ts.
test.describe
  .skip('terminal image paste WebGL recovery (retired — see aterm-gpu-context-loss.spec.ts)', () => {
  test('retired', () => {})
})
