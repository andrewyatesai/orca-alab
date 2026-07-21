import { execFile } from 'node:child_process'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { promisify } from 'node:util'

const SWEEP_TIMEOUT_MS = 30_000
const SWEEP_MAX_BUFFER = 4 * 1024 * 1024
// Why: registering every file of a huge worktree with RestartManager is too
// slow; a breadth-first sample catches the near-root files agents hold open.
const SWEEP_MAX_REGISTERED_FILES = 512
const HANDLE_RELEASE_SETTLE_MS = 500
// Why: RestartManager reports every holder (editors, indexers, AV); only agent
// CLIs Orca launches are safe to force-kill as a last resort (#9045).
const ORPHANED_AGENT_PROCESS_NAMES = ['claude'] as const

export const WORKTREE_SWEEP_ROOT_ENV = 'ORCA_WORKTREE_SWEEP_ROOT'
export const WORKTREE_SWEEP_LOCKED_PATH_ENV = 'ORCA_WORKTREE_SWEEP_LOCKED_PATH'

export type WorktreeHandleHolder = {
  pid: number
  name: string
}

export type WorktreeHandleSweepResult = {
  killedPids: number[]
  holders: WorktreeHandleHolder[]
}

export type WorktreeHandleSweepExecutor = (
  file: string,
  args: string[],
  options: {
    encoding: 'utf8'
    timeout: number
    maxBuffer: number
    windowsHide: boolean
    env: NodeJS.ProcessEnv
  }
) => Promise<{ stdout: string }>

type SweepOptions = {
  lockedPathHint?: string
  execute?: WorktreeHandleSweepExecutor
}

const execFileAsync = promisify(execFile) as unknown as WorktreeHandleSweepExecutor

const EMPTY_SWEEP_RESULT: WorktreeHandleSweepResult = { killedPids: [], holders: [] }

