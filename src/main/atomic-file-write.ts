import { closeSync, fsyncSync, openSync, renameSync, rmSync, writeSync } from 'node:fs'
import { open, rename, rm } from 'node:fs/promises'
import { dirname } from 'node:path'

// Why fsync: tmp+rename is crash-atomic but NOT power-loss-durable. On APFS (macOS) and ext4 the rename's
// metadata can commit before the file's data blocks reach disk, so an OS crash / power loss can leave a
// durable zero-length or truncated file despite the atomic rename. Fsyncing the tmp file before the rename,
// then fsyncing the containing directory after, makes both the bytes and the rename survive power loss —
// the same guarantee write-file-atomic and electron-store provide. Matters most on network/SSH filesystems
// (per AGENTS.md), where a torn write is far likelier than on a local disk.

function defaultTmpPath(targetPath: string): string {
  return `${targetPath}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`
}

// Why best-effort + win32 skip: opening a directory for fsync isn't supported on Windows; a failure to
// durably-sync the dir must never abort an otherwise-successful write.
function fsyncDirSync(dir: string): void {
  if (process.platform === 'win32') {
    return
  }
  let fd: number | null = null
  try {
    fd = openSync(dir, 'r')
    fsyncSync(fd)
  } catch {
    // Best-effort directory durability.
  } finally {
    if (fd !== null) {
      try {
        closeSync(fd)
      } catch {
        // Ignore secondary close error.
      }
    }
  }
}

async function fsyncDirAsync(dir: string): Promise<void> {
  if (process.platform === 'win32') {
    return
  }
  try {
    const handle = await open(dir, 'r')
    try {
      await handle.sync()
    } finally {
      await handle.close()
    }
  } catch {
    // Best-effort directory durability.
  }
}

export function fsyncDirForFileSync(targetPath: string): void {
  fsyncDirSync(dirname(targetPath))
}

export async function fsyncDirForFileAsync(targetPath: string): Promise<void> {
  await fsyncDirAsync(dirname(targetPath))
}

// Write `data` to `tmpPath` and fsync it to disk (data blocks durable) before returning. Callers that must
// interleave logic between the write and the rename (e.g. a generation re-check) use this plus their own
// renameSync + fsyncDirForFileSync.
export function writeTmpDurableSync(
  tmpPath: string,
  data: string | Uint8Array,
  mode = 0o666
): void {
  // Why normalize to a Buffer and loop: writeSync on a regular file can return a short count for a multi-MB
  // payload (POSIX write()), so a single writeSync could truncate the primary store. Loop until fully written.
  const buffer =
    typeof data === 'string'
      ? Buffer.from(data, 'utf-8')
      : Buffer.isBuffer(data)
        ? data
        : Buffer.from(data)
  const fd = openSync(tmpPath, 'w', mode)
  try {
    let offset = 0
    while (offset < buffer.length) {
      offset += writeSync(fd, buffer, offset, buffer.length - offset)
    }
    fsyncSync(fd)
  } finally {
    closeSync(fd)
  }
}

export async function writeTmpDurableAsync(
  tmpPath: string,
  data: string | Uint8Array,
  mode = 0o666
): Promise<void> {
  const handle = await open(tmpPath, 'w', mode)
  try {
    await handle.writeFile(data)
    await handle.sync()
  } finally {
    await handle.close()
  }
}

export type AtomicWriteOptions = {
  mode?: number
  tmpPath?: string
}

// Durable tmp+fsync+rename+dir-fsync write. Removes the tmp file on any failure so a crash can't leak an orphan.
export function writeFileAtomicSync(
  targetPath: string,
  data: string | Uint8Array,
  options: AtomicWriteOptions = {}
): void {
  const tmpPath = options.tmpPath ?? defaultTmpPath(targetPath)
  let renamed = false
  try {
    writeTmpDurableSync(tmpPath, data, options.mode ?? 0o666)
    renameSync(tmpPath, targetPath)
    renamed = true
    fsyncDirSync(dirname(targetPath))
  } finally {
    if (!renamed) {
      try {
        rmSync(tmpPath, { force: true })
      } catch {
        // Best-effort cleanup; the write already failed.
      }
    }
  }
}

export async function writeFileAtomicAsync(
  targetPath: string,
  data: string | Uint8Array,
  options: AtomicWriteOptions = {}
): Promise<void> {
  const tmpPath = options.tmpPath ?? defaultTmpPath(targetPath)
  let renamed = false
  try {
    await writeTmpDurableAsync(tmpPath, data, options.mode ?? 0o666)
    await rename(tmpPath, targetPath)
    renamed = true
    await fsyncDirAsync(dirname(targetPath))
  } finally {
    if (!renamed) {
      await rm(tmpPath, { force: true }).catch(() => {})
    }
  }
}
