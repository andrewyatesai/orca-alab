import { describe, expect, it, vi } from 'vitest'
import type { OrcaDeepLinkUiEvent } from '../../shared/orca-deep-link'
import { createMainDeepLinkDispatcher } from './deep-links'

function makeDispatcher(overrides?: {
  focusTerminal?: (handle: string) => Promise<unknown>
  runtimeMissing?: boolean
}): {
  dispatcher: ReturnType<typeof createMainDeepLinkDispatcher>
  sent: OrcaDeepLinkUiEvent[]
  focusMainWindow: ReturnType<typeof vi.fn>
  focusTerminal: (handle: string) => Promise<unknown>
  logged: string[]
} {
  const sent: OrcaDeepLinkUiEvent[] = []
  const focusMainWindow = vi.fn()
  const focusTerminal = overrides?.focusTerminal ?? vi.fn(() => Promise.resolve({}))
  const logged: string[] = []
  const dispatcher = createMainDeepLinkDispatcher({
    getRuntime: () => (overrides?.runtimeMissing ? null : { focusTerminal }),
    sendDeepLinkUiEvent: (event) => {
      sent.push(event)
      return true
    },
    focusMainWindow,
    log: (message) => logged.push(message)
  })
  return { dispatcher, sent, focusMainWindow, focusTerminal, logged }
}

async function flushMicrotasks(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

describe('createMainDeepLinkDispatcher', () => {
  it('focus routes through runtime.focusTerminal after surfacing the window', () => {
    const { dispatcher, focusMainWindow, focusTerminal, sent } = makeDispatcher()

    dispatcher.dispatch({ kind: 'focus', handle: 'term_abc' }, { source: 'os' })

    expect(focusMainWindow).toHaveBeenCalledTimes(1)
    expect(focusTerminal).toHaveBeenCalledWith('term_abc')
    expect(sent).toHaveLength(0)
  })

  it('stale focus handles surface the terminal-gone notice', async () => {
    const { dispatcher, sent } = makeDispatcher({
      focusTerminal: () => Promise.reject(new Error('terminal_handle_stale'))
    })

    dispatcher.dispatch({ kind: 'focus', handle: 'term_stale' }, { source: 'os' })
    await flushMicrotasks()

    expect(sent).toEqual([{ type: 'notice', notice: 'terminal-gone' }])
  })

  it('focus before the runtime exists degrades to the terminal-gone notice', () => {
    const { dispatcher, sent } = makeDispatcher({ runtimeMissing: true })

    dispatcher.dispatch({ kind: 'focus', handle: 'term_abc' }, { source: 'os' })

    expect(sent).toEqual([{ type: 'notice', notice: 'terminal-gone' }])
  })

  it('worktree/pair/run forward to the renderer with the transport-stamped origin', () => {
    const { dispatcher, sent } = makeDispatcher()

    dispatcher.dispatch({ kind: 'worktree', worktreeId: 'r::p' }, { source: 'os' })
    dispatcher.dispatch({ kind: 'pair', code: 'abc' }, { source: 'os' })

    expect(sent).toEqual([
      { type: 'link', link: { kind: 'worktree', worktreeId: 'r::p' }, origin: { source: 'os' } },
      { type: 'link', link: { kind: 'pair', code: 'abc' }, origin: { source: 'os' } }
    ])
  })

  it('logs pair dispatches redacted', () => {
    const { dispatcher, logged } = makeDispatcher()

    dispatcher.dispatch({ kind: 'pair', code: 'super-secret' }, { source: 'os' })

    expect(logged.join('\n')).not.toContain('super-secret')
  })

  it('notifyUnrecognized sends the unrecognized notice', () => {
    const { dispatcher, sent } = makeDispatcher()

    dispatcher.notifyUnrecognized()

    expect(sent).toEqual([{ type: 'notice', notice: 'unrecognized' }])
  })
})
