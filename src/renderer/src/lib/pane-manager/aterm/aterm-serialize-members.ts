import type { AtermTerminal } from './aterm_wasm.js'
import type { AtermDrawStrategy } from './aterm-draw-strategy'

/** The controller's serialize surface: SYNC (the in-process engine, or the worker's
 *  cached blob) + AWAITABLE (a worker round-trip for fresh off-screen history; resolved
 *  synchronously for the in-process engine). Extracted to keep the wiring under the line
 *  cap. The worker strategy supplies serializeAsync/serializeScrollbackAsync; in-process
 *  strategies leave them unset, so the awaitable form wraps the sync engine result. */
export function buildAtermSerializeMembers(
  term: Pick<AtermTerminal, 'serialize' | 'serialize_scrollback'>,
  strategy: Pick<AtermDrawStrategy, 'serializeAsync' | 'serializeScrollbackAsync'>
): {
  serialize: (scrollbackRows?: number) => string
  serializeScrollback: (maxRows?: number) => string
  serializeAsync: (scrollbackRows?: number) => Promise<string>
  serializeScrollbackAsync: (maxRows?: number) => Promise<string>
} {
  return {
    serialize: (scrollbackRows) => term.serialize(scrollbackRows),
    serializeScrollback: (maxRows) => term.serialize_scrollback(maxRows),
    serializeAsync: (scrollbackRows) =>
      strategy.serializeAsync
        ? strategy.serializeAsync(scrollbackRows)
        : Promise.resolve(term.serialize(scrollbackRows)),
    serializeScrollbackAsync: (maxRows) =>
      strategy.serializeScrollbackAsync
        ? strategy.serializeScrollbackAsync(maxRows)
        : Promise.resolve(term.serialize_scrollback(maxRows))
  }
}
