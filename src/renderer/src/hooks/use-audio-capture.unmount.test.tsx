// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAudioCapture } from './use-audio-capture'

type CaptureApi = ReturnType<typeof useAudioCapture>

let root: Root | null = null
let container: HTMLDivElement | null = null
let captureApi: CaptureApi | null = null

const trackStop = vi.fn()
const contextClose = vi.fn()
const processorDisconnect = vi.fn()
const sourceDisconnect = vi.fn()

class MockAudioContext {
  state = 'running'
  sampleRate = 48000
  destination = {}
  resume = vi.fn(async () => {})
  close = vi.fn(async () => {
    this.state = 'closed'
    contextClose()
  })
  createMediaStreamSource(): { connect: () => void; disconnect: () => void } {
    return { connect: vi.fn(), disconnect: sourceDisconnect }
  }
  createScriptProcessor(): {
    connect: () => void
    disconnect: () => void
    onaudioprocess: ((e: unknown) => void) | null
  } {
    return { connect: vi.fn(), disconnect: processorDisconnect, onaudioprocess: null }
  }
}

function Probe(): null {
  captureApi = useAudioCapture()
  return null
}

async function renderProbe(): Promise<void> {
  container = document.createElement('div')
  document.body.appendChild(container)
  root = createRoot(container)
  await act(async () => {
    root?.render(<Probe />)
  })
}

beforeEach(() => {
  vi.stubGlobal('AudioContext', MockAudioContext)
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: {
      getUserMedia: vi.fn(async () => ({
        getTracks: () => [{ stop: trackStop }]
      }))
    }
  })
  Object.assign(window, {
    api: { speech: { feedAudio: vi.fn(async () => undefined) } }
  })
})

afterEach(() => {
  root = null
  container?.remove()
  container = null
  captureApi = null
  vi.unstubAllGlobals()
  vi.clearAllMocks()
})

describe('useAudioCapture unmount teardown', () => {
  it('stops the MediaStream and closes the AudioContext when the consumer unmounts mid-capture', async () => {
    await renderProbe()

    await act(async () => {
      await captureApi?.start({ sessionId: 'test' })
    })

    // Capture is live: nothing torn down yet.
    expect(trackStop).not.toHaveBeenCalled()
    expect(contextClose).not.toHaveBeenCalled()

    await act(async () => {
      root?.unmount()
    })

    // The unmount guard must release the mic + audio graph even though the
    // consumer never called stop().
    expect(trackStop).toHaveBeenCalledTimes(1)
    expect(contextClose).toHaveBeenCalledTimes(1)
    expect(processorDisconnect).toHaveBeenCalled()
    expect(sourceDisconnect).toHaveBeenCalled()
  })
})
