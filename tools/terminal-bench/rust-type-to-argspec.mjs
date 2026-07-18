// Maps a Rust parameter type (as written in a ts2rust corpus kernel's `pub fn`
// signature) to the autoformalize fuzz argspec token that fuzz.mjs understands.
// Kept in its own module so gauntlet.mjs stays within the max-lines budget and the
// type-shape rules live in one auditable place.
//
// The fuzz argspec is a VALUE-SHAPE description, not a Rust type: primitives pass
// through, strings collapse to `str`, slices to `T[]`, and a struct becomes
// `Name{field:type,...}` where each field carries the type the fuzzer needs to emit
// a serde-valid JSON value (a bool field fuzzed as u32 makes serde reject the input:
// `invalid type: integer, expected a boolean`).

const PRIM = new Set(['u32', 'i32', 'u64', 'i64', 'u16', 'i16', 'bool'])
const SLICE = {
  '&[u32]': 'u32[]',
  '&[i32]': 'i32[]',
  '&[u64]': 'u64[]',
  '&[i64]': 'i64[]',
  '&[u16]': 'u16[]',
  '&[i16]': 'i16[]',
  '&[&str]': 'str[]'
}

export function rustTypeToArgspec(ty, src) {
  // Strip lifetime annotations (`&'a str`, `&[&'a str]`) before matching — they are
  // invisible to the fuzz argspec, which cares only about the value shape.
  const t = ty.replace(/'[a-z_]\w*\b/gu, '').replace(/\s+/gu, '')
  if (PRIM.has(t)) {
    return t
  }
  // Owned and borrowed strings share a value shape — both fuzz as `str`.
  if (t === '&str' || t === 'String') {
    return 'str'
  }
  if (SLICE[t]) {
    return SLICE[t]
  }
  // `Option<T>` fuzzes as its inner T (a present value). The field TYPE must reach
  // the fuzzer: without it the field defaulted to u32 and serde rejected the value
  // for a bool/str field. (Null-coverage of the optional is a separate refinement
  // that needs a fuzzer `?`-suffix mode.)
  const opt = t.match(/^Option<(.+)>$/u)
  if (opt) {
    return rustTypeToArgspec(opt[1], src)
  }
  if (/^[A-Z]\w*$/u.test(t)) {
    const structIdx = src.search(new RegExp(`struct\\s+${t}\\s*\\{`, 'u'))
    const m = src.match(new RegExp(`struct\\s+${t}\\s*\\{([^}]*)\\}`, 'u'))
    // The struct's `#[serde(rename_all = "...")]` decides the JSON KEY the fuzzer
    // must emit — a camelCase struct deserializes `mergeStateStatus`, not the
    // snake_case Rust field `merge_state_status` (else serde: `missing field`).
    const renameAll =
      structIdx >= 0
        ? src
            .slice(Math.max(0, structIdx - 240), structIdx)
            .match(/rename_all\s*=\s*"([\w]+)"/u)?.[1]
        : undefined
    const toCamel = (n) => n.replace(/_([a-z0-9])/gu, (_, c) => c.toUpperCase())
    const renameKey = (n) => (renameAll === 'camelCase' ? toCamel(n) : n)
    // Capture each field's NAME **and TYPE**, mapping the type through so the fuzzer
    // emits a serde-valid value per field (was: names only, so every field became a
    // u32 and any bool/str/Option field crashed the differential harness), and the
    // KEY through the struct's rename_all. A field whose type we cannot fuzz (opaque
    // `serde_json::Value`, a map) declines the whole kernel — honest: it cannot be
    // differentially exercised.
    const fields = m
      ? [...m[1].matchAll(/(?:pub\s+)?(\w+)\s*:\s*([^,]+?)\s*(?:,|$)/gu)].map((x) => {
          const ft = rustTypeToArgspec(x[2].trim(), src)
          return ft ? `${renameKey(x[1])}:${ft}` : null
        })
      : []
    return fields.length && fields.every(Boolean) ? `${t}{${fields.join(',')}}` : null
  }
  return null
}
