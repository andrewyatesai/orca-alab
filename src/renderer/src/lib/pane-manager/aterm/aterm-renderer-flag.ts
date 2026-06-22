import { useAppStore } from '@/store'

// Module-const fallback so the renderer can be force-enabled without settings
// or env wiring (flip locally during bring-up). Keep false on commit.
const ENABLE_ATERM_RENDERER = false

/** True when terminal panes should paint via the in-page aterm canvas renderer
 *  instead of xterm.js. Off by default; enabled by the experimental setting, a
 *  Vite build env override, or an e2e/dev window flag. */
export function isAtermRendererEnabled(): boolean {
  if (ENABLE_ATERM_RENDERER) {
    return true
  }
  if (import.meta.env?.VITE_ATERM_RENDERER === 'true') {
    return true
  }
  if (typeof window !== 'undefined' && window.__atermRendererEnabled === true) {
    return true
  }
  return useAppStore.getState().settings?.experimentalAtermRenderer === true
}
