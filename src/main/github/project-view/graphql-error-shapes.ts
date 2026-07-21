// GraphQL error envelope shapes shared by the ProjectV2 read path: the raw
// error entries `gh api graphql` emits and the Issue.parent capability probe
// that decides whether to retry a table fetch without the parent selection.
export type GhGraphqlErrorShape = {
  type?: string
  message?: string
  path?: (string | number)[]
  extensions?: { code?: string }
}

export function extractGraphqlErrors(stderr: string, stdout: string): GhGraphqlErrorShape[] {
  // `gh api graphql` prints the response JSON to stdout even on GraphQL
  // errors, and the stderr carries a summary. Try stdout first; if parsing
  // fails, fall back to stderr.
  const sources = [stdout, stderr]
  for (const src of sources) {
    if (!src) {
      continue
    }
    try {
      const parsed = JSON.parse(src) as { errors?: GhGraphqlErrorShape[] }
      if (parsed.errors && parsed.errors.length > 0) {
        return parsed.errors
      }
    } catch {
      // not JSON — continue
    }
  }
  return []
}

export function errorsIndicateParentField(errors: GhGraphqlErrorShape[], stderr: string): boolean {
  const lower = stderr.toLowerCase()
  // Preview-header shape: gh returns a 4xx with "preview" in the message.
  if (lower.includes('preview') && lower.includes('parent')) {
    return true
  }
  return errors.some((e) => {
    const type = (e.type ?? '').toUpperCase()
    if (type === 'FIELD_NOT_FOUND' || type === 'UNDEFINED_FIELD' || type === 'FIELD_ERRORS') {
      const tail = e.path?.at(-1)
      if (tail === 'parent') {
        return true
      }
      // FIELD_ERRORS often omits `path`; match on message for the parent field.
      if ((e.message ?? '').toLowerCase().includes('parent')) {
        return true
      }
    }
    return false
  })
}

