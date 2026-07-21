import { beforeEach, describe, expect, it, vi } from 'vitest'
import { handleRichMarkdownImagePaste } from './rich-markdown-paste-image'
import { handleRichMarkdownFilesystemPathPaste } from './rich-markdown-path-paste'
import { handleRichMarkdownLargeTextPaste } from './rich-markdown-large-text-paste'
import { handleRichMarkdownTerminalPathPaste } from './rich-markdown-terminal-path-paste'
import { handleRichMarkdownPaste } from './rich-markdown-paste-handler'

vi.mock('./rich-markdown-paste-image', () => ({
  handleRichMarkdownImagePaste: vi.fn()
}))

vi.mock('./rich-markdown-path-paste', () => ({
  handleRichMarkdownFilesystemPathPaste: vi.fn()
}))

vi.mock('./rich-markdown-large-text-paste', () => ({
  handleRichMarkdownLargeTextPaste: vi.fn()
}))

vi.mock('./rich-markdown-terminal-path-paste', () => ({
  handleRichMarkdownTerminalPathPaste: vi.fn()
}))

describe('rich markdown paste handler', () => {
  beforeEach(() => {
    vi.mocked(handleRichMarkdownImagePaste).mockReset()
    vi.mocked(handleRichMarkdownFilesystemPathPaste).mockReset()
    vi.mocked(handleRichMarkdownLargeTextPaste).mockReset()
    vi.mocked(handleRichMarkdownTerminalPathPaste).mockReset()
  })

  it('keeps image paste ownership ahead of large text fallback', () => {
    vi.mocked(handleRichMarkdownImagePaste).mockReturnValue(true)
    const editor = {} as never
    const event = {} as ClipboardEvent

    expect(
      handleRichMarkdownPaste({
        editor,
        event,
        filePath: '/repo/note.md',
        worktreeId: 'wt-1'
      })
    ).toBe(true)

    expect(handleRichMarkdownImagePaste).toHaveBeenCalledWith({
      editor,
      event,
      filePath: '/repo/note.md',
      worktreeId: 'wt-1',
      runtimeEnvironmentId: undefined
    })
    expect(handleRichMarkdownFilesystemPathPaste).not.toHaveBeenCalled()
    expect(handleRichMarkdownTerminalPathPaste).not.toHaveBeenCalled()
    expect(handleRichMarkdownLargeTextPaste).not.toHaveBeenCalled()
  })

  it('claims a filesystem path paste before terminal path and large text fallbacks', () => {
    vi.mocked(handleRichMarkdownImagePaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownFilesystemPathPaste).mockReturnValue(true)
    const editor = {} as never
    const event = {} as ClipboardEvent

    expect(
      handleRichMarkdownPaste({
        editor,
        event,
        filePath: '/repo/note.md',
        worktreeId: 'wt-1'
      })
    ).toBe(true)

    expect(handleRichMarkdownFilesystemPathPaste).toHaveBeenCalledWith(editor, event)
    expect(handleRichMarkdownTerminalPathPaste).not.toHaveBeenCalled()
    expect(handleRichMarkdownLargeTextPaste).not.toHaveBeenCalled()
  })

  it('keeps terminal path paste ahead of large text fallback', () => {
    vi.mocked(handleRichMarkdownImagePaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownFilesystemPathPaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownTerminalPathPaste).mockReturnValue(true)
    const editor = {} as never
    const event = {} as ClipboardEvent

    expect(
      handleRichMarkdownPaste({
        editor,
        event,
        filePath: '/repo/note.md',
        worktreeId: 'wt-1'
      })
    ).toBe(true)

    expect(handleRichMarkdownTerminalPathPaste).toHaveBeenCalledWith(editor, event)
    expect(handleRichMarkdownLargeTextPaste).not.toHaveBeenCalled()
  })

  it('falls through to large text handling when image and path paste decline ownership', () => {
    vi.mocked(handleRichMarkdownImagePaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownFilesystemPathPaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownTerminalPathPaste).mockReturnValue(false)
    vi.mocked(handleRichMarkdownLargeTextPaste).mockReturnValue(true)
    const editor = {} as never
    const event = {} as ClipboardEvent

    expect(
      handleRichMarkdownPaste({
        editor,
        event,
        filePath: '/repo/note.md',
        worktreeId: 'wt-1',
        runtimeEnvironmentId: 'env-1'
      })
    ).toBe(true)

    expect(handleRichMarkdownLargeTextPaste).toHaveBeenCalledWith(editor, event)
  })
})
