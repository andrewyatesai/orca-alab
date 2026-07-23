// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest'

import { isProjectHeaderDragHandleTarget, isRepoHeaderActionTarget } from './project-header-drag'
import {
  isProjectGroupHeaderActionTarget,
  isProjectGroupHeaderDragHandleTarget
} from './project-group-header-drag'
import { isHostHeaderActionTarget } from './host-header-drag-dom'

// Why: grab-handle and action glyphs render as lucide <svg> (SVGElement), so a
// pointerdown over the painted icon must still be recognized (#8575) — an
// HTMLElement-only guard silently dropped the SVG target and broke reordering.
function createHeader(markup: string): HTMLElement {
  const header = document.createElement('div')
  header.innerHTML = markup
  document.body.appendChild(header)
  return header
}

function svgTarget(header: HTMLElement): SVGElement {
  const svg = header.querySelector('svg')
  if (!svg) {
    throw new Error('expected an <svg> in the header markup')
  }
  return svg
}

describe('header drag guards accept SVG icon targets', () => {
  it('recognizes an <svg> inside the project drag handle', () => {
    const header = createHeader(
      '<div data-repo-header-drag-handle=""><svg><path></path></svg></div>'
    )
    expect(svgTarget(header)).toBeInstanceOf(SVGElement)
    expect(isProjectHeaderDragHandleTarget(svgTarget(header), header)).toBe(true)
  })

  it('recognizes an <svg> inside a project header action button', () => {
    const header = createHeader('<button type="button"><svg><path></path></svg></button>')
    expect(isRepoHeaderActionTarget(svgTarget(header), header)).toBe(true)
  })

  it('recognizes an <svg> inside the project group drag handle', () => {
    const header = createHeader(
      '<div data-project-group-header-drag-handle=""><svg><path></path></svg></div>'
    )
    expect(isProjectGroupHeaderDragHandleTarget(svgTarget(header), header)).toBe(true)
  })

  it('recognizes an <svg> inside a project group action button', () => {
    const header = createHeader('<button type="button"><svg><path></path></svg></button>')
    expect(isProjectGroupHeaderActionTarget(svgTarget(header), header)).toBe(true)
  })

  it('recognizes an <svg> inside a host header action button', () => {
    const header = createHeader('<button type="button"><svg><path></path></svg></button>')
    expect(isHostHeaderActionTarget(svgTarget(header), header)).toBe(true)
  })
})
