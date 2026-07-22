import fs from 'node:fs/promises'
import type { Dirent } from 'node:fs'
import path from 'node:path'
import { randomUUID } from 'node:crypto'

import { app } from 'electron'
import { requireSshFilesystemProvider } from '../providers/ssh-filesystem-dispatch'
import { isWindowsAbsolutePathLike } from '../../shared/cross-platform-path'
import { assertClipboardImageByteLengthWithinLimit } from '../../shared/clipboard-image'

export type SaveClipboardImageAsTempFileArgs = {
  connectionId?: string | null
  runtimeEnvironmentId?: string | null
}

const REMOTE_CLIPBOARD_IMAGE_TEMP_DIR = '/tmp'
const CLIPBOARD_IMAGE_TEMP_DIR_PREFIX = 'orca-paste-'
const CLIPBOARD_IMAGE_TEMP_DIR_TTL_MS = 7 * 24 * 60 * 60 * 1000

function joinRemotePath(basePath: string, fileName: string): string {
  if (isWindowsAbsolutePathLike(basePath)) {
    return path.win32.join(basePath, fileName)
  }
  return path.posix.join(basePath, fileName)
}

export async function saveClipboardImageBufferAsTempFile(
  buffer: Buffer,
  args?: SaveClipboardImageAsTempFileArgs
): Promise<string> {
  assertClipboardImageByteLengthWithinLimit(buffer.byteLength)

  if (args?.connectionId) {
    const fileName = `${CLIPBOARD_IMAGE_TEMP_DIR_PREFIX}${Date.now()}-${randomUUID()}.png`
    const provider = requireSshFilesystemProvider(args.connectionId)
    const remoteTempDir = (await provider.getTempDir?.()) ?? REMOTE_CLIPBOARD_IMAGE_TEMP_DIR
    const remotePath = joinRemotePath(remoteTempDir, fileName)
    // Why: SSH terminal agents run on the remote host, so the pasted path must
    // name a remote file. The provider's base64 path writes binary bytes via SFTP.
    await provider.writeFileBase64(remotePath, buffer.toString('base64'))
    return remotePath
  }

  // Why: mkdtemp creates a 0700 dir on POSIX so pasted images aren't readable by
  // other users of the shared temp root (#7333); Windows %TEMP% is already per-user.
  const tempDir = await fs.mkdtemp(path.join(app.getPath('temp'), CLIPBOARD_IMAGE_TEMP_DIR_PREFIX))
  // Why: downstream paste detection matches an `orca-paste-*.png` basename, so keep the prefix.
  const tempPath = path.join(tempDir, `${CLIPBOARD_IMAGE_TEMP_DIR_PREFIX}${randomUUID()}.png`)
  await fs.writeFile(tempPath, buffer)
  return tempPath
}

// Why: pasted-image temp dirs otherwise accumulate forever (#7333); sweep stale
// ones best-effort at startup, long after any agent could still need the path.
export async function cleanupExpiredClipboardImageTempDirs(nowMs = Date.now()): Promise<void> {
  const tempRoot = app.getPath('temp')
  let entries: Dirent[]
  try {
    entries = await fs.readdir(tempRoot, { withFileTypes: true })
  } catch {
    return
  }

  await Promise.all(
    entries.map(async (entry) => {
      if (!entry.isDirectory() || !entry.name.startsWith(CLIPBOARD_IMAGE_TEMP_DIR_PREFIX)) {
        return
      }
      const tempDir = path.join(tempRoot, entry.name)
      try {
        const tempStats = await fs.stat(tempDir)
        if (nowMs - tempStats.mtimeMs < CLIPBOARD_IMAGE_TEMP_DIR_TTL_MS) {
          return
        }
        await fs.rm(tempDir, { recursive: true, force: true })
      } catch {
        // Why: stale paste dirs must not make startup cleanup noisy.
      }
    })
  )
}
