// The wasm binding exposes the rain master but not the host's accessibility
// result or cursor-glow state. Retain the applied activity gates by object
// identity so disabled effects cost no JS->wasm call or worker message.
const matrixRainTargets = new WeakSet<object>()
const cursorGlowTargets = new WeakSet<object>()

function setActivity(targets: WeakSet<object>, target: object, enabled: boolean): void {
  if (enabled) {
    targets.add(target)
  } else {
    targets.delete(target)
  }
}

export function setAtermMatrixRainActivity(target: object, enabled: boolean): void {
  setActivity(matrixRainTargets, target, enabled)
}

export function setAtermCursorGlowActivity(target: object, enabled: boolean): void {
  setActivity(cursorGlowTargets, target, enabled)
}

export function shouldNoteAtermMatrixRainActivity(target: object): boolean {
  return matrixRainTargets.has(target)
}

export function shouldNoteAtermKeystroke(target: object): boolean {
  return matrixRainTargets.has(target) || cursorGlowTargets.has(target)
}
