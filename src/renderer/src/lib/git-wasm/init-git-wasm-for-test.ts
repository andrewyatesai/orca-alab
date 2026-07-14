// Side-effect import for tests that render or call wasm-backed helpers (tui
// agent-startup plan builders, terminal quick-commands, git line stats):
// synchronously initialises the orca-git wasm so `isGitWasmReady()` is true and
// those helpers return real results instead of their pre-ready null fallback.
//
// Node has no main-thread sync-compile restriction, so `initSync` is safe here
// (the app renderer must stay on the async `startGitWasm()`). Not reachable from
// production code, so it is never bundled. Import it once at the top of a test:
//   import '@/lib/git-wasm/init-git-wasm-for-test'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { initGitWasmForTestFromBytes, isGitWasmReady } from './git-line-stats'

// Under the DOM test environments (happy-dom/jsdom) `import.meta.url` resolves
// against the mocked http origin, not a file URL — so fall back to the wasm's
// checked-in path (vitest cwd is the repo root) to stay env-agnostic.
function resolveWasmPath(): string {
  const url = new URL('./orca_git_wasm_bg.wasm', import.meta.url)
  if (url.protocol === 'file:') {return fileURLToPath(url)}
  return resolve(process.cwd(), 'src/renderer/src/lib/git-wasm/orca_git_wasm_bg.wasm')
}

if (!isGitWasmReady()) {
  initGitWasmForTestFromBytes(readFileSync(resolveWasmPath()))
}
