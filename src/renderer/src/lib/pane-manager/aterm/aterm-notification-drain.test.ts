/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import { parseAtermNotifications } from './aterm-notification-drain'
import { createWorkerTerminal } from './aterm-worker-terminal'
import { createWorkerBackedTerm } from './aterm-worker-term'
import { createAtermTerminalFacade } from './aterm-terminal-facade'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermPaneController } from './aterm-pane-controller-types'
import type { AtermWorkerPaneCommand, AtermWorkerState } from './aterm-render-worker-protocol'

// The OSC 9/99/777 notification channel host-side: the JSON drain decode, the
// worker-side per-chunk drain + authorize passthrough, the main-side worker-term
// buffering, and the facade's fail-closed drain → onTerminalAppNotification.

describe('parseAtermNotifications (engine JSON → typed events)', () => {
  it('decodes structured OSC 99/777 fields incl. urgency', () => {
    const json = JSON.stringify([
      { id: 'n1', title: 'Build', body: 'done', urgency: 'critical' },
      { id: null, title: 'Ping', body: null, urgency: 'low' }
    ])
    expect(parseAtermNotifications(json)).toEqual([
      { id: 'n1', title: 'Build', body: 'done', urgency: 'critical' },
      { id: null, title: 'Ping', body: null, urgency: 'low' }
    ])
  })

  it('maps OSC 9 body-only payloads (null title) through unchanged', () => {
    const [n] = parseAtermNotifications(
      JSON.stringify([{ id: null, title: null, body: 'hello', urgency: 'normal' }])
    )
    expect(n).toEqual({ id: null, title: null, body: 'hello', urgency: 'normal' })
  })

  it('normalizes unknown/missing urgency to normal, keeps low/critical', () => {
    const parsed = parseAtermNotifications(
      JSON.stringify([
        { body: 'a', urgency: 'urgent!!' },
        { body: 'b' },
        { body: 'c', urgency: 'low' },
        { body: 'd', urgency: 'critical' }
      ])
    )
    expect(parsed.map((n) => n.urgency)).toEqual(['normal', 'normal', 'low', 'critical'])
  })

  it('is tolerant: undefined / malformed JSON / non-array / junk entries → dropped', () => {
    expect(parseAtermNotifications(undefined)).toEqual([])
    expect(parseAtermNotifications('{nope')).toEqual([])
    expect(parseAtermNotifications('{"id":"x"}')).toEqual([])
    expect(parseAtermNotifications(JSON.stringify([null, 42, 'str']))).toEqual([])
  })

  it('drops text-less notifications (nothing to render OS-side)', () => {
    const parsed = parseAtermNotifications(
      JSON.stringify([{ id: 'x', title: null, body: '', urgency: 'normal' }, { body: 'keep' }])
    )
    expect(parsed).toHaveLength(1)
    expect(parsed[0].body).toBe('keep')
  })
})

// ── Worker side: per-chunk drain + authorize passthrough (mock engine) ──

type NotificationEngineCalls = { authorized: boolean[]; drains: number }

function makeNotificationHandle(pendingJson: () => string | undefined): {
  handle: EngineHandle
  calls: NotificationEngineCalls
} {
  const calls: NotificationEngineCalls = { authorized: [], drains: 0 }
  const engine = {
    display_offset: 0,
    cell_width: 8,
    cell_height: 16,
    cursor_color: undefined,
    take_response: () => undefined,
    take_osc_events: () => undefined,
    take_notifications: () => {
      calls.drains += 1
      return pendingJson()
    },
    drain_bell: () => false,
    authorize_notifications: (allowed: boolean) => calls.authorized.push(allowed),
    scroll_to_bottom: () => undefined
  }
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search: () => new Uint32Array(0),
    dispose: () => undefined
  } as EngineHandle
  return { handle, calls }
}

describe('worker terminal notification channel (mock engine)', () => {
  it('drains take_notifications per processed chunk into the side channels', () => {
    let pending: string | undefined = '[{"id":null,"title":null,"body":"hi","urgency":"normal"}]'
    const { handle, calls } = makeNotificationHandle(() => {
      const out = pending
      pending = undefined
      return out
    })
    const term = createWorkerTerminal(handle)
    const first = term.processBytes('x')
    expect(first.notifications).toContain('"body":"hi"')
    // Drained: the next chunk reports nothing pending.
    const second = term.processBytes('y')
    expect(second.notifications).toBeUndefined()
    expect(calls.drains).toBe(2)
  })

  it('passes setNotificationsAuthorized through to the engine gate', () => {
    const { handle, calls } = makeNotificationHandle(() => undefined)
    const term = createWorkerTerminal(handle)
    term.setNotificationsAuthorized(true)
    term.setNotificationsAuthorized(false)
    expect(calls.authorized).toEqual([true, false])
  })
})

// ── Main side: worker-backed term buffering + authorize command post ──

