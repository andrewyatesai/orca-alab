// Upstream packaging bug: @xterm/addon-ligatures declares `"main":
// "lib/addon-ligatures.js"` but ships only the `.mjs` entry, so Vite fails to
// resolve the bare import. Fixed locally via config/patches/@xterm__addon-ligatures*.
// Tracking upstream: https://github.com/xtermjs/xterm.js/issues/5822 and
// https://github.com/xtermjs/xterm.js/pull/5828 — drop the patch once that lands.
import { LigaturesAddon } from '@xterm/addon-ligatures'

import type { ManagedPaneInternal } from './pane-manager-types'
import { attachWebgl, disposeWebgl } from './pane-webgl-renderer'

export function disposeLigatures(pane: ManagedPaneInternal): void {
  if (pane.ligaturesAddon) {
    try {
      pane.ligaturesAddon.dispose()
    } catch {
      /* ignore */
    }
    pane.ligaturesAddon = null
  }
}

export function attachLigatures(pane: ManagedPaneInternal): void {
  if (pane.ligaturesAddon) {
    return
  }
  try {
    const ligaturesAddon = new LigaturesAddon()
    pane.terminal.loadAddon(ligaturesAddon)
    pane.ligaturesAddon = ligaturesAddon
    // Why: ligatures can be enabled after rows already rendered, especially
    // from Settings. Force existing glyph runs to be recomputed immediately.
    pane.terminal.refresh(0, pane.terminal.rows - 1)
    // Why: the WebGL renderer builds its glyph texture atlas at activation
    // time, so `font-feature-settings` applied after WebGL loaded won't
    // reach the GPU-rendered cells until the atlas is rebuilt. The upstream
    // docs call this out explicitly — reactivating WebGL after ligatures
    // forces a fresh atlas that includes the ligated glyphs.
    if (pane.webglAddon) {
      disposeWebgl(pane)
      attachWebgl(pane)
    }
  } catch (err) {
    console.warn('[terminal] ligatures addon failed to attach for pane', pane.id, err)
    pane.ligaturesAddon = null
  }
}

/** Enable or disable ligatures in-place, reusing the running terminal so the
 *  setting can be toggled without dropping scrollback or the PTY binding. */
export function setLigaturesEnabled(pane: ManagedPaneInternal, enabled: boolean): void {
  if (enabled) {
    attachLigatures(pane)
  } else if (pane.ligaturesAddon) {
    disposeLigatures(pane)
    // Why: ligatures lived inside the WebGL atlas, so after disposing the
    // addon the atlas still holds the ligated glyphs. Rebuild it so text
    // renders as the non-ligated fallback immediately.
    if (pane.webglAddon) {
      disposeWebgl(pane)
      attachWebgl(pane)
    }
  }
}
