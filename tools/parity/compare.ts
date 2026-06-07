// Semantic JSON equality for the parity diff. Mirrors the Rust
// `compare::json_semantic_eq`: numbers by value (1 === 1.0), object keys
// order-insensitive, arrays order-sensitive, and a missing key is treated the
// same as an explicit `undefined` (both implementations omit absent optionals).

export function semanticEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true

  if (typeof a === 'number' && typeof b === 'number') {
    return a === b || (Number.isNaN(a) && Number.isNaN(b))
  }

  if (a === null || b === null) return a === b
  if (typeof a !== 'object' || typeof b !== 'object') return false

  const aArray = Array.isArray(a)
  const bArray = Array.isArray(b)
  if (aArray !== bArray) return false

  if (aArray && bArray) {
    if (a.length !== b.length) return false
    return a.every((value, index) => semanticEqual(value, b[index]))
  }

  const aObj = a as Record<string, unknown>
  const bObj = b as Record<string, unknown>
  // Ignore keys whose value is `undefined` on either side (JSON-stringify drops them).
  const aKeys = Object.keys(aObj).filter((key) => aObj[key] !== undefined)
  const bKeys = Object.keys(bObj).filter((key) => bObj[key] !== undefined)
  if (aKeys.length !== bKeys.length) return false
  return aKeys.every((key) => semanticEqual(aObj[key], bObj[key]))
}
