import { beforeEach, describe, expect, it, vi } from 'vitest'

// Proves the Scenes settings row can only appear when the vendored engine build
// actually ships scene art: empty/failed registry reads resolve to no names, so
// the row stays hidden instead of overclaiming (STYLEGUIDE: no overclaiming).

const mocks = vi.hoisted(() => ({
  loadAterm: vi.fn<() => Promise<unknown>>(),
  sceneNamesCsv: vi.fn<() => string>()
}))

vi.mock('@/lib/pane-manager/aterm/load-aterm', () => ({
  loadAterm: mocks.loadAterm
}))
vi.mock('@/lib/pane-manager/aterm/aterm_wasm.js', () => ({
  scene_names_csv: mocks.sceneNamesCsv
}))

type SceneAvailabilityModule = {
  parseSceneNamesCsv: (csv: string) => string[]
  listAtermSceneNames: () => Promise<readonly string[]>
}

// The module caches its names promise, so each test gets a fresh copy.
async function importSceneAvailability(): Promise<SceneAvailabilityModule> {
  vi.resetModules()
  return import('./terminal-engine-scene-availability')
}

beforeEach(() => {
  mocks.loadAterm.mockReset().mockResolvedValue(undefined)
  mocks.sceneNamesCsv.mockReset().mockReturnValue('')
})

describe('parseSceneNamesCsv', () => {
  it('parses a comma-separated registry listing', async () => {
    const { parseSceneNamesCsv } = await importSceneAvailability()
    expect(parseSceneNamesCsv('aurora,tidepool')).toEqual(['aurora', 'tidepool'])
    expect(parseSceneNamesCsv(' aurora , tidepool ')).toEqual(['aurora', 'tidepool'])
  })

  it("treats today's empty registry (and whitespace/empty fields) as no scenes", async () => {
    const { parseSceneNamesCsv } = await importSceneAvailability()
    expect(parseSceneNamesCsv('')).toEqual([])
    expect(parseSceneNamesCsv('  ')).toEqual([])
    expect(parseSceneNamesCsv('aurora,,')).toEqual(['aurora'])
  })
})

describe('listAtermSceneNames', () => {
  it('returns the engine registry names after one shared engine load', async () => {
    mocks.sceneNamesCsv.mockReturnValue('aurora,tidepool')
    const { listAtermSceneNames } = await importSceneAvailability()

    const [first, second] = await Promise.all([listAtermSceneNames(), listAtermSceneNames()])

    expect(first).toEqual(['aurora', 'tidepool'])
    expect(second).toEqual(['aurora', 'tidepool'])
    expect(mocks.loadAterm).toHaveBeenCalledTimes(1)
  })

  it('resolves [] when the engine cannot load, keeping the row hidden', async () => {
    mocks.loadAterm.mockRejectedValue(new Error('wasm init failed'))
    const { listAtermSceneNames } = await importSceneAvailability()

    await expect(listAtermSceneNames()).resolves.toEqual([])
  })
})
