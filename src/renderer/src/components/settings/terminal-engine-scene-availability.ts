import { loadAterm } from '@/lib/pane-manager/aterm/load-aterm'
import * as atermWasm from '@/lib/pane-manager/aterm/aterm_wasm.js'

// The Terminal Engine settings pane only shows a Scenes row when the engine
// actually ships scenes — the registry is deliberately empty today, and a row
// describing unavailable art would overclaim (STYLEGUIDE: UI copy must not
// overclaim).

/** `scene_names_csv()` → names. Empty/whitespace CSV → no scenes. */
export function parseSceneNamesCsv(csv: string): string[] {
  return csv
    .split(',')
    .map((name) => name.trim())
    .filter((name) => name.length > 0)
}

// Feature-detected: the currently vendored engine build ships no scene
// registry export at all, which is exactly the "no scenes" answer.
const sceneNamesCsv = (atermWasm as { scene_names_csv?: () => string }).scene_names_csv

let namesPromise: Promise<readonly string[]> | null = null

/** Scene names the vendored engine build ships. Shared across callers (the wasm
 *  module is a singleton and the registry is compile-time constant); resolves []
 *  when the engine can't load, so callers simply keep the row hidden. */
export function listAtermSceneNames(): Promise<readonly string[]> {
  namesPromise ??= (async () => {
    try {
      if (!sceneNamesCsv) {
        return []
      }
      // Free wasm functions need the module initialized; loadAterm shares the
      // one init every pane already performs.
      await loadAterm()
      return parseSceneNamesCsv(sceneNamesCsv())
    } catch {
      return []
    }
  })()
  return namesPromise
}