export function buildWorktreeHandleSweepScript(): string {
  const allowlist = ORPHANED_AGENT_PROCESS_NAMES.map((name) => `'${name}'`).join(', ')
  return `$ErrorActionPreference = 'SilentlyContinue'
$root = $env:${WORKTREE_SWEEP_ROOT_ENV}
if ([string]::IsNullOrWhiteSpace($root)) { exit 0 }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace OrcaWorktreeSweep
{
  [StructLayout(LayoutKind.Sequential)]
  public struct RM_UNIQUE_PROCESS
  {
    public int dwProcessId;
    public System.Runtime.InteropServices.ComTypes.FILETIME ProcessStartTime;
  }

  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct RM_PROCESS_INFO
  {
    public RM_UNIQUE_PROCESS Process;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)]
    public string strAppName;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)]
    public string strServiceShortName;
    public int ApplicationType;
    public uint AppStatus;
    public uint TSSessionId;
    [MarshalAs(UnmanagedType.Bool)]
    public bool bRestartable;
  }

  public static class RestartManager
  {
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    public static extern int RmStartSession(out uint pSessionHandle, int dwSessionFlags, string strSessionKey);

    [DllImport("rstrtmgr.dll")]
    public static extern int RmEndSession(uint pSessionHandle);

    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    public static extern int RmRegisterResources(uint pSessionHandle, uint nFiles, string[] rgsFileNames, uint nApplications, RM_UNIQUE_PROCESS[] rgApplications, uint nServices, string[] rgsServiceNames);

    [DllImport("rstrtmgr.dll")]
    public static extern int RmGetList(uint dwSessionHandle, out uint pnProcInfoNeeded, ref uint pnProcInfo, [In, Out] RM_PROCESS_INFO[] rgAffectedApps, ref uint lpdwRebootReasons);
  }
}
'@

$files = New-Object System.Collections.Generic.List[string]
$lockedPath = $env:${WORKTREE_SWEEP_LOCKED_PATH_ENV}
if ($lockedPath -and [System.IO.File]::Exists($lockedPath)) { $files.Add($lockedPath) }
if ([System.IO.Directory]::Exists($root)) {
  $queue = New-Object System.Collections.Generic.Queue[string]
  $queue.Enqueue($root)
  while ($queue.Count -gt 0 -and $files.Count -lt ${SWEEP_MAX_REGISTERED_FILES}) {
    $directory = $queue.Dequeue()
    try {
      foreach ($entry in [System.IO.Directory]::EnumerateFileSystemEntries($directory)) {
        if ($files.Count -ge ${SWEEP_MAX_REGISTERED_FILES}) { break }
        if ([System.IO.Directory]::Exists($entry)) { $queue.Enqueue($entry) } else { $files.Add($entry) }
      }
    } catch { }
  }
} elseif ([System.IO.File]::Exists($root)) {
  $files.Add($root)
}
if ($files.Count -eq 0) { exit 0 }

$session = [uint32]0
if ([OrcaWorktreeSweep.RestartManager]::RmStartSession([ref]$session, 0, [Guid]::NewGuid().ToString()) -ne 0) { exit 0 }
try {
  if ([OrcaWorktreeSweep.RestartManager]::RmRegisterResources($session, [uint32]$files.Count, $files.ToArray(), 0, $null, 0, $null) -ne 0) { exit 0 }
  $needed = [uint32]0
  $count = [uint32]0
  $reasons = [uint32]0
  $result = [OrcaWorktreeSweep.RestartManager]::RmGetList($session, [ref]$needed, [ref]$count, $null, [ref]$reasons)
  if ($result -eq 0 -or $needed -eq 0) { exit 0 }
  # 234 = ERROR_MORE_DATA: the sizing call reports how many holders to allocate.
  if ($result -ne 234) { exit 0 }
  $holders = New-Object -TypeName 'OrcaWorktreeSweep.RM_PROCESS_INFO[]' -ArgumentList ([int]$needed)
  $count = [uint32]$holders.Length
  if ([OrcaWorktreeSweep.RestartManager]::RmGetList($session, [ref]$needed, [ref]$count, $holders, [ref]$reasons) -ne 0) { exit 0 }
  $tab = [string][char]9
  for ($index = 0; $index -lt [int]$count; $index++) {
    $holderPid = $holders[$index].Process.dwProcessId
    $holderProcess = Get-Process -Id $holderPid -ErrorAction SilentlyContinue
    if ($null -eq $holderProcess) { continue }
    $holderName = $holderProcess.ProcessName
    Write-Output ('HOLDER' + $tab + $holderPid + $tab + $holderName)
    if (@(${allowlist}) -notcontains $holderName.ToLowerInvariant()) { continue }
    $startLow = [int64]$holders[$index].Process.ProcessStartTime.dwLowDateTime
    if ($startLow -lt 0) { $startLow += 4294967296 }
    $startFileTime = ([int64]$holders[$index].Process.ProcessStartTime.dwHighDateTime -shl 32) + $startLow
    $currentStart = $null
    try { $currentStart = $holderProcess.StartTime } catch { }
    # Why: a recycled PID must not inherit the kill; RestartManager pins the
    # holder by start time, so compare it against the live process.
    if ($null -ne $currentStart -and $startFileTime -gt 0) {
      $reportedStart = [DateTime]::FromFileTime($startFileTime)
      if ([Math]::Abs(($currentStart - $reportedStart).TotalSeconds) -gt 5) { continue }
    }
    Stop-Process -Id $holderPid -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 200
    if ($null -eq (Get-Process -Id $holderPid -ErrorAction SilentlyContinue)) {
      Write-Output ('KILLED' + $tab + $holderPid + $tab + $holderName)
    }
  }
} finally {
  [void][OrcaWorktreeSweep.RestartManager]::RmEndSession($session)
}
exit 0
`
}