function makeWorkerState(overrides: Partial<AtermWorkerState> = {}): AtermWorkerState {
  return {
    type: 'state',
    engine: 'cpu',
    width: 0,
    height: 0,
    cols: 80,
    rows: 24,
    cellWidth: 8,
    cellHeight: 16,
    displayOffset: 0,
    displayOriginAbsolute: 0,
    cursorX: 0,
    cursorY: 0,
    cursorStyle: 1,
    baseY: 0,
    isAltScreen: false,
    bracketedPasteMode: false,
    isMouseTracking: false,
    mouseWantsMotion: false,
    mouseWantsAnyMotion: false,
    isFocusEventMode: false,
    isColorSchemeUpdatesMode: false,
    isAppCursorMode: false,
    isAlternateScroll: false,
    keyboardModeBits: 0,
    isReady: true,
    title: null,
    cursorColor: null,
    selectionRange: null,
    hoverLink: null,
    hoverCursor: '',
    searchCount: 0,
    searchActiveIndex: 0,
    searchActiveRect: null,
    searchMatchRects: [],
    dirtyRows: [],
    ...overrides
  }
}

describe('worker-backed term notification channel', () => {
  it('authorize_notifications posts the setNotificationsAuthorized command', () => {
    const posted: AtermWorkerPaneCommand[] = []
    const backed = createWorkerBackedTerm({
      post: (cmd) => posted.push(cmd),
      initial: makeWorkerState()
    })
    ;(
      backed.term as unknown as { authorize_notifications: (b: boolean) => void }
    ).authorize_notifications(true)
    expect(posted).toContainEqual({ type: 'setNotificationsAuthorized', allowed: true })
  })

  it('buffers pushed notifications, drains once via take_notifications, and pings the side channel', () => {
    const backed = createWorkerBackedTerm({ post: () => undefined, initial: makeWorkerState() })
    let pings = 0
    backed.onSideChannel(() => (pings += 1))
    backed.pushNotifications('[{"id":null,"title":"T","body":"B","urgency":"low"}]')
    expect(pings).toBe(1)
    const term = backed.term as unknown as { take_notifications: () => string | undefined }
    const drained = term.take_notifications()
    expect(drained && JSON.parse(drained)).toEqual([
      { id: null, title: 'T', body: 'B', urgency: 'low' }
    ])
    expect(term.take_notifications()).toBeUndefined()
  })

  it('drops a malformed push without breaking later drains', () => {
    const backed = createWorkerBackedTerm({ post: () => undefined, initial: makeWorkerState() })
    backed.pushNotifications('{malformed')
    backed.pushNotifications('[{"body":"ok"}]')
    const term = backed.term as unknown as { take_notifications: () => string | undefined }
    expect(term.take_notifications()).toBe('[{"body":"ok"}]')
  })

  it('pings the side channel when the snapshot cursor colour moves (OSC 12 follow)', () => {
    const backed = createWorkerBackedTerm({ post: () => undefined, initial: makeWorkerState() })
    let pings = 0
    backed.onSideChannel(() => (pings += 1))
    backed.applyState(makeWorkerState({ cursorColor: 0xff8800 }))
    expect(pings).toBe(1)
    // Unchanged colour: no spurious ping.
    backed.applyState(makeWorkerState({ cursorColor: 0xff8800 }))
    expect(pings).toBe(1)
  })
})

// ── Facade: post-process drain → onTerminalAppNotification (fail-closed engine) ──

function makeControllerMock(takeNotifications: () => string | undefined): AtermPaneController {
  return {
    process: () => undefined,
    drainBell: () => false,
    takeOscEvents: () => undefined,
    takeNotifications,
    isAltScreen: () => false,
    selectionRange: () => null,
    onEngineSideChannel: undefined,
    onSelectionMutation: () => undefined,
    setLinkProviderSource: () => undefined,
    dispose: () => undefined
  } as unknown as AtermPaneController
}

describe('facade notification drain', () => {
  it('emits parsed notifications to onTerminalAppNotification after each engine feed', () => {
    let pending: string | undefined
    const facade = createAtermTerminalFacade({ options: {} })
    const seen: unknown[] = []
    facade.onTerminalAppNotification((n) => seen.push(n))
    facade.__attachController(
      makeControllerMock(() => {
        const out = pending
        pending = undefined
        return out
      }),
      {
        element: document.createElement('div'),
        textarea: document.createElement('textarea')
      }
    )
    pending = '[{"id":null,"title":null,"body":"ding","urgency":"critical"}]'
    facade.__feedEngine('output-chunk')
    expect(seen).toEqual([{ id: null, title: null, body: 'ding', urgency: 'critical' }])
  })

  it('emits nothing while the engine drain stays empty (unauthorized fail-closed gate)', () => {
    const facade = createAtermTerminalFacade({ options: {} })
    const seen: unknown[] = []
    facade.onTerminalAppNotification((n) => seen.push(n))
    facade.__attachController(
      makeControllerMock(() => undefined),
      {
        element: document.createElement('div'),
        textarea: document.createElement('textarea')
      }
    )
    facade.__feedEngine('output-chunk')
    expect(seen).toEqual([])
  })

  it('skips the engine crossing entirely while nothing subscribed', () => {
    let drains = 0
    const facade = createAtermTerminalFacade({ options: {} })
    facade.__attachController(
      makeControllerMock(() => {
        drains += 1
        return undefined
      }),
      {
        element: document.createElement('div'),
        textarea: document.createElement('textarea')
      }
    )
    facade.__feedEngine('output-chunk')
    expect(drains).toBe(0)
  })
})
