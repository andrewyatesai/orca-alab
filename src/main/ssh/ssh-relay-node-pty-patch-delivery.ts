import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { joinRemotePath, type RemoteHostPlatform } from './ssh-remote-platform'

// Why (#8855/#9586): the remote relay installs vanilla node-pty from the npm
// registry, so Orca's pnpm-patched runtime fixes (ConPTY agent AttachConsole
// fallback, asar-safe spawn-helper resolution) never reach SSH hosts on their
// own. build-relay.mjs bundles the patched runtime JS under this dir inside
// the relay package; the deploy overwrites the freshly installed files with
// these copies after `npm install`.
export const NODE_PTY_PATCH_PAYLOAD_DIR = 'node-pty-patched'

export type PatchedNodePtyFile = {
  /** Path relative to the node-pty package root, always '/'-separated. */
  packageRelativePosixPath: string
  contents: string
}

/**
 * Read the patched node-pty payload bundled into the local relay package.
 * Returns [] when the payload dir is absent (relay built before the payload
 * existed, or an ORCA_RELAY_PATH override without it).
 */
export function collectPatchedNodePtyPayload(localRelayDir: string): PatchedNodePtyFile[] {
  const files: PatchedNodePtyFile[] = []
  const walk = (dir: string, relPosixPath: string): void => {
    let entries
    try {
      entries = readdirSync(dir, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const childRelPath = relPosixPath ? `${relPosixPath}/${entry.name}` : entry.name
      if (entry.isDirectory()) {
        walk(join(dir, entry.name), childRelPath)
      } else if (entry.isFile()) {
        files.push({
          packageRelativePosixPath: childRelPath,
          contents: readFileSync(join(dir, entry.name), 'utf-8')
        })
      }
    }
  }
  walk(join(localRelayDir, NODE_PTY_PATCH_PAYLOAD_DIR), '')
  return files
}

/**
 * Overwrite the remote `node_modules/node-pty` runtime files with the patched
 * payload from the local relay package. Runs after the remote `npm install`
 * so the vanilla registry files never survive an install. Returns the number
 * of files delivered (0 when the local package carries no payload).
 */
export async function deliverPatchedNodePtyFiles(options: {
  localRelayDir: string
  remoteDir: string
  hostPlatform: RemoteHostPlatform
  writeRemoteFile: (remotePath: string, contents: string) => Promise<void>
}): Promise<number> {
  const payload = collectPatchedNodePtyPayload(options.localRelayDir)
  for (const file of payload) {
    const remotePath = joinRemotePath(
      options.hostPlatform,
      options.remoteDir,
      'node_modules/node-pty',
      file.packageRelativePosixPath
    )
    await options.writeRemoteFile(remotePath, file.contents)
  }
  return payload.length
}
