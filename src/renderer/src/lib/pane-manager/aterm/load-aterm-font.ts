import fontUrl from '@renderer/assets/fonts/jetbrains-mono.ttf?url'

// Why: the JetBrains Mono face is an immutable, shared asset both the CPU and GPU
// engine loaders need. Fetch it exactly ONCE and hand the same bytes to every
// pane — the CPU and GPU loaders share this so a GPU→CPU context-loss swap never
// re-fetches the font. (A second fetch of the same ?url asset from inside the
// context-loss swap path was observed to hang in Electron, leaving the recovered
// CPU pane unable to build its engine; reusing the bytes avoids that fetch.)
let fontPromise: Promise<Uint8Array> | null = null

async function fetchFontBytesOnce(): Promise<Uint8Array> {
  const response = await fetch(fontUrl)
  return new Uint8Array(await response.arrayBuffer())
}

export function loadAtermFontBytes(): Promise<Uint8Array> {
  fontPromise ??= fetchFontBytesOnce()
  return fontPromise
}
