// End-to-end proof: load the wasm aterm renderer, feed PTY bytes, rasterize to
// RGBA, assert it produced real glyph pixels, and write a PNG to look at.
import { readFileSync, writeFileSync } from 'node:fs'
import { deflateSync } from 'node:zlib'
import { AtermTerminal } from './pkg/aterm_wasm.js'

const FONT = '/System/Library/Fonts/Supplemental/Andale Mono.ttf'
const font = new Uint8Array(readFileSync(FONT))

const t = new AtermTerminal(12, 48, font, 18)
t.process(new TextEncoder().encode('\x1b[1;32mhello\x1b[0m \x1b[1;34materm\x1b[0m in \x1b[1;31mwasm\x1b[0m\r\n'))
t.process(new TextEncoder().encode('\x1b[33m$\x1b[0m echo "rendered by aterm-render"\r\n'))
t.process(new TextEncoder().encode('\x1b[7m inverse \x1b[0m \x1b[4munderline\x1b[0m 12345 ┌─┐│ │└─┘\r\n'))
t.render()

const w = t.width
const h = t.height
const rgba = t.rgba()
if (!(w > 0 && h > 0)) throw new Error('empty frame')
if (rgba.length !== w * h * 4) throw new Error(`rgba size ${rgba.length} != ${w}*${h}*4`)
const bg = rgba.slice(0, 4)
let nonBg = 0
const colors = new Set()
for (let i = 0; i < rgba.length; i += 4) {
  if (rgba[i] !== bg[0] || rgba[i + 1] !== bg[1] || rgba[i + 2] !== bg[2]) nonBg++
  colors.add((rgba[i] << 16) | (rgba[i + 1] << 8) | rgba[i + 2])
}
console.log(`frame: ${w}x${h}px | non-background pixels: ${nonBg} | distinct colors: ${colors.size}`)
if (nonBg < 100) throw new Error('too few non-background pixels — nothing rendered')
if (colors.size < 4) throw new Error('expected multiple SGR colors in the frame')

// Minimal PNG encoder (RGBA, no filter) so we can view the result.
function png(width, height, rgbaBuf) {
  const raw = Buffer.alloc((width * 4 + 1) * height)
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0 // filter: none
    rgbaBuf.copy
      ? rgbaBuf.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4)
      : Buffer.from(rgbaBuf.subarray(y * width * 4, (y + 1) * width * 4)).copy(
          raw,
          y * (width * 4 + 1) + 1
        )
  }
  const crc = (buf) => {
    let c = ~0
    for (const b of buf) {
      c ^= b
      for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1
    }
    return (~c) >>> 0
  }
  const chunk = (type, data) => {
    const len = Buffer.alloc(4)
    len.writeUInt32BE(data.length)
    const td = Buffer.concat([Buffer.from(type), data])
    const c = Buffer.alloc(4)
    c.writeUInt32BE(crc(td))
    return Buffer.concat([len, td, c])
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // RGBA
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0))
  ])
}

const out = '/tmp/aterm-wasm-render.png'
writeFileSync(out, png(w, h, Buffer.from(rgba)))
console.log(`✅ aterm rendered a terminal in wasm -> ${out}`)
