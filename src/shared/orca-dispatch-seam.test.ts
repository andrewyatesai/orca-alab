import { afterEach, describe, expect, it } from 'vitest'
import {
  isOrcaDispatchReady,
  requireOrcaDispatch,
  setOrcaDispatchBinding,
  tryOrcaDispatch
} from './orca-dispatch-seam'

// Exercise the seam's indirection + JSON round-trip in isolation with a stub
// binding (real Rust output through the wasm/napi core is covered by the parity
// suite and the cut-over modules' own tests). The global test setup installs the
// real binding; reset after each test so nothing leaks.
afterEach(() => setOrcaDispatchBinding(null))

describe('orca-dispatch-seam', () => {
  it('reports readiness only once a binding is installed', () => {
    setOrcaDispatchBinding(null)
    expect(isOrcaDispatchReady()).toBe(false)
    setOrcaDispatchBinding(() => 'null')
    expect(isOrcaDispatchReady()).toBe(true)
  })

  it('tryOrcaDispatch JSON-round-trips input through the binding', () => {
    setOrcaDispatchBinding((_module, _fn, inputJson) => `{"echo":${inputJson}}`)
    expect(tryOrcaDispatch('mod', 'fn', { a: 1 })).toEqual({ echo: { a: 1 } })
  })

  it('serializes absent/undefined input as JSON null', () => {
    let seen = ''
    setOrcaDispatchBinding((_module, _fn, inputJson) => {
      seen = inputJson
      return 'null'
    })
    tryOrcaDispatch('mod', 'fn', undefined)
    expect(seen).toBe('null')
  })

  it('tryOrcaDispatch returns null and requireOrcaDispatch throws when unbound', () => {
    setOrcaDispatchBinding(null)
    expect(tryOrcaDispatch('mod', 'fn', null)).toBeNull()
    expect(() => requireOrcaDispatch('mod', 'fn', null)).toThrow(/not bound/)
  })

  it('requireOrcaDispatch routes through the binding when installed', () => {
    setOrcaDispatchBinding((module, fn) => `"${module}.${fn}"`)
    expect(requireOrcaDispatch('mod', 'fn', null)).toBe('mod.fn')
  })
})
