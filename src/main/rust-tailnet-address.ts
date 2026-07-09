// Main-process tailnet-address classifier, driven by the Rust orca-core core via
// napi (the shared TS impl was deleted). One source of truth with the
// parity-proven Rust port. The vector input is a bare string, so we stringify
// the address directly (matching the Rust dispatch's `input.as_str()`).
import { requireRustGitBinding } from './daemon/rust-git-addon'

export function isTailnetIPv4Address(address: string): boolean {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'tailnet-address',
      'isTailnetIPv4Address',
      JSON.stringify(address)
    )
  ) as boolean
}
