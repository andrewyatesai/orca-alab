// Renderer hosted-review ref normalizers, driven by the Rust hosted-review-refs
// core in the orca-git wasm module (the shared TS impl was gutted). Consumers do
// string ops (.split/.length/.toLowerCase) on the result, so a wasm-load FAILURE
// passes the input string through unchanged rather than returning null, which
// would throw at those callsites.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'

function op(fn: string, input: unknown): string | null {
  if (!isGitWasmReady()) {return null}
  return JSON.parse(orcaDispatch('hosted-review-refs', fn, JSON.stringify(input ?? null))) as string
}

export function normalizeHostedReviewHeadRef(ref: string): string {
  return op('normalizeHostedReviewHeadRef', ref) ?? (typeof ref === 'string' ? ref : '')
}

export function normalizeHostedReviewBaseRef(ref: string): string {
  return op('normalizeHostedReviewBaseRef', ref) ?? (typeof ref === 'string' ? ref : '')
}
