import { describe, expect, it, vi } from 'vitest'
import { createFederatedLinearScanReader } from './aterm-federated-linear-scan-reader'

// Fast unit coverage of the coordinate math + fail-closed shape. The REAL
// engine's viewport-only row_range_json behavior (and the never-silent-empty
// scan guarantee) is pinned separately against the committed wasm in
// federated-fed-exports-real-engine.test.ts.

describe('createFederatedLinearScanReader', () => {
  it('is null on pins WITHOUT row_range_json (over-budget stays an honest empty batch)', () => {
    const engine = { search_display_origin: 0, display_offset: 0 }
    expect(createFederatedLinearScanReader(engine, 24, 80)).toBeNull()
  })

  it('advertises the READABLE viewport window and maps abs→display rows', () => {
    const row_range_json = vi.fn((first: number, count: number) =>
      JSON.stringify(
        Array.from({ length: count }, (_, i) => ({
          text: `display-row-${first + i}`,
          wrapped: false,
          len: 3
        }))
      )
    )
    // Viewport top absolute row = search_display_origin - display_offset = 100.
    const engine = { search_display_origin: 100, display_offset: 0, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    // Readable window is the viewport [100, 124) — NOT the full retained history.
    expect(reader?.oldestAbsRow).toBe(100)
    expect(reader?.rowCount).toBe(24)
    // History exists below the viewport top → the scan must settle incomplete.
    expect(reader?.hasUnreadableDepth).toBe(true)
    // Absolute row 120 → display row 20 (absRow − viewportTopAbs).
    const rows = reader?.read(120, 2)
    expect(row_range_json).toHaveBeenCalledExactlyOnceWith(20, 2)
    expect(rows).toEqual(['display-row-20', 'display-row-21'])
  })

  it('honors display_offset when the pane is scrolled into history', () => {
    const row_range_json = vi.fn((first: number, count: number) =>
      JSON.stringify(Array.from({ length: count }, (_, i) => ({ text: `d${first + i}`, wrapped: false, len: 1 })))
    )
    // Scrolled up by 5: viewport top absolute = 100 − 5 = 95.
    const engine = { search_display_origin: 100, display_offset: 5, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 10, 80)
    expect(reader?.oldestAbsRow).toBe(95)
    reader?.read(95, 1)
    expect(row_range_json).toHaveBeenCalledExactlyOnceWith(0, 1)
  })

  it('refuses a range that leaves the viewport (scrollback is un-readable here)', () => {
    const row_range_json = vi.fn(() => JSON.stringify([{ text: 'x', wrapped: false, len: 1 }]))
    const engine = { search_display_origin: 100, display_offset: 0, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    // absRow 90 is below the viewport top (100) → null, no engine call.
    expect(reader?.read(90, 1)).toBeNull()
    expect(row_range_json).not.toHaveBeenCalled()
  })

  it('a fresh short pane (nothing scrolled off) has no unreadable depth', () => {
    const row_range_json = vi.fn(() => '[]')
    const engine = { search_display_origin: 0, display_offset: 0, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    expect(reader?.oldestAbsRow).toBe(0)
    expect(reader?.hasUnreadableDepth).toBe(false)
  })

  it('propagates a reader gap as null (row_range_json returned undefined)', () => {
    const row_range_json = vi.fn(() => undefined)
    const engine = { search_display_origin: 0, display_offset: 0, row_range_json }
    const reader = createFederatedLinearScanReader(engine, 24, 80)
    expect(reader?.read(0, 4)).toBeNull()
  })
})
