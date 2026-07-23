import { describe, expect, it } from 'vitest'
import { getFeatureWallTerminalShell } from './feature-wall-terminal-shell'

describe('feature wall terminal shell', () => {
  it('uses host-neutral shell copy because the runtime may be local, SSH, or paired', () => {
    expect(getFeatureWallTerminalShell()).toEqual({
      banner: 'Persistent shell session ready',
      prompt: '>'
    })
  })
})
