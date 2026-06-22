// Vitest setup: guarantee a working Web Storage API in DOM test environments.
//
// Why: Node 26 ships a gated global `localStorage` (needs `--localstorage-file`)
// that shadows happy-dom's prototype-getter `localStorage` once Vitest copies the
// window globals onto `globalThis`, so DOM tests see `window.localStorage`
// undefined. happy-dom itself provides Storage (so on the project's target Node 24
// nothing here fires); this only supplies an in-memory Storage when the running
// Node makes the DOM env's storage non-functional. No-op under the default `node`
// environment, where there is no `document`/`window`.

function createMemoryStorage(): Storage {
  const store = new Map<string, string>()
  return {
    get length(): number {
      return store.size
    },
    clear(): void {
      store.clear()
    },
    getItem(key: string): string | null {
      return store.has(String(key)) ? (store.get(String(key)) as string) : null
    },
    key(index: number): string | null {
      return Array.from(store.keys())[index] ?? null
    },
    removeItem(key: string): void {
      store.delete(String(key))
    },
    setItem(key: string, value: string): void {
      store.set(String(key), String(value))
    }
  } as Storage
}

function ensureStorage(key: 'localStorage' | 'sessionStorage'): void {
  const g = globalThis as unknown as Record<string, unknown> & { document?: unknown; window?: unknown }
  // Only act in a DOM environment (happy-dom / jsdom); the node env has neither.
  if (typeof g.document === 'undefined' && typeof g.window === 'undefined') {
    return
  }
  let working = false
  try {
    const existing = g[key] as Storage | undefined
    working = !!existing && typeof existing.setItem === 'function'
  } catch {
    working = false
  }
  if (working) {
    return
  }
  const storage = createMemoryStorage()
  Object.defineProperty(g, key, { configurable: true, writable: true, value: storage })
  const win = g.window as Record<string, unknown> | undefined
  if (win && win !== (g as unknown)) {
    Object.defineProperty(win, key, { configurable: true, writable: true, value: storage })
  }
}

ensureStorage('localStorage')
ensureStorage('sessionStorage')
