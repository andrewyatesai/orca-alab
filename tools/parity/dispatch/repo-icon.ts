// TS dispatch for the repo-icon parity module: maps the shared vector function
// names to the real `src/shared/repo-icon.ts` exports so the harness compares
// the live TS reference against the Rust port.

import {
  faviconUrlFromWebsite,
  githubAvatarIcon,
  sanitizeRepoIcon
} from '../../../src/shared/repo-icon'

// `sanitizeRepoIcon` is tri-state: `undefined` (leave as-is), `null` (reset), or
// an icon. `undefined` isn't JSON-representable, so both adapters encode it as
// this sentinel string — icons are always objects, so there's no collision.
const SANITIZE_UNDEFINED = '__undefined__'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'sanitizeRepoIcon': {
      const result = sanitizeRepoIcon(input)
      return result === undefined ? SANITIZE_UNDEFINED : result
    }
    case 'faviconUrlFromWebsite':
      return faviconUrlFromWebsite(input as string)
    case 'githubAvatarIcon':
      return githubAvatarIcon(input as { owner: string; repo: string })
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
