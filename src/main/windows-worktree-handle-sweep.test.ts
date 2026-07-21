import { describe, expect, it, vi } from 'vitest'
import {
  buildWorktreeHandleSweepScript,
  lockedPathFromRemovalError,
  parseWorktreeHandleSweepOutput,
  retryWorktreeRemovalAfterHandleSweep,
  sweepOrphanedWorktreeHandleOwners,
  WORKTREE_SWEEP_LOCKED_PATH_ENV,
  WORKTREE_SWEEP_ROOT_ENV,
  type WorktreeHandleSweepExecutor
} from './windows-worktree-handle-sweep'

async function withPlatform<T>(platform: NodeJS.Platform, fn: () => Promise<T>): Promise<T> {
  const original = process.platform
  Object.defineProperty(process, 'platform', { configurable: true, value: platform })
  try {
    return await fn()
  } finally {
    Object.defineProperty(process, 'platform', { configurable: true, value: original })
  }
}

function executorReturning(stdout: string): WorktreeHandleSweepExecutor & ReturnType<typeof vi.fn> {
  return vi.fn().mockResolvedValue({ stdout })
}

describe('buildWorktreeHandleSweepScript', () => {
  const script = buildWorktreeHandleSweepScript()

  it('drives the RestartManager session lifecycle', () => {
    for (const call of ['RmStartSession', 'RmRegisterResources', 'RmGetList', 'RmEndSession']) {
      expect(script).toContain(call)
    }
    // Why: RmEndSession must run even when enumeration throws mid-sweep.
    expect(script.indexOf('finally')).toBeGreaterThan(script.indexOf('RmRegisterResources'))
  })

  it('reads its inputs from env vars instead of interpolated paths', () => {
    expect(script).toContain(`$env:${WORKTREE_SWEEP_ROOT_ENV}`)
    expect(script).toContain(`$env:${WORKTREE_SWEEP_LOCKED_PATH_ENV}`)
    // Why: a stray TS interpolation would serialize as 'undefined' in the script.
    expect(script).not.toContain('undefined')
  })

  it('only force-kills allowlisted agent CLI holders', () => {
    expect(script).toContain("@('claude') -notcontains")
    expect(script).toContain('Stop-Process -Id $holderPid -Force')
    // Why: $pid is a PowerShell automatic variable (the shell's own PID);
    // shadowing it would kill or report the sweeping powershell.exe itself.
    expect(script).not.toMatch(/\$pid\b/i)
  })

  it('guards against PID recycling before killing', () => {
    expect(script).toContain('ProcessStartTime')
    expect(script).toContain('FromFileTime')
  })

  it('bounds the registered file sample', () => {
    expect(script).toContain('$files.Count -lt 512')
    expect(script).toContain('$files.Count -ge 512')
  })
})

describe('parseWorktreeHandleSweepOutput', () => {
  it('parses HOLDER and KILLED lines with CRLF endings', () => {
    const result = parseWorktreeHandleSweepOutput(
      'HOLDER\t4242\tclaude\r\nHOLDER\t99\tCode\r\nKILLED\t4242\tclaude\r\n'
    )
    expect(result.holders).toEqual([
      { pid: 4242, name: 'claude' },
      { pid: 99, name: 'Code' }
    ])
    expect(result.killedPids).toEqual([4242])
  })

  it('ignores noise lines and malformed pids', () => {
    const result = parseWorktreeHandleSweepOutput(
      'Add-Type warning text\nKILLED\tnot-a-pid\tclaude\nKILLED\t-4\tclaude\nKILLED\t7\tclaude\nKILLED\t7\tclaude\n'
    )
    expect(result.holders).toEqual([])
    expect(result.killedPids).toEqual([7])
  })
})

