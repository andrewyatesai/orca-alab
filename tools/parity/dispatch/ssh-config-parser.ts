// TS dispatch for the ssh-config-parser parity module. The shared TS twin
// (`parseSshConfig`) is being cut over to the Rust orca-ssh core (napi in main,
// the only process that reads ~/.ssh/config), so this adapter drives the napi
// binding: the vectors' TS-derived goldens pin that surface absolutely, and the
// harness's TS-vs-Rust diff degenerates to napi-vs-binary. Requires the built
// addon, like the napi-parity suite.
import { requireRustGitBinding } from '../../../src/main/daemon/rust-git-addon'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'parseSshConfig': {
      const { content, home } = input as { content: string; home: string }
      // The Rust core takes `home` explicitly (TS reads os.homedir()); the
      // vector pins it so ~-expansion is reproducible.
      return JSON.parse(requireRustGitBinding().parseSshConfig(content, home))
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
