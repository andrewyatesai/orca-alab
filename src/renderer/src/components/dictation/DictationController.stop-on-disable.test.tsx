// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { GlobalSettings } from '../../../../shared/types'
import type { DictationState } from '../../../../shared/speech-types'
import { DICTATION_CONTROL_EVENT } from './dictation-control-events'

// React needs this flag to flush effects/state updates inside act(...).
;(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true

const { useAppStoreMock } = vi.hoisted(() => ({ useAppStoreMock: vi.fn() }))
vi.mock('@/store', () => ({ useAppStore: useAppStoreMock }))

const startCaptureMock = vi.fn(async () => {})
const stopCaptureMock = vi.fn()
vi.mock('@/hooks/use-audio-capture', () => ({
  useAudioCapture: () => ({
    start: startCaptureMock,
    stop: stopCaptureMock,
    flushBufferedAudio: vi.fn(async () => {}),
    discardBufferedAudio: vi.fn(),
    getCapturedChunkCount: () => 0
  })
}))

vi.mock('sonner', () => ({
  toast: Object.assign(vi.fn(), { message: vi.fn(), error: vi.fn(), success: vi.fn() })
}))

vi.mock('./DictationIndicator', () => ({ DictationIndicator: () => null }))

import { DictationController } from './DictationController'

let root: Root | null = null
let container: HTMLDivElement | null = null
let stopDictationIpc: ReturnType<typeof vi.fn>
let onStoppedCallback: ((data: { sessionId: string }) => void) | null = null

type ControllerState = {
  dictationState: DictationState
  settings: GlobalSettings | null
  keybindings: Record<string, unknown>
  setDictationState: ReturnType<typeof vi.fn>
  setPartialTranscript: ReturnType<typeof vi.fn>
  recordFeatureInteraction: ReturnType<typeof vi.fn>
}

let state: ControllerState

function makeSettings(enabled: boolean): GlobalSettings {
  return {
    voice: { enabled, sttModel: 'test-model', dictationMode: 'toggle' }
  } as GlobalSettings
}

function installWindowApi(): void {
  stopDictationIpc = vi.fn(async (sessionId: string) => {
    // Emulate the main process emitting the stopped event so
    // finishDictationSession's waitForStoppedSession can resolve.
    onStoppedCallback?.({ sessionId })
  })
  Object.assign(window, {
    api: {
      speech: {
        startDictation: vi.fn(async () => {}),
        stopDictation: stopDictationIpc,
        feedAudio: vi.fn(async () => undefined),
        onPartialTranscript: vi.fn(() => () => {}),
        onFinalTranscript: vi.fn(() => () => {}),
        onStopped: vi.fn((cb: (data: { sessionId: string }) => void) => {
          onStoppedCallback = cb
          return () => {}
        }),
        onError: vi.fn(() => () => {})
      },
      ui: { onDictationKeyDown: vi.fn(() => () => {}) }
    }
  })
}

async function render(): Promise<void> {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  await act(async () => {
    root?.render(<DictationController />)
  })
}

async function rerender(): Promise<void> {
  await act(async () => {
    root?.render(<DictationController />)
  })
}

beforeEach(() => {
  onStoppedCallback = null
  state = {
    dictationState: 'idle',
    settings: makeSettings(true),
    keybindings: {},
    // Mirror state transitions so a rerender's `dictationStateRef.current =
    // dictationState` reflects the live 'listening' state rather than resetting.
    setDictationState: vi.fn((next: DictationState) => {
      state.dictationState = next
    }),
    setPartialTranscript: vi.fn(),
    recordFeatureInteraction: vi.fn()
  }
  useAppStoreMock.mockImplementation((selector: (s: ControllerState) => unknown) => selector(state))
  installWindowApi()
})

afterEach(async () => {
  if (root) {
    await act(async () => {
      root?.unmount()
    })
  }
  root = null
  container?.remove()
  container = null
  vi.clearAllMocks()
})

describe('DictationController stop-on-disable', () => {
  it('tears down capture and stops the session when Voice is disabled mid-listening', async () => {
    await render()

    // Drive a real dictation start so activeSessionIdRef is populated and the
    // internal state ref reaches 'listening'.
    await act(async () => {
      document.dispatchEvent(new CustomEvent(DICTATION_CONTROL_EVENT, { detail: 'start' }))
    })
    await act(async () => {})

    // Happy-path start does not tear down capture.
    expect(stopCaptureMock).not.toHaveBeenCalled()

    // User disables Voice while still listening.
    state.settings = makeSettings(false)
    await rerender()
    await act(async () => {})

    // The mic/AudioContext must be released and the session stopped, even though
    // no hotkey or control event can fire once Voice is disabled.
    expect(stopCaptureMock).toHaveBeenCalled()
    expect(stopDictationIpc).toHaveBeenCalled()
  })

  it('does not stop anything when Voice is disabled while idle', async () => {
    await render()

    state.settings = makeSettings(false)
    await rerender()
    await act(async () => {})

    expect(stopCaptureMock).not.toHaveBeenCalled()
    expect(stopDictationIpc).not.toHaveBeenCalled()
  })
})
