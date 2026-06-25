import type { IDisposable, IParser } from '@xterm/xterm'

/** xterm OSC handlers receive the raw payload string and return whether they
 *  consumed the sequence. */
type OscHandler = (data: string) => boolean | Promise<boolean>

/** The xterm-compatible `parser` object, backed by the aterm engine's drained
 *  OSC app-events (take_osc_events). CSI/ESC/DCS replies are owned by the engine
 *  (take_response), so those registrations are real no-ops. Signatures mirror
 *  xterm's IParser so the (unchanged) callers compile. */
export type AtermFacadeParser = {
  /** Register an OSC handler for `code`. The facade drains take_osc_events()
   *  after each process() and dispatches (code, payload) here, so OSC 52/7/133
   *  keep working without xterm's parser. */
  registerOscHandler(code: number, handler: OscHandler): IDisposable
  /** No-op: aterm's take_response() is the authoritative query responder, so a
   *  CSI handler can't double-answer. Returns a real disposable. */
  registerCsiHandler: IParser['registerCsiHandler']
  /** No-op: unused by orca; aterm owns ESC parsing. Returns a real disposable. */
  registerEscHandler: IParser['registerEscHandler']
  /** No-op: unused by orca; aterm owns DCS parsing. Returns a real disposable. */
  registerDcsHandler: IParser['registerDcsHandler']
}

/** Re-encode an engine-decoded OSC payload back into the xterm wire format the
 *  unchanged orca OSC handlers parse. The engine pre-decodes (OSC 52 → plaintext,
 *  OSC 7 → bare path); orca's parsers expect the raw `Pc;<base64>` / `file://…`
 *  forms. Behavior is preserved (the handler decodes back to the same value);
 *  only fields orca's handlers ignore (OSC 52 selection kind, OSC 7 host) are
 *  defaulted, since the engine doesn't surface them. */
function toXtermOscWireFormat(code: number, payload: string): string {
  if (code === 52) {
    // Re-base64 the decoded UTF-8 text and tag it clipboard ("c"): the handler
    // strips "c;", base64-decodes, and writes exactly `payload` to the clipboard.
    const utf8 = new TextEncoder().encode(payload)
    let binary = ''
    for (const byte of utf8) {
      binary += String.fromCharCode(byte)
    }
    return `c;${btoa(binary)}`
  }
  if (code === 7) {
    // Wrap the bare path back into a host-less file:// URI; parseOsc7's regex
    // accepts an empty host and returns the (percent-decoded) path unchanged.
    return `file://${encodeURI(payload)}`
  }
  return payload
}

export function createAtermFacadeParser(): {
  parser: AtermFacadeParser
  /** Dispatch one drained OSC event (code + engine-decoded payload) to the
   *  registered handlers, re-encoding to the xterm wire format they expect.
   *  Called by the facade after each process(). */
  dispatchOscEvent(code: number, payload: string): void
} {
  // Multiple handlers per code are allowed (xterm runs them newest-first); orca
  // registers at most one per code, but keep a list for fidelity.
  const handlersByCode = new Map<number, Set<OscHandler>>()

  const parser: AtermFacadeParser = {
    registerOscHandler(code, handler) {
      let set = handlersByCode.get(code)
      if (!set) {
        set = new Set()
        handlersByCode.set(code, set)
      }
      set.add(handler)
      return {
        dispose: () => {
          set?.delete(handler)
          if (set && set.size === 0) {
            handlersByCode.delete(code)
          }
        }
      }
    },
    // aterm owns CSI/ESC/DCS replies + parsing; these existed only to stop xterm
    // double-answering. Real no-op disposables (documented authority shift).
    registerCsiHandler: () => ({ dispose: () => undefined }),
    registerEscHandler: () => ({ dispose: () => undefined }),
    registerDcsHandler: () => ({ dispose: () => undefined })
  }

  const dispatchOscEvent = (code: number, payload: string): void => {
    const set = handlersByCode.get(code)
    if (!set) {
      return
    }
    const wire = toXtermOscWireFormat(code, payload)
    for (const handler of set) {
      // Handlers may return a promise (OSC 52 clipboard); we don't await — the
      // engine already acted, this just notifies orca's UI side.
      void handler(wire)
    }
  }

  return { parser, dispatchOscEvent }
}
