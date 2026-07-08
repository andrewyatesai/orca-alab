// Side-effect import for node-env tests that render or call wasm-backed helpers
// (tui agent-startup plan builders, terminal quick-commands, git line stats):
// synchronously initialises the orca-git wasm so `isGitWasmReady()` is true and
// those helpers return real results instead of their pre-ready null fallback.
//
// Node has no main-thread sync-compile restriction, so `initSync` is safe here
// (the app renderer must stay on the async `startGitWasm()`). Not reachable from
// production code, so it is never bundled. Import it once at the top of a test:
//   import '@/lib/git-wasm/init-git-wasm-for-test'
import { readFileSync } from 'node:fs'
import { initGitWasmForTestFromBytes, isGitWasmReady } from './git-line-stats'

if (!isGitWasmReady()) {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
}
