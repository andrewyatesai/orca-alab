import { test } from './helpers/orca-app'

// RETIRED: this spec corrupted xterm's live @xterm/addon-webgl glyph-atlas
// textures (texSubImage2D noise into bound TEXTURE_2D) and asserted that
// reopening the floating panel recovered them via clearTextureAtlas /
// resumeRendering. aterm — now the only terminal engine — has no shared WebGL
// glyph atlas whose textures can be corrupted: its GPU drawer re-presents the
// engine grid every frame, and a lost WebGL2 context is handled by an automatic
// GPU→CPU swap. The real aterm GPU/context-loss recovery is covered end-to-end
// by tests/e2e/aterm-gpu-context-loss.spec.ts.
test.describe
  .skip('floating workspace reopen WebGL recovery (retired — see aterm-gpu-context-loss.spec.ts)', () => {
  test('retired', () => {})
})
