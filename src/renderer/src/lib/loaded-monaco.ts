import type * as MonacoModule from 'monaco-editor'

export type LoadedMonaco = typeof MonacoModule

let loadedMonaco: LoadedMonaco | null = null

// Why: always-mounted surfaces (App-level cleanup gate) must reach Monaco
// models without statically importing monaco-editor into the eager startup
// chunk (renderer-chunk-budget). monaco-setup registers the instance on load;
// a null read means Monaco never loaded, so no models can exist.
export function registerLoadedMonaco(instance: LoadedMonaco): void {
  loadedMonaco = instance
}

export function getLoadedMonaco(): LoadedMonaco | null {
  return loadedMonaco
}
