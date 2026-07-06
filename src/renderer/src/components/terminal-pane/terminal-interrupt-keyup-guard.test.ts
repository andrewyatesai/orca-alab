/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import { createTerminalInterruptKeyupGuard } from './terminal-interrupt-keyup-guard'
import type { XtermBypassEvent } from './xterm-bypass-policy'

function keyup(overrides: Partial<XtermBypassEvent> = {}): XtermBypassEvent {
  return {
    type: 'keyup',
    key: 'c',
    code: 'KeyC',
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides
  }
}

describe('createTerminalInterruptKeyupGuard', () => {
  it('suppresses the paired keyup exactly once after an armed claim', () => {
    const guard = createTerminalInterruptKeyupGuard(undefined)
    expect(guard.claimKeyEvent(keyup())).toBe(false) // not armed yet
    guard.arm()
    expect(guard.claimKeyEvent(keyup())).toBe(true)
    // One claim per arm: a second 'c' keyup is normal input again.
    expect(guard.claimKeyEvent(keyup())).toBe(false)
    guard.dispose()
  })

  it('only matches the interrupt keyup shape while armed', () => {
    const guard = createTerminalInterruptKeyupGuard(undefined)
    guard.arm()
    expect(guard.claimKeyEvent(keyup({ key: 'x', code: 'KeyX' }))).toBe(false)
    expect(guard.claimKeyEvent(keyup({ type: 'keydown' }))).toBe(false)
    // Still armed after non-matching events; the real paired keyup is claimed.
    expect(guard.claimKeyEvent(keyup())).toBe(true)
    guard.dispose()
  })

  it('disarms explicitly when the claim path sees the keyup itself', () => {
    const guard = createTerminalInterruptKeyupGuard(undefined)
    guard.arm()
    guard.disarm()
    expect(guard.claimKeyEvent(keyup())).toBe(false)
    guard.dispose()
  })

  it('clears the armed flag on terminal blur so it cannot swallow a later unrelated keyup', () => {
    // The regression this exists for: Ctrl+C press claimed → focus stolen
    // before the keyup → flag leaked armed across the blur → after refocus, an
    // ordinary 'c' keyup was silently swallowed.
    const element = document.createElement('div')
    const child = document.createElement('textarea')
    element.appendChild(child)
    document.body.appendChild(element)
    const guard = createTerminalInterruptKeyupGuard(element)
    guard.arm()
    // Capture-phase listener: a blur dispatched on the inner textarea (the
    // real focus sink) must clear the flag too.
    child.dispatchEvent(new FocusEvent('blur'))
    expect(guard.claimKeyEvent(keyup())).toBe(false)
    guard.dispose()
  })

  it('stops listening after dispose', () => {
    const element = document.createElement('div')
    document.body.appendChild(element)
    const guard = createTerminalInterruptKeyupGuard(element)
    guard.dispose()
    guard.arm()
    element.dispatchEvent(new FocusEvent('blur'))
    // Disposed guard no longer reacts to blur; the armed flag survives, which
    // is fine because dispose also cleared it and nothing consults it after
    // teardown — the assertion documents that no listener remains.
    expect(guard.claimKeyEvent(keyup())).toBe(true)
  })
})