export function parseWorktreeHandleSweepOutput(stdout: string): WorktreeHandleSweepResult {
  const killedPids: number[] = []
  const holders: WorktreeHandleHolder[] = []
  for (const raw of stdout.split(/\r?\n/)) {
    const fields = raw.split('\t')
    if (fields.length < 3) {
      continue
    }
    const pid = Number.parseInt(fields[1], 10)
    if (!Number.isSafeInteger(pid) || pid <= 0) {
      continue
    }
    if (fields[0] === 'HOLDER') {
      holders.push({ pid, name: fields[2] })
    } else if (fields[0] === 'KILLED' && !killedPids.includes(pid)) {
      killedPids.push(pid)
    }
  }
  return { killedPids, holders }
}

function windowsPowerShellPath(): string {
  const systemRoot = process.env.SystemRoot || process.env.windir
  // Why: the delete path must not depend on a user-mutated PATH; resolve the
  // stock Windows PowerShell absolutely and only fall back to PATH lookup.
  return systemRoot
    ? join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe')
    : 'powershell.exe'
}

export async function sweepOrphanedWorktreeHandleOwners(
  worktreePath: string,
  options: SweepOptions = {}
): Promise<WorktreeHandleSweepResult> {
  if (process.platform !== 'win32') {
    return EMPTY_SWEEP_RESULT
  }
  const execute = options.execute ?? execFileAsync
  try {
    const { stdout } = await execute(
      windowsPowerShellPath(),
      ['-NoProfile', '-NonInteractive', '-Command', buildWorktreeHandleSweepScript()],
      {
        encoding: 'utf8',
        timeout: SWEEP_TIMEOUT_MS,
        maxBuffer: SWEEP_MAX_BUFFER,
        windowsHide: true,
        env: {
          ...process.env,
          [WORKTREE_SWEEP_ROOT_ENV]: worktreePath,
          ...(options.lockedPathHint
            ? { [WORKTREE_SWEEP_LOCKED_PATH_ENV]: options.lockedPathHint }
            : {})
        }
      }
    )
    const result = parseWorktreeHandleSweepOutput(stdout)
    if (result.holders.length > 0) {
      console.warn(
        `[worktrees] RestartManager holders for ${worktreePath}:`,
        result.holders.map((holder) => `${holder.name}(${holder.pid})`).join(', '),
        result.killedPids.length > 0 ? `killed: ${result.killedPids.join(', ')}` : 'killed: none'
      )
    }
    return result
  } catch (error) {
    console.warn(`[worktrees] RestartManager sweep failed for ${worktreePath}`, error)
    return EMPTY_SWEEP_RESULT
  }
}

export function lockedPathFromRemovalError(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null) {
    return undefined
  }
  const path = 'path' in error ? (error as NodeJS.ErrnoException).path : undefined
  return typeof path === 'string' && path.length > 0 ? path : undefined
}

type SweepRetryArgs<T> = {
  worktreePath: string
  wslDistro?: string
  lockedPathHint?: string
  retry: () => Promise<T>
  execute?: WorktreeHandleSweepExecutor
  settleMs?: number
}

/**
 * Last resort for Windows worktree deletion blocked by orphaned agent-CLI
 * handles (#9045): sweep holders via RestartManager, then retry removal once.
 * Returns undefined when the sweep does not apply or freed nothing.
 */
export async function retryWorktreeRemovalAfterHandleSweep<T>(
  args: SweepRetryArgs<T>
): Promise<{ result: T } | undefined> {
  // Why: WSL-owned worktrees are deleted inside the distro where Win32 handle
  // locks (and RestartManager) do not apply.
  if (process.platform !== 'win32' || args.wslDistro?.trim()) {
    return undefined
  }
  const sweep = await sweepOrphanedWorktreeHandleOwners(args.worktreePath, {
    ...(args.lockedPathHint ? { lockedPathHint: args.lockedPathHint } : {}),
    ...(args.execute ? { execute: args.execute } : {})
  })
  if (sweep.killedPids.length === 0) {
    return undefined
  }
  await delay(args.settleMs ?? HANDLE_RELEASE_SETTLE_MS)
  try {
    return { result: await args.retry() }
  } catch (error) {
    console.warn(
      `[worktrees] Removal of ${args.worktreePath} still failed after terminating orphaned agent processes`,
      error
    )
    return undefined
  }
}
