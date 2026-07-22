import { afterEach, describe, expect, it, vi } from 'vitest'

// PC-8367 apply-order proofs for the IN-PROCESS lazy injector: the user stack
// leads the fallback chain (set RESETS to user[0], the rest APPEND), the CJK
// face demotes to an append only when a user stack exists, then the script
// chain; an empty stack keeps set(cjk) exactly as before.

// Both wasm modules are mocked so the test never loads engine glue; handles are
// tagged by the marker byte of the registered blob for order assertions.
const registerCalls: number[] = []
vi.mock('./aterm_wasm.js', () => ({
  register_font: (bytes: Uint8Array) => {
    registerCalls.push(bytes[0])
    return bytes[0]
  }
}))
vi.mock('./aterm_gpu_web.js', () => ({
  register_font: (bytes: Uint8Array) => {
    registerCalls.push(bytes[0])
    return bytes[0]
  }
}))

type FallbackFontsPayload = {
  user: { family: string; bytes: Uint8Array }[]
  cjk?: { bytes: Uint8Array; region: 'ja' | 'ko' | 'zh-Hant' | 'zh-Hans' }
  emoji?: Uint8Array
  symbol?: Uint8Array
  chain: { bytes: Uint8Array; script: 'arabic' | 'hebrew' | 'devanagari' | 'thai' | 'unicode' }[]
}

// Marker bytes double as handles (the register mock returns bytes[0]).
const USER_A = 0x0a
const USER_B = 0x0b
const CJK = 0x1c
const CHAIN_0 = 0x2c
const SYMBOL = 0x3c

function stubFontsApi(payload: FallbackFontsPayload): void {
  vi.stubGlobal('window', {
    api: { fonts: { getTerminalFallbackFonts: vi.fn().mockResolvedValue(payload) } }
  })
}

// A fake terminal that records the injection order as `<op>:<handle>` strings
// and reports one MISSING_TEXT (bit 1) miss.
function makeRecordingTerm(): { ops: string[]; term: Record<string, unknown> } {
  const ops: string[] = []
  let reported = false
  const term = {
    set_fallback_font_registered: (h: number) => ops.push(`set:${h}`),
    add_fallback_font_registered: (h: number) => ops.push(`add:${h}`),
    set_emoji_font_registered: (h: number) => ops.push(`emoji:${h}`),
    set_symbol_font_registered: (h: number) => ops.push(`symbol:${h}`),
    take_missing_font_classes: () => {
      if (reported) {
        return 0
      }
      reported = true
      return 1
    }
  }
  return { ops, term }
}

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

async function injectTextClass(payload: FallbackFontsPayload): Promise<string[]> {
  // Fresh module per call: the per-module handle memos are process-scoped.
  vi.resetModules()
  stubFontsApi(payload)
  const { createLazyFallbackFontInjector } = await import('./inject-terminal-fallback-fonts')
  const { ops, term } = makeRecordingTerm()
  const requestRedraw = vi.fn()
  const injector = createLazyFallbackFontInjector({
    term: term as never,
    engine: 'cpu',
    requestRedraw
  })
  injector.poll()
  await settle()
  expect(requestRedraw).toHaveBeenCalled()
  return ops
}

describe('inject-terminal-fallback-fonts apply order (user stacks)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    registerCalls.length = 0
  })

  it('applies set(user0), add(user1..), add(cjk), add(chain), symbol', async () => {
    const ops = await injectTextClass({
      user: [
        { family: 'A', bytes: new Uint8Array([USER_A]) },
        { family: 'B', bytes: new Uint8Array([USER_B]) }
      ],
      cjk: { bytes: new Uint8Array([CJK]), region: 'zh-Hans' },
      chain: [{ bytes: new Uint8Array([CHAIN_0]), script: 'arabic' }],
      symbol: new Uint8Array([SYMBOL])
    })
    expect(ops).toEqual([
      `set:${USER_A}`,
      `add:${USER_B}`,
      `add:${CJK}`,
      `add:${CHAIN_0}`,
      `symbol:${SYMBOL}`
    ])
  })

  it('an empty stack keeps set(cjk) exactly as before', async () => {
    const ops = await injectTextClass({
      user: [],
      cjk: { bytes: new Uint8Array([CJK]), region: 'zh-Hans' },
      chain: [{ bytes: new Uint8Array([CHAIN_0]), script: 'arabic' }],
      symbol: new Uint8Array([SYMBOL])
    })
    expect(ops).toEqual([`set:${CJK}`, `add:${CHAIN_0}`, `symbol:${SYMBOL}`])
  })

  it('registers user faces before the CJK/chain faces (module registry order)', async () => {
    await injectTextClass({
      user: [{ family: 'A', bytes: new Uint8Array([USER_A]) }],
      cjk: { bytes: new Uint8Array([CJK]), region: 'zh-Hans' },
      chain: [{ bytes: new Uint8Array([CHAIN_0]), script: 'arabic' }],
      symbol: new Uint8Array([SYMBOL])
    })
    expect(registerCalls).toEqual([USER_A, CJK, CHAIN_0, SYMBOL])
  })
})
