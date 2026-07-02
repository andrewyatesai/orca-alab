import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Mock } from 'vitest'
import type { GlobalSettings } from '../../../../shared/types'

type Cleanup = () => void

type MockPreviewEngine = {
  applyTheme: Mock
  applyFontAndBuffer: Mock
  dispose: Mock
}

// The preview drives the REAL aterm engine through this seam module. The wasm
// engine can't load in the node vitest env, so we mock OUR seam to assert the
// COMPONENT's lifecycle wiring (create once, dispose on unmount, re-theme /
// re-feed on setting changes). The engine's real aterm rendering is exercised in
// the running app, not faked here.
const mockEngine = vi.hoisted(() => ({
  instances: [] as MockPreviewEngine[],
  createArgs: [] as Record<string, unknown>[],
  // createTerminalPreviewAtermEngine is async; resolve synchronously-ish so the
  // mount effect's .then() runs within the test's microtask flush.
  resolveValue: true as boolean,
  createImpl: null as null | (() => MockPreviewEngine | null)
}))

const mockReactRuntime = vi.hoisted(() => ({
  cleanups: [] as Cleanup[],
  canvas: { nodeName: 'CANVAS' },
  refCallIndex: 0,
  effects: [] as (() => void | Cleanup)[]
}))

vi.mock('react', async () => {
  const actual = await vi.importActual<typeof import('react')>('react') // eslint-disable-line @typescript-eslint/consistent-type-imports -- vi.importActual requires inline import()
  return {
    ...actual,
    useEffect: (effect: () => void | Cleanup) => {
      const cleanup = effect()
      if (typeof cleanup === 'function') {
        mockReactRuntime.cleanups.push(cleanup)
      }
    },
    useMemo: (factory: () => unknown) => factory(),
    useRef: (initialValue: unknown) => {
      const ref = { current: initialValue }
      // First useRef in the component is canvasRef; seed it with a stub canvas.
      if (mockReactRuntime.refCallIndex === 0) {
        ref.current = mockReactRuntime.canvas
      }
      mockReactRuntime.refCallIndex += 1
      return ref
    },
    useState: (initialValue: unknown) => [
      typeof initialValue === 'function' ? (initialValue as () => unknown)() : initialValue,
      vi.fn()
    ]
  }
})

vi.mock('./terminal-preview-aterm-engine', () => ({
  createTerminalPreviewAtermEngine: vi.fn(async (args: Record<string, unknown>) => {
    mockEngine.createArgs.push(args)
    const engine = mockEngine.createImpl
      ? mockEngine.createImpl()
      : mockEngine.resolveValue
        ? { applyTheme: vi.fn(), applyFontAndBuffer: vi.fn(), dispose: vi.fn() }
        : null
    if (engine) {
      mockEngine.instances.push(engine)
    }
    return engine
  })
}))

vi.mock('@/components/ui/card', () => ({
  Card: 'Card',
  CardContent: 'CardContent',
  CardDescription: 'CardDescription',
  CardHeader: 'CardHeader',
  CardTitle: 'CardTitle'
}))

vi.mock('@/components/terminal-pane/terminal-appearance', () => ({
  composeActiveTerminalTheme: () => ({ background: '#111111', foreground: '#eeeeee' })
}))

vi.mock('@/lib/pane-manager/aterm/aterm-theme-colors', () => ({
  atermThemeColorsFromITheme: (theme: Record<string, unknown>) => ({
    fg: 0xeeeeee,
    bg: 0x111111,
    cursor: 0xffffff,
    selection: 0x264f78,
    selectionForeground: null,
    palette: [],
    _from: theme
  })
}))

vi.mock('@/lib/terminal-theme', () => ({
  clampNumber: (value: number, min: number, max: number) => Math.max(min, Math.min(max, value)),
  resolveEffectiveTerminalAppearance: () => ({
    dividerColor: '#333333',
    theme: { background: '#000000' }
  })
}))

import { TerminalSettingsPreview } from './TerminalSettingsPreview'
import { createTerminalPreviewAtermEngine } from './terminal-preview-aterm-engine'

function makeSettings(overrides: Partial<GlobalSettings> = {}): GlobalSettings {
  return {
    theme: 'dark',
    terminalFontFamily: 'SF Mono',
    terminalFontSize: 14,
    terminalFontWeight: 400,
    terminalLineHeight: 1,
    terminalCursorStyle: 'block',
    terminalCursorBlink: true,
    terminalLigatures: 'off',
    terminalThemeDark: 'Dark',
    terminalThemeLight: 'Light',
    terminalUseSeparateLightTheme: true,
    terminalDividerColorDark: '#333333',
    terminalDividerColorLight: '#dddddd',
    terminalColorOverrides: {},
    terminalBackgroundOpacity: 1,
    terminalCursorOpacity: 1,
    terminalDividerThicknessPx: 3,
    terminalInactivePaneOpacity: 0.6,
    ...overrides
  } as GlobalSettings
}

function renderPreview(settings = makeSettings()): void {
  TerminalSettingsPreview({
    title: 'Preview',
    description: 'Preview description',
    settings,
    systemPrefersDark: true
  })
}

function runCleanups(): void {
  for (const cleanup of [...mockReactRuntime.cleanups].toReversed()) {
    cleanup()
  }
  mockReactRuntime.cleanups.length = 0
}

// Let the mount effect's async createTerminalPreviewAtermEngine().then() resolve.
async function flushMicrotasks(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

describe('TerminalSettingsPreview engine lifecycle', () => {
  beforeEach(() => {
    mockReactRuntime.cleanups.length = 0
    mockReactRuntime.refCallIndex = 0
    mockEngine.instances.length = 0
    mockEngine.createArgs.length = 0
    mockEngine.resolveValue = true
    mockEngine.createImpl = null
    vi.mocked(createTerminalPreviewAtermEngine).mockClear()
  })

  afterEach(() => {
    runCleanups()
  })

  it('creates the aterm preview engine once on mount with the fixed grid + cursor-seeded buffer', async () => {
    renderPreview()

    expect(createTerminalPreviewAtermEngine).toHaveBeenCalledOnce()
    const args = mockEngine.createArgs[0]
    expect(args).toMatchObject({
      canvas: mockReactRuntime.canvas,
      cols: 36,
      rows: 15,
      fontPx: 14
    })
    // The buffer carries the user's cursor style as a DECSCUSR sequence so the
    // trailing prompt renders the chosen shape. Block + blink → CSI 1 SP q.
    expect(String(args.buffer)).toContain('\x1b[1 q')

    await flushMicrotasks()
    expect(mockEngine.instances).toHaveLength(1)
  })

  it('disposes the engine on unmount', async () => {
    renderPreview()
    await flushMicrotasks()

    const engine = mockEngine.instances[0]
    runCleanups()
    expect(engine.dispose).toHaveBeenCalledOnce()
  })

  it('disposes an engine that resolves after the component already unmounted', async () => {
    // The mount effect's cleanup sets cancelled=true synchronously; the late
    // engine must be disposed by the .then() guard, not leaked.
    renderPreview()
    // Unmount before the async create resolves.
    runCleanups()
    await flushMicrotasks()

    expect(mockEngine.instances).toHaveLength(1)
    expect(mockEngine.instances[0].dispose).toHaveBeenCalledOnce()
  })

  it('tolerates a null engine result (load cancelled) without throwing', async () => {
    mockEngine.resolveValue = false
    expect(() => renderPreview()).not.toThrow()
    await flushMicrotasks()
    expect(mockEngine.instances).toHaveLength(0)
  })
})
