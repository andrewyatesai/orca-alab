/**
 * Issue #8878 — Remote client resumes live host agent sessions, spawning
 * duplicate TUIs on the same provider session.
 *
 * Regression guard (was a failing-first repro on the handoff branch):
 * `resumeSleepingAgentSessionsForWorktree` must short-circuit when the
 * worktree is runtime-owned — same host-authority predicate as plain
 * terminal auto-create (`isWebRuntimeSessionActive` +
 * `getRuntimeEnvironmentIdForWorktree`). Behavioral matrix lives in
 * `resume-sleeping-agent-session-runtime-owner.test.ts`.
 *
 * Re-run:
 *   pnpm exec vitest run --config config/vitest.config.ts \
 *     src/renderer/src/lib/repro-8878-remote-resume-no-runtime-gate.test.ts
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const resumeSource = readFileSync(join(__dirname, 'resume-sleeping-agent-session.ts'), 'utf8')
const activationSource = readFileSync(join(__dirname, 'worktree-activation.ts'), 'utf8')
const terminalSource = readFileSync(join(__dirname, '../components/Terminal.tsx'), 'utf8')

describe('#8878 remote client resume runtime-ownership gate', () => {
  it('resumeSleepingAgentSessionsForWorktree gates on isWebRuntimeSessionActive', () => {
    expect(resumeSource).toMatch(
      /export function resumeSleepingAgentSessionsForWorktree\s*\(\s*worktreeId:\s*string/
    )
    // Fix: choke-point gate shared by activation, startup, and background wake.
    expect(resumeSource).toMatch(/isWebRuntimeSessionActive/)
    expect(resumeSource).toMatch(/getRuntimeEnvironmentIdForWorktree/)
    expect(resumeSource).toMatch(
      /isWebRuntimeSessionActive\(\s*getRuntimeEnvironmentIdForWorktree\(\s*state,\s*worktreeId\s*\)\s*\)/
    )
  })

  it('worktree activation still invokes resume (gate lives inside the choke point)', () => {
    expect(activationSource).toMatch(/resumeSleepingAgentSessionsForWorktree\(workspaceKey\)/)
    expect(activationSource).toMatch(/resumeSleepingAgentSessionsForWorktree\(worktreeId\)/)
    // Auto-create for runtime-owned worktrees remains gated nearby.
    expect(activationSource).toMatch(
      /isWebRuntimeSessionActive\(getRuntimeEnvironmentIdForWorktree/
    )
  })

  it('Terminal auto-create and startup resume stay consistent via the choke point', () => {
    expect(terminalSource).toMatch(
      /isWebRuntimeSessionActive\(getActiveWorktreeRuntimeEnvironmentId\(activeWorktreeId\)\)/
    )
    expect(terminalSource).toMatch(/resumeSleepingAgentSessionsForWorktree\(activeWorktreeId\)/)
  })
})