describe('sweepOrphanedWorktreeHandleOwners', () => {
  it('is a no-op off Windows', async () => {
    const execute = executorReturning('KILLED\t1\tclaude\n')
    await withPlatform('darwin', async () => {
      const result = await sweepOrphanedWorktreeHandleOwners('C:\\wt', { execute })
      expect(result).toEqual({ killedPids: [], holders: [] })
    })
    expect(execute).not.toHaveBeenCalled()
  })

  it('passes the worktree root and locked-path hint through env vars', async () => {
    const execute = executorReturning('HOLDER\t4242\tclaude\nKILLED\t4242\tclaude\n')
    const result = await withPlatform('win32', () =>
      sweepOrphanedWorktreeHandleOwners('C:\\repo\\wt', {
        execute,
        lockedPathHint: 'C:\\repo\\wt\\held.log'
      })
    )
    expect(result.killedPids).toEqual([4242])
    const [file, args, options] = execute.mock.calls[0]
    expect(String(file).toLowerCase()).toContain('powershell.exe')
    expect(args).toContain('-NoProfile')
    expect(options.windowsHide).toBe(true)
    expect(options.env[WORKTREE_SWEEP_ROOT_ENV]).toBe('C:\\repo\\wt')
    expect(options.env[WORKTREE_SWEEP_LOCKED_PATH_ENV]).toBe('C:\\repo\\wt\\held.log')
  })

  it('degrades to an empty result when PowerShell fails', async () => {
    const execute = vi.fn().mockRejectedValue(new Error('spawn failed'))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const result = await withPlatform('win32', () =>
        sweepOrphanedWorktreeHandleOwners('C:\\wt', { execute })
      )
      expect(result).toEqual({ killedPids: [], holders: [] })
    } finally {
      warn.mockRestore()
    }
  })
})

describe('retryWorktreeRemovalAfterHandleSweep', () => {
  it('does not apply off Windows or inside WSL distros', async () => {
    const retry = vi.fn()
    const execute = executorReturning('KILLED\t1\tclaude\n')
    await withPlatform('linux', async () => {
      expect(
        await retryWorktreeRemovalAfterHandleSweep({ worktreePath: '/wt', retry, execute })
      ).toBeUndefined()
    })
    await withPlatform('win32', async () => {
      expect(
        await retryWorktreeRemovalAfterHandleSweep({
          worktreePath: 'C:\\wt',
          wslDistro: 'Ubuntu',
          retry,
          execute
        })
      ).toBeUndefined()
    })
    expect(execute).not.toHaveBeenCalled()
    expect(retry).not.toHaveBeenCalled()
  })

  it('retries removal once after the sweep frees a holder', async () => {
    const retry = vi.fn().mockResolvedValue({ preservedBranch: undefined })
    const execute = executorReturning('HOLDER\t4242\tclaude\nKILLED\t4242\tclaude\n')
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const outcome = await withPlatform('win32', () =>
        retryWorktreeRemovalAfterHandleSweep({
          worktreePath: 'C:\\wt',
          retry,
          execute,
          settleMs: 0
        })
      )
      expect(outcome).toEqual({ result: { preservedBranch: undefined } })
      expect(retry).toHaveBeenCalledTimes(1)
    } finally {
      warn.mockRestore()
    }
  })

  it('skips the retry when the sweep killed nothing', async () => {
    const retry = vi.fn()
    const execute = executorReturning('HOLDER\t99\tCode\n')
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const outcome = await withPlatform('win32', () =>
        retryWorktreeRemovalAfterHandleSweep({
          worktreePath: 'C:\\wt',
          retry,
          execute,
          settleMs: 0
        })
      )
      expect(outcome).toBeUndefined()
      expect(retry).not.toHaveBeenCalled()
    } finally {
      warn.mockRestore()
    }
  })

  it('surfaces undefined when the post-sweep retry still fails', async () => {
    const retry = vi.fn().mockRejectedValue(new Error('still locked'))
    const execute = executorReturning('KILLED\t4242\tclaude\n')
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const outcome = await withPlatform('win32', () =>
        retryWorktreeRemovalAfterHandleSweep({
          worktreePath: 'C:\\wt',
          retry,
          execute,
          settleMs: 0
        })
      )
      expect(outcome).toBeUndefined()
      expect(retry).toHaveBeenCalledTimes(1)
    } finally {
      warn.mockRestore()
    }
  })
})

describe('lockedPathFromRemovalError', () => {
  it('extracts the failing path from Node fs errors', () => {
    expect(
      lockedPathFromRemovalError(
        Object.assign(new Error('EBUSY'), { code: 'EBUSY', path: 'C:\\wt\\held.log' })
      )
    ).toBe('C:\\wt\\held.log')
    expect(lockedPathFromRemovalError(new Error('plain'))).toBeUndefined()
    expect(lockedPathFromRemovalError(null)).toBeUndefined()
  })
})
