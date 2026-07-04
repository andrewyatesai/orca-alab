import { describe, expect, it, vi } from 'vitest'
import { subscribeToTerminalUserInput } from './terminal-user-input-signal'

// A pane event target that mirrors the DOM contract the signal relies on:
// capture listeners registered on the stable pane wrapper/container see the
// gestures dispatched on the (async-attached) helper textarea and canvas.
function createPaneEventTarget() {
  const listeners = new Map<string, Set<(event: unknown) => void>>()
  const addOptions: { type: string; options: unknown }[] = []
  return {
    addOptions,
    addEventListener: vi.fn(
      (type: string, handler: (event: unknown) => void, options?: unknown) => {
        const set = listeners.get(type) ?? new Set<(event: unknown) => void>()
        set.add(handler)
        listeners.set(type, set)
        addOptions.push({ type, options })
      }
    ),
    removeEventListener: vi.fn((type: string, handler: (event: unknown) => void) => {
      listeners.get(type)?.delete(handler)
    }),
    dispatch(type: string, event: Record<string, unknown> = {}): void {
      listeners.get(type)?.forEach((handler) => handler({ type, ...event }))
    },
    subscribedTypes(): string[] {
      return [...listeners.entries()]
        .filter(([, set]) => set.size > 0)
        .map(([type]) => type)
        .sort()
    }
  }
}

// Live terminal mode reads (the structural facade subset the classifier uses).
function createTerminalReads() {
  const state = { selection: false, mouseTracking: 'none', bufferType: 'normal' }
  return {
    state,
    terminal: {
      hasSelection: () => state.selection,
      modes: {
        get mouseTrackingMode() {
          return state.mouseTracking
        }
      },
      buffer: {
        active: {
          get type() {
            return state.bufferType
          }
        }
      }
    }
  }
}

describe('subscribeToTerminalUserInput', () => {
  it('fires for real user input gestures and not for the emulator reply sources', () => {
    const { terminal, state } = createTerminalReads()
    const target = createPaneEventTarget()
    const listener = vi.fn()
    const subscription = subscribeToTerminalUserInput(terminal, target, listener)
    expect(subscription).not.toBeNull()

    // Keyboard/IME/paste paths are real user input.
    target.dispatch('keydown', { key: 'a' })
    expect(listener).toHaveBeenCalledTimes(1)
    // Held keys stream real bytes, so autorepeat still counts as input.
    target.dispatch('keydown', { key: 'a', repeat: true })
    expect(listener).toHaveBeenCalledTimes(2)
    target.dispatch('paste')
    expect(listener).toHaveBeenCalledTimes(3)

    // Modifier-only presses produce no terminal bytes and must not fire.
    for (const key of ['Alt', 'AltGraph', 'Control', 'Meta', 'Shift']) {
      target.dispatch('keydown', { key })
    }
    expect(listener).toHaveBeenCalledTimes(3)

    // A copy chord over an active selection copies instead of sending bytes...
    state.selection = true
    target.dispatch('keydown', { key: 'c', ctrlKey: true })
    expect(listener).toHaveBeenCalledTimes(3)
    // ...while Ctrl+C without a selection is a real interrupt keystroke.
    state.selection = false
    target.dispatch('keydown', { key: 'c', ctrlKey: true })
    expect(listener).toHaveBeenCalledTimes(4)

    // The fork's auto-reply sources stay structurally untapped: focus reports
    // originate from textarea focus/blur (aterm-focus-input) and DA/DSR/CPR
    // replies from the engine drain (no DOM event at all) — the signal never
    // subscribes those channels, so synthetic replies can't read as activity.
    expect(target.subscribedTypes()).toEqual(['keydown', 'mousedown', 'paste', 'wheel'])
    target.dispatch('focus')
    target.dispatch('blur')
    expect(listener).toHaveBeenCalledTimes(4)

    // Capture-phase registration is what lets the stable pane target observe
    // the async-attached textarea/canvas — every listener must opt in.
    for (const { options } of target.addOptions) {
      expect(options).toMatchObject({ capture: true })
    }

    subscription?.dispose()
    target.dispatch('keydown', { key: 'b' })
    expect(listener).toHaveBeenCalledTimes(4)
    expect(target.removeEventListener).toHaveBeenCalledTimes(4)
  })

  it('gates pointer gestures on the modes that actually produce PTY bytes', () => {
    const { terminal, state } = createTerminalReads()
    const target = createPaneEventTarget()
    const listener = vi.fn()
    expect(subscribeToTerminalUserInput(terminal, target, listener)).not.toBeNull()

    // No mouse tracking, normal screen: clicks select and wheel scrolls the
    // viewport — nothing reaches the PTY, so nothing may record as input.
    target.dispatch('mousedown')
    target.dispatch('wheel')
    expect(listener).not.toHaveBeenCalled()

    // A TUI tracking the mouse turns both into report bytes.
    state.mouseTracking = 'vt200'
    target.dispatch('mousedown')
    expect(listener).toHaveBeenCalledTimes(1)
    target.dispatch('wheel')
    expect(listener).toHaveBeenCalledTimes(2)

    // Alternate screen without tracking: wheel synthesizes arrow-key presses
    // (aterm-scroll-input's alternate-scroll), but clicks still send nothing.
    state.mouseTracking = 'none'
    state.bufferType = 'alternate'
    target.dispatch('wheel')
    expect(listener).toHaveBeenCalledTimes(3)
    target.dispatch('mousedown')
    expect(listener).toHaveBeenCalledTimes(3)
  })

  it('returns null when the pane target cannot host the signal', () => {
    const { terminal } = createTerminalReads()
    const listener = vi.fn()
    expect(subscribeToTerminalUserInput(terminal, null, listener)).toBeNull()
    expect(subscribeToTerminalUserInput(terminal, undefined, listener)).toBeNull()
    expect(subscribeToTerminalUserInput(terminal, {} as never, listener)).toBeNull()
    // A target that can subscribe but never unsubscribe must read as
    // unavailable, so callers keep their accepted-send fallback instead of
    // trusting a subscription they could never dispose.
    expect(
      subscribeToTerminalUserInput(terminal, { addEventListener: vi.fn() } as never, listener)
    ).toBeNull()

    // A target that throws mid-attach degrades to unavailable AND leaves no
    // half-attached listener behind (that would double-record activity once
    // the caller re-enables its fallback).
    const removeEventListener = vi.fn()
    const addEventListener = vi
      .fn()
      .mockImplementationOnce(() => undefined)
      .mockImplementation(() => {
        throw new Error('unavailable')
      })
    expect(
      subscribeToTerminalUserInput(
        terminal,
        { addEventListener, removeEventListener } as never,
        listener
      )
    ).toBeNull()
    expect(removeEventListener).toHaveBeenCalledTimes(1)
    expect(removeEventListener).toHaveBeenCalledWith(
      'keydown',
      expect.any(Function),
      expect.objectContaining({ capture: true })
    )
    expect(listener).not.toHaveBeenCalled()
  })
})
