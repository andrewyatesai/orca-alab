// Per-cell SGR attribute fingerprint, computed identically on the xterm and
// Rust sides so the conformance runner can diff colour + style, not just glyphs.

// Per-cell attribute fingerprint, computed identically on the xterm and Rust
// sides so the conformance runner can diff color + style, not just glyphs.
function fgFingerprint(cell, isFg) {
  const def = isFg ? cell.isFgDefault() : cell.isBgDefault()
  if (def) {
    return 'd'
  }
  const pal = isFg ? cell.isFgPalette() : cell.isBgPalette()
  const v = isFg ? cell.getFgColor() : cell.getBgColor()
  if (pal) {
    return `p${v}`
  }
  return `r${(v >> 16) & 255},${(v >> 8) & 255},${v & 255}`
}
function cellFingerprint(cell) {
  let f = ''
  if (cell.isBold()) {
    f += 'b'
  }
  if (cell.isDim()) {
    f += 'd'
  }
  if (cell.isItalic()) {
    f += 'i'
  }
  if (cell.isUnderline()) {
    f += 'u'
  }
  if (cell.isBlink()) {
    f += 'k'
  }
  if (cell.isInverse()) {
    f += 'v'
  }
  if (cell.isInvisible()) {
    f += 'c'
  }
  if (cell.isStrikethrough()) {
    f += 's'
  }
  if (cell.isOverline()) {
    f += 'o'
  }
  return `${f}/${fgFingerprint(cell, true)}/${fgFingerprint(cell, false)}`
}
function attrGrid(term, cols, rows) {
  const buf = term.buffer.active
  const lines = []
  for (let r = 0; r < rows; r++) {
    const line = buf.getLine(buf.baseY + r)
    const cells = []
    for (let c = 0; c < cols; c++) {
      const cc = line ? line.getCell(c) : null
      cells.push(cc ? cellFingerprint(cc) : '/d/d')
    }
    lines.push(cells.join(';'))
  }
  return lines.join('\n')
}

export { attrGrid }
