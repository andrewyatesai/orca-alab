// CM-A3 renderer plumbing over the engine's `last_command_output()` JSON binding
// (the newest row-sealed OSC-133 block). Feature-detected everywhere: the GPU
// wasm module doesn't export the binding yet, and older pins may lack it — the
// member then resolves null and the menu item stays hidden.

import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'

/** The last completed command block's output, or the honest eviction marker
 *  (rows scrolled past the scrollback cap — the text is unrecoverable). */
export type AtermLastCommandOutput =
  | { status: 'ok'; text: string; exitCode: number | null }
  | { status: 'evicted' }

/** Parse the binding's JSON drain shape:
 *  `{"status":"ok","text":"…","exitCode":0|null}` / `{"status":"evicted"}` /
 *  undefined (no completed block, incl. post-reattach). Malformed → null. */
export function parseAtermLastCommandOutput(
  json: string | null | undefined
): AtermLastCommandOutput | null {
  if (!json) {
    return null
  }
  try {
    const parsed = JSON.parse(json) as { status?: unknown; text?: unknown; exitCode?: unknown }
    if (parsed.status === 'evicted') {
      return { status: 'evicted' }
    }
    if (parsed.status === 'ok' && typeof parsed.text === 'string') {
      return {
        status: 'ok',
        text: parsed.text,
        exitCode: typeof parsed.exitCode === 'number' ? parsed.exitCode : null
      }
    }
    return null
  } catch {
    return null
  }
}

/** Build the controller's `lastCommandOutputAsync` member for a pane engine:
 *  worker-backed facades answer through the id-correlated query channel (the
 *  sync snapshot can't read blocks); in-process engines read the binding
 *  directly when the loaded wasm exports it. */
export function buildAtermLastCommandOutputMember(
  term: AtermTerminal
): () => Promise<AtermLastCommandOutput | null> {
  const workerTerm = term as AtermTerminal & Partial<AtermWorkerAsyncFacade>
  return async () => {
    if (workerTerm.lastCommandOutputAsync) {
      return parseAtermLastCommandOutput(await workerTerm.lastCommandOutputAsync())
    }
    // Feature-detect the engine binding (older pins / GPU module lack it).
    if (typeof term.last_command_output === 'function') {
      return parseAtermLastCommandOutput(term.last_command_output())
    }
    return null
  }
}
