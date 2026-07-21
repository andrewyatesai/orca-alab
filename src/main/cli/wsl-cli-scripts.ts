const MANAGED_MARKER = '# Orca managed WSL CLI launcher'
const BRIDGE_MANAGED_MARKER = '# Orca managed WSL CLI PowerShell bridge'

export function buildWslLauncher(
  windowsLauncherPath: string,
  bridgePath = '${XDG_DATA_HOME:-$HOME/.local/share}/orca/orca-wsl-bridge.ps1'
): string {
  const encodedTarget = Buffer.from(windowsLauncherPath, 'utf8').toString('base64')
  return `#!/usr/bin/env bash
set -euo pipefail
${MANAGED_MARKER}
# ORCA_WIN_LAUNCHER_B64=${encodedTarget}
ORCA_WIN_LAUNCHER=${quoteShell(windowsLauncherPath)}
ORCA_BRIDGE_PS1=${quoteShell(bridgePath)}
if command -v powershell.exe >/dev/null 2>&1; then
  ORCA_POWERSHELL=powershell.exe
elif [ -x /mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe ]; then
  ORCA_POWERSHELL=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe
else
  echo "Orca WSL CLI requires Windows interop and could not find powershell.exe." >&2
  exit 1
fi
# Why: a shell can outlive a deleted worktree; keep explicit CLI selectors and
# help usable, and repair cwd before any WSL interop tool tries to resolve it.
ORCA_WSL_CWD=$(pwd -P 2>/dev/null) || {
  ORCA_WSL_CWD=/
  cd /
}
ORCA_BRIDGE_PS1_WIN=$(wslpath -w "$ORCA_BRIDGE_PS1")
ORCA_WSL_CWD_WIN=$(wslpath -w "$ORCA_WSL_CWD")
exec "$ORCA_POWERSHELL" -NoProfile -ExecutionPolicy Bypass -File "$ORCA_BRIDGE_PS1_WIN" "$ORCA_WIN_LAUNCHER" -WslCwd "$ORCA_WSL_CWD_WIN" "$@"
`
}

export function buildWslBridgeScript(): string {
  return `${BRIDGE_MANAGED_MARKER}
[CmdletBinding(PositionalBinding=$false)]
param(
  [Parameter(Mandatory=$true, Position=0)]
  [string]$OrcaLauncher,

  [string]$WslCwd,

  [Parameter(ValueFromRemainingArguments=$true)]
  [string[]]$ForwardArgs
)

function Repair-OrcaDuplicateEnvNames {
  # Why: WSL interop can hand the Windows child both 'PATH' and 'Path'; .NET
  # children crash building a case-insensitive env dictionary from that block (#9498).
  try {
    $null = (New-Object System.Diagnostics.ProcessStartInfo).EnvironmentVariables
    return
  } catch {
  }
  try {
    $definition = @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern System.IntPtr GetEnvironmentStringsW();
[System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern bool FreeEnvironmentStringsW(System.IntPtr block);
'@
    $native = Add-Type -MemberDefinition $definition -Name 'NativeEnvironmentBlock' -Namespace 'OrcaWslBridge' -PassThru
    $block = $native::GetEnvironmentStringsW()
    if ($block -eq [System.IntPtr]::Zero) { return }
    $names = @{}
    try {
      $cursor = $block
      while ($true) {
        $entry = [System.Runtime.InteropServices.Marshal]::PtrToStringUni($cursor)
        if ([string]::IsNullOrEmpty($entry)) { break }
        $cursor = [System.IntPtr]($cursor.ToInt64() + (($entry.Length + 1) * 2))
        $separator = $entry.IndexOf('=', 1)
        if ($separator -lt 1) { continue }
        $name = $entry.Substring(0, $separator)
        $upper = $name.ToUpperInvariant()
        if (-not $names.ContainsKey($upper)) { $names[$upper] = @() }
        $names[$upper] += ,@($name, $entry.Substring($separator + 1))
      }
    } finally {
      $null = $native::FreeEnvironmentStringsW($block)
    }
    foreach ($upper in @($names.Keys)) {
      $variants = $names[$upper]
      if ($variants.Count -le 1) { continue }
      # Why: keep the Windows-conventional spelling (e.g. 'Path') so Windows
      # children still resolve system binaries; the WSL-forwarded twin loses.
      $keep = $variants[0]
      foreach ($variant in $variants) {
        if ($variant[0] -cne $variant[0].ToUpperInvariant()) { $keep = $variant; break }
      }
      $attempts = 0
      while ($attempts -lt 8 -and $null -ne [System.Environment]::GetEnvironmentVariable($keep[0])) {
        [System.Environment]::SetEnvironmentVariable($keep[0], $null)
        $attempts = $attempts + 1
      }
      [System.Environment]::SetEnvironmentVariable($keep[0], $keep[1])
    }
  } catch {
  }
}
Repair-OrcaDuplicateEnvNames

$exitCode = 0
try {
  if ([string]::IsNullOrEmpty($WslCwd)) {
    Remove-Item Env:ORCA_CLI_CWD -ErrorAction SilentlyContinue
  } else {
    $env:ORCA_CLI_CWD = $WslCwd
  }
  Push-Location -LiteralPath (Split-Path -Parent $OrcaLauncher)
  & $OrcaLauncher @ForwardArgs
  if ($null -eq $LASTEXITCODE) {
    if (-not $?) {
      $exitCode = 1
    } else {
      $exitCode = 0
    }
  } else {
    $exitCode = $LASTEXITCODE
  }
} catch {
  Write-Error $_
  $exitCode = 1
}
exit $exitCode
`
}

