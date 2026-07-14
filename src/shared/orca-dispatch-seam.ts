// Surface-agnostic injection seam for the Rust dispatch aggregate.
//
// A src/shared module runs in every surface (Electron main, cli, renderer,
// relay) and cannot import a surface-specific binding — napi lives only in
// main/cli, wasm only in renderer/relay — so a shared module can't reach the
// Rust core directly. Each entry point installs its own dispatch fn here ONCE at
// bootstrap (main/cli → napi, renderer → wasm at ready, relay → wasm via
// initSync); shared modules then dispatch through this indirection.
//
// This file imports NO napi/wasm — it is pure indirection, safe to load in any
// surface.

export type OrcaDispatchFn = (module: string, fn: string, inputJson: string) => string

let bound: OrcaDispatchFn | null = null

/** Install the surface's dispatch binding (last wins; pass null only to reset
 *  in tests). Called once per surface at bootstrap. */
export function setOrcaDispatchBinding(fn: OrcaDispatchFn | null): void {
  bound = fn
}

/** True once a surface has installed its binding. The only not-ready window is
 *  the renderer before wasm init (main/cli/relay bind synchronously). */
export function isOrcaDispatchReady(): boolean {
  return bound !== null
}

/** Dispatch through the bound Rust core, or `null` when no binding is installed
 *  yet (pre-bootstrap / renderer wasm not ready). Callers MUST supply a safe
 *  degraded fallback at the call site — never keep the old TS impl as the
 *  fallback, which would defeat the dedup. */
export function tryOrcaDispatch(module: string, fn: string, input: unknown): unknown | null {
  if (!bound) {
    return null
  }
  return JSON.parse(bound(module, fn, JSON.stringify(input ?? null)))
}

/** Dispatch through the bound Rust core, THROWING if no binding is installed.
 *  Use only from modules that run exclusively on always-ready surfaces
 *  (main/cli/relay bind synchronously at bootstrap) — an unbound seam there is a
 *  bootstrap-order bug we want to surface loudly, not silently degrade. */
export function requireOrcaDispatch(module: string, fn: string, input: unknown): unknown {
  if (!bound) {
    throw new Error(
      `orcaDispatch seam not bound for ${module}.${fn} — the surface bootstrap must install its binding first`
    )
  }
  return JSON.parse(bound(module, fn, JSON.stringify(input ?? null)))
}
