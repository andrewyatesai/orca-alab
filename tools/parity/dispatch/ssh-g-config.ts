// TS dispatch for the ssh-g-config parity module. The shared TS twin
// (`parseSshGOutput`) is cut over to the Rust orca-ssh core via the orcaDispatch
// aggregate (napi in main, the only process that runs `ssh -G`), so this adapter
// drives that same binding: the vectors' TS-derived goldens pin the surface
// absolutely, and the harness's TS-vs-Rust diff degenerates to napi-vs-binary.
// Requires the built addon, like the napi-parity suite.
import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseSshGOutput': {
      const { stdout, home } = input as { stdout: string; home: string }
      // The Rust core takes `home` explicitly (TS reads os.homedir()); the
      // vector pins it so ~-expansion is reproducible.
      return JSON.parse(
        requireRustGitBinding().orcaDispatch('ssh-g-config', 'parseSshGOutput', JSON.stringify({ stdout, home }))
      )
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
