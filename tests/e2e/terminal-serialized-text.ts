// Why: the aterm SerializeAddon replays the buffer as control-sequence-laden
// text — each visual row is prefixed with a cursor move (`ESC[<row>;1H`) and a
// clear-line (`ESC[K`), and SGR resets sit between styled spans. When a probe
// marker is longer than the (often deliberately narrow) terminal width, the
// shell wraps it across rows, so the serialized form splits the marker with
// those control sequences in the middle. A raw `serialized.includes(marker)`
// then misses a marker the shell actually printed. Stripping the control
// sequences rejoins the wrapped fragments so marker matching is width-agnostic.
// Safe for the unique random markers these probes use: removing cursor moves can
// only concatenate adjacent row text, never fabricate a fresh UUID-bearing run.

// ESC/BEL built at runtime (not source control chars) so the regexes stay free
// of the no-control-regex lint while still matching the real serialized bytes.
const ESC = String.fromCharCode(27)
const BEL = String.fromCharCode(7)

// Standard ANSI/VT escape families, each anchored on the ESC byte so only real
// control sequences are removed, never ordinary marker characters:
//  - CSI: ESC [ <params> <intermediates> <final>  (SGR, cursor moves, erases)
//  - OSC: ESC ] ... terminated by BEL or ST (ESC \)
//  - other two-byte escapes: ESC <0x40-0x5F> (e.g. ESC M index)
const CSI_SEQUENCE = new RegExp(`${ESC}\\[[0-?]*[ -/]*[@-~]`, 'g')
const OSC_SEQUENCE = new RegExp(`${ESC}\\][^${BEL}${ESC}]*(?:${BEL}|${ESC}\\\\)?`, 'g')
const TWO_BYTE_ESCAPE = new RegExp(`${ESC}[@-Z\\\\-_]`, 'g')

export function stripSerializedControlSequences(serialized: string): string {
  return serialized.replace(OSC_SEQUENCE, '').replace(CSI_SEQUENCE, '').replace(TWO_BYTE_ESCAPE, '')
}
