// The orca-runtime store serializes every row to its TS Row shape as JSON; these
// map the marshalled strings back to typed rows. `optRowFromJson` restores the
// old getter contract where a store `null` (absent row) becomes `undefined`.
export function rowFromJson<T>(json: string): T {
  return JSON.parse(json) as T
}

export function optRowFromJson<T>(json: string | null): T | undefined {
  return json === null ? undefined : (JSON.parse(json) as T)
}

export function listFromJson<T>(json: string): T[] {
  return JSON.parse(json) as T[]
}
