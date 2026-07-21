export type WslUncPathInfo = {
  distro: string
  linuxPath: string
}

export function parseWslUncPath(path: string): WslUncPathInfo | null {
  const normalized = path.replace(/\\/g, '/')
  const match = normalized.match(/^\/\/(wsl\.localhost|wsl\$)\/([^/]+)(\/.*)?$/i)
  if (!match) {
    return null
  }

  return {
    distro: match[2],
    linuxPath: match[3] || '/'
  }
}

export function isWslUncPath(path: string): boolean {
  return parseWslUncPath(path) !== null
}

/** Convert an absolute Linux path in a known WSL distro to its Windows form. */
export function toWindowsWslPath(linuxPath: string, distro: string): string {
  const mntMatch = linuxPath.match(/^\/mnt\/([a-z])(\/.*)?$/)
  if (mntMatch) {
    const rest = (mntMatch[2] || '').replace(/\//g, '\\')
    return `${mntMatch[1].toUpperCase()}:${rest || '\\'}`
  }

  return `\\\\wsl.localhost\\${distro}${linuxPath.replace(/\//g, '\\')}`
}

// Why (issue #8156): terminals in WSL worktrees print POSIX paths the Windows
// host cannot stat or open; rebase them onto the worktree's own UNC share so
// path-exists probes and file-open routing resolve them. Null when the worktree
// is not a WSL UNC path or the path is not POSIX-absolute.
export function mapPosixPathToWslWorktreeUncPath(
  posixPath: string,
  wslWorktreePath: string
): string | null {
  if (!posixPath.startsWith('/') || posixPath.startsWith('//')) {
    return null
  }
  const worktree = parseWslUncPath(wslWorktreePath)
  if (!worktree) {
    return null
  }
  // Why: keep the worktree's own share spelling (\\wsl$ vs \\wsl.localhost) so
  // mapped paths relativize against worktreePath without alias mismatches.
  const share = /^[\\/]{2}wsl\$[\\/]/i.test(wslWorktreePath) ? 'wsl$' : 'wsl.localhost'
  return `\\\\${share}\\${worktree.distro}${posixPath.replaceAll('/', '\\')}`
}

// Why: Windows folds the share (\\wsl$ aliases \\wsl.localhost), the distro, and
// drvfs /mnt/<drive> tails case-insensitively; the rest of the Linux path is not.
export function foldWslUncPathCaseInsensitiveParts(path: string): string | null {
  const parsed = parseWslUncPath(path)
  if (!parsed) {
    return null
  }
  // Why: the drvfs automount is literally lowercase /mnt — a case-variant like
  // /MNT is an ordinary case-sensitive Linux dir and must not be folded.
  const linuxPath = /^\/mnt\/[a-zA-Z](?:\/|$)/.test(parsed.linuxPath)
    ? parsed.linuxPath.toLowerCase()
    : parsed.linuxPath
  return `//wsl.localhost/${parsed.distro.toLowerCase()}${linuxPath === '/' ? '' : linuxPath}`
}
