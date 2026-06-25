import { describe, expect, it } from 'vitest'
import { stripSerializedControlSequences } from './terminal-serialized-text'

const ESC = String.fromCharCode(27)
const BEL = String.fromCharCode(7)

describe('stripSerializedControlSequences', () => {
  it('rejoins a marker the serialize step wrapped across narrow-terminal rows', () => {
    // Real shape captured from the aterm SerializeAddon: a long marker wrapped at
    // a narrow width, each continuation row prefixed with a cursor move + clear.
    const marker = 'ORCA_PTY_READY_18f86068-e5dd-4886-a906-ab3e00a56810'
    const wrapped =
      `ORCA_PTY_READY_18f86068-e5dd-4886${ESC}[0m${ESC}[26;1H${ESC}[K` +
      `-a906-ab3e00a56810${ESC}[0m${ESC}[27;1H${ESC}[K`

    expect(wrapped.includes(marker)).toBe(false)
    expect(stripSerializedControlSequences(wrapped).includes(marker)).toBe(true)
  })

  it('rejoins a "<marker>:<value>" pair split across rows so the value parses', () => {
    const wrapped = `ORCA_PTY_COLUMNS_abc:3${ESC}[0m${ESC}[6;1H${ESC}[K2\r\n`
    const match = stripSerializedControlSequences(wrapped).match(/ORCA_PTY_COLUMNS_abc:(\d+)/)
    expect(match?.[1]).toBe('32')
  })

  it('strips OSC title sequences terminated by BEL or ST', () => {
    expect(stripSerializedControlSequences(`${ESC}]0;a title${BEL}KEEP`)).toBe('KEEP')
    expect(stripSerializedControlSequences(`${ESC}]0;a title${ESC}\\KEEP`)).toBe('KEEP')
  })

  it('preserves ordinary marker characters (uppercase, underscore, hyphen, colon)', () => {
    const plain = 'HELLO_WORLD-A_B:42'
    expect(stripSerializedControlSequences(plain)).toBe(plain)
  })
})
