// Network-proxy TYPES only. The behavior (normalize/build/redact + env getters)
// moved to the parity-proven Rust orca-net core: main drives it via napi
// (src/main/rust-network-proxy.ts, which also owns the main-only env getters),
// the renderer via wasm (src/renderer/src/lib/git-wasm/network-proxy.ts). Kept
// import-safe from every surface (no napi/wasm import here).
export type NetworkProxySettings = {
  httpProxyUrl?: string | null
  httpProxyBypassRules?: string | null
}

export type ProxyUrlValidationResult =
  | { ok: true; value: string; message?: undefined }
  | { ok: false; value: ''; message: string }
