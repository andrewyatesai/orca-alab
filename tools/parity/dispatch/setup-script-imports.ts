// TS dispatch for the setup-script-imports parity module. The shared TS twin
// (`inspectSetupScriptImportCandidates`) is cut over to the Rust orca-config core
// via the orcaDispatch aggregate (main/runtime readers only), so this adapter
// drives that same napi binding: the vectors carry the pre-read `contentsByPath`
// (+ optional `existingPaths`) the IO edge would supply, and the harness's
// TS-vs-Rust diff degenerates to napi-vs-binary with the TS-derived goldens as
// the absolute pin. Requires the built addon, like the napi-parity suite.
import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'inspectSetupScriptImportCandidates': {
      return JSON.parse(
        requireRustGitBinding().orcaDispatch(
          'setup-script-imports',
          'inspectSetupScriptImportCandidates',
          JSON.stringify(input)
        )
      )
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