export function getBridgePathFromCommandPath(commandPath: string): string {
  // Why: both the current Linux command and the legacy pre-rename command
  // share one WSL bridge under ~/.local/share/orca.
  return `${commandPath.replace(/\/\.local\/bin\/(?:orca|orca-ide)$/, '/.local/share/orca')}/orca-wsl-bridge.ps1`
}

export function buildSafeReplaceGuard(path: string, managedMarker: string): string {
  const quotedPath = quoteShell(path)
  const quotedMarker = quoteShell(managedMarker)
  return [
    `if [ -L ${quotedPath} ]; then`,
    '  echo "__ORCA_CONFLICT__"',
    '  exit 23',
    `elif [ -e ${quotedPath} ] && { [ ! -f ${quotedPath} ] || ! grep -Fq ${quotedMarker} ${quotedPath}; }; then`,
    '  echo "__ORCA_CONFLICT__"',
    '  exit 23',
    'fi'
  ].join('\n')
}

export function buildRegistrationLockPrelude(commandPath: string): string {
  const lockDir = getPosixDirname(getBridgePathFromCommandPath(commandPath))
  // Why: the per-distro queue only serializes one Orca process; flock covers
  // a second install (e.g. stable + nightly) mutating the same distro files.
  return [
    `if command -v flock >/dev/null 2>&1 && mkdir -p ${quoteShell(lockDir)} 2>/dev/null; then`,
    `  exec 9>${quoteShell(`${lockDir}/.orca-wsl-cli.lock`)}`,
    '  flock -x -w 30 9',
    'fi'
  ].join('\n')
}

export function buildManagedLegacyRemoveCommand(quotedLegacyCommandPath: string): string {
  // Why: remove only the Orca-managed pre-rename wrapper; user-owned `orca`
  // commands and symlinks must survive.
  return `if [ ! -L ${quotedLegacyCommandPath} ] && [ -f ${quotedLegacyCommandPath} ] && grep -Fq ${quoteShell(MANAGED_MARKER)} ${quotedLegacyCommandPath}; then rm -f ${quotedLegacyCommandPath}; fi`
}

export function buildSafeRemoveCommand(commandPath: string, legacyCommandPath?: string): string {
  const bridgePath = getBridgePathFromCommandPath(commandPath)
  return [
    'set -euo pipefail',
    buildRegistrationLockPrelude(commandPath),
    buildSafeReplaceGuard(commandPath, MANAGED_MARKER),
    buildSafeReplaceGuard(bridgePath, BRIDGE_MANAGED_MARKER),
    `rm -f ${quoteShell(commandPath)} ${quoteShell(bridgePath)}`,
    // Why: leaving a managed legacy `orca` behind lets startup reconciliation
    // re-adopt it as opt-in proof and silently undo this removal.
    ...(legacyCommandPath ? [buildManagedLegacyRemoveCommand(quoteShell(legacyCommandPath))] : [])
  ].join('\n')
}

export function parseManagedLauncherTarget(content: string): string | null {
  const encoded = content.match(/^# ORCA_WIN_LAUNCHER_B64=([A-Za-z0-9+/=]+)$/m)?.[1]
  if (encoded) {
    try {
      return Buffer.from(encoded, 'base64').toString('utf8')
    } catch {
      return null
    }
  }

  const legacyTarget = content.match(/^ORCA_WIN_LAUNCHER='((?:[^']|'"'"')*)'$/m)?.[1]
  return legacyTarget ? legacyTarget.replaceAll(`'"'"'`, "'") : null
}

export function getPosixDirname(path: string): string {
  return path.slice(0, path.lastIndexOf('/')) || '/'
}

export function getWslLauncherMarker(): string {
  return MANAGED_MARKER
}

export function getWslBridgeMarker(): string {
  return BRIDGE_MANAGED_MARKER
}

export function quoteShell(value: string): string {
  return `'${value.replaceAll("'", `'"'"'`)}'`
}
