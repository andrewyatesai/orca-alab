// @vitest-environment happy-dom

import { afterEach, describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import { createRichMarkdownExtensions } from './rich-markdown-extensions'
import { createRichMarkdownEditorCodec } from './rich-markdown-source-transport'
import { handleRichMarkdownFilesystemPathPaste } from './rich-markdown-path-paste'

const TEST_KEY = '0123456789abcdef0123456789abcdef'

function makeEditor(): Editor {
  const element = document.createElement('div')
  document.body.appendChild(element)
  return new Editor({
    element,
    extensions: createRichMarkdownExtensions({ codec: createRichMarkdownEditorCodec(TEST_KEY) }),
    content: '',
    contentType: 'markdown',
    editorProps: {
      // Why: mirror the production wiring so the regression test exercises the
      // same paste path order (our handler ahead of the Link extension plugins).
      handlePaste: (_view, event) =>
        handleRichMarkdownFilesystemPathPaste(editorRef, event as ClipboardEvent)
    }
  })
}

let editorRef: Editor

function makeClipboardEvent(plain: string, html = ''): ClipboardEvent {
  const data = new DataTransfer()
  if (plain) {
    data.setData('text/plain', plain)
  }
  if (html) {
    data.setData('text/html', html)
  }
  return new ClipboardEvent('paste', {
    clipboardData: data,
    bubbles: true,
    cancelable: true
  })
}

function pasteIntoEditor(editor: Editor, plain: string, html = ''): string {
  editorRef = editor
  editor.view.focus()
  const event = makeClipboardEvent(plain, html)
  editor.view.dom.dispatchEvent(event)
  return editor.getMarkdown().trimEnd()
}

afterEach(() => {
  document.body.replaceChildren()
})

describe('rich markdown filesystem path paste (real editor)', () => {
  it('keeps a pasted Windows absolute path as plain text, not a broken link', () => {
    const editor = makeEditor()
    try {
      const path = 'C:\\Users\\me\\repo\\CLAUDE.md'
      const markdown = pasteIntoEditor(
        editor,
        path,
        `<a href="http://CLAUDE.md">${path}</a>`
      )
      expect(markdown).toBe(path)
      expect(markdown).not.toContain('http://')
      expect(markdown).not.toContain('](')
    } finally {
      editor.destroy()
    }
  })

  it('keeps a pasted UNC path as plain text', () => {
    const editor = makeEditor()
    try {
      const path = '\\\\server\\share\\notes\\CLAUDE.md'
      const markdown = pasteIntoEditor(editor, path)
      expect(markdown).toBe(path)
      expect(markdown).not.toContain('](')
    } finally {
      editor.destroy()
    }
  })

  it('keeps a pasted POSIX absolute path as plain text', () => {
    const editor = makeEditor()
    try {
      const path = '/home/me/repo/CLAUDE.md'
      const markdown = pasteIntoEditor(editor, path)
      expect(markdown).toBe(path)
      expect(markdown).not.toContain('](')
    } finally {
      editor.destroy()
    }
  })

  it('still autolinks a genuine https URL', () => {
    const editor = makeEditor()
    try {
      const url = 'https://example.com/docs'
      const markdown = pasteIntoEditor(editor, url)
      expect(markdown).toBe(`[${url}](${url})`)
    } finally {
      editor.destroy()
    }
  })
})
