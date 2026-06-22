import { useAppStore } from '@/store'

// Module-const fallback so the renderer can be force-enabled without settings
// or env wiring (flip locally during bring-up). Keep false on commit.
const ENABLE_ATERM_RENDERER = false

/** True when terminal panes should paint via the in-page aterm canvas renderer
 *  instead of xterm.js. Default ON (opt-out); a force-OFF window flag, the
 *  experimental setting set to false, the const, a Vite build env override, or
 *  an explicit force-ON window flag can change it. */
export function isAtermRendererEnabled(): boolean {
  // Explicit force-ON window flag wins over everything below — the aterm e2e
  // specs set this and must opt in even when the suite-wide disable is active.
  if (typeof window !== 'undefined' && window.__atermRendererEnabled === true) {
    return true
  }
  // Force-OFF escape hatch for the existing e2e suite (which asserts via the
  // xterm DOM). Checked after the explicit force-ON so ON always wins.
  if (typeof window !== 'undefined' && window.__atermRendererDisabled === true) {
    return false
  }
  if (ENABLE_ATERM_RENDERER) {
    return true
  }
  if (import.meta.env?.VITE_ATERM_RENDERER === 'true') {
    return true
  }
  // Opt-out semantics: an unset value defaults ON so existing users get it too
  // with no settings migration. Only an explicit `false` turns it off.
  return useAppStore.getState().settings?.experimentalAtermRenderer !== false
}
