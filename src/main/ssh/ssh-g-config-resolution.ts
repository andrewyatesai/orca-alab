import { execFile } from 'node:child_process'
import { homedir } from 'node:os'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

export type SshResolvedConfig = {
  hostname: string
  user?: string
  port: number
  identityFile: string[]
  identityAgent?: string
  identitiesOnly: boolean
  forwardAgent: boolean
  /** Effective GSSAPIAuthentication, including distro-wide /etc/ssh defaults. */
  gssapiAuthentication?: boolean
  proxyCommand?: string
  proxyUseFdpass: boolean
  proxyJump?: string
  controlMaster: string
  controlPath?: string
  controlPersist: string
}

const SSH_G_TIMEOUT_MS = 5000

// Why: `ssh -G <host>` asks OpenSSH for the effective config, including
// Include/Match/wildcard inheritance, without reimplementing OpenSSH matching.
export function resolveWithSshG(host: string): Promise<SshResolvedConfig | null> {
  return new Promise((resolve) => {
    let settled = false
    let child: ReturnType<typeof execFile> | undefined
    const timer = setTimeout(() => {
      if (settled) {
        return
      }
      settled = true
      child?.kill()
      resolve(null)
    }, SSH_G_TIMEOUT_MS)

    const settle = (callback: () => void): void => {
      if (settled) {
        return
      }
      settled = true
      clearTimeout(timer)
      callback()
    }

    // Why: '--' prevents host labels starting with '-' from becoming SSH flags.
    // execFile's timeout only signals ssh; keep the null fallback for stuck callbacks.
    try {
      child = execFile('ssh', ['-G', '--', host], { timeout: SSH_G_TIMEOUT_MS }, (err, stdout) => {
        if (err) {
          settle(() => resolve(null))
          return
        }
        settle(() => resolve(parseSshGOutput(stdout)))
      })
    } catch {
      settle(() => resolve(null))
    }
  })
}

// Parsing is cut over to the Rust orca-ssh core via the orcaDispatch aggregate
// (main-only reader); `home` (~-expansion base) is injected here from
// os.homedir() so the core stays pure. Running `ssh -G` above is the IO edge and
// stays in TS.
export function parseSshGOutput(stdout: string): SshResolvedConfig {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'ssh-g-config',
      'parseSshGOutput',
      JSON.stringify({ stdout, home: homedir() })
    )
  ) as SshResolvedConfig
}
