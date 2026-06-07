// TS dispatch for the worktree-ownership parity module: maps the shared vector
// function names to the real `src/shared/worktree-ownership.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  areRuntimePathsEqual,
  buildKnownOrcaWorkspaceLayouts,
  classifyWorktreeOwnership,
  effectiveExternalWorktreeVisibility,
  isLegacyRepoForExternalWorktreeVisibility,
  matchesStrongOrcaCreatePath,
  shouldShowWorktree,
  toDetectedWorktree
} from '../../../src/shared/worktree-ownership'
import type { OrcaWorkspaceLayout, Repo } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isLegacyRepoForExternalWorktreeVisibility':
      return isLegacyRepoForExternalWorktreeVisibility(input as Repo)
    case 'effectiveExternalWorktreeVisibility': {
      const { repo, isLegacyRepoForVisibility } = input as {
        repo: Pick<Repo, 'externalWorktreeVisibility'>
        isLegacyRepoForVisibility: boolean
      }
      return effectiveExternalWorktreeVisibility(repo, isLegacyRepoForVisibility)
    }
    case 'buildKnownOrcaWorkspaceLayouts': {
      const { settings, repo } = input as {
        settings: Parameters<typeof buildKnownOrcaWorkspaceLayouts>[0]
        repo?: Parameters<typeof buildKnownOrcaWorkspaceLayouts>[1]
      }
      return buildKnownOrcaWorkspaceLayouts(settings, repo)
    }
    case 'classifyWorktreeOwnership':
      return classifyWorktreeOwnership(input as Parameters<typeof classifyWorktreeOwnership>[0])
    case 'toDetectedWorktree':
      // Output spreads the input worktree, so vectors pass only { path, isMainWorktree }
      // to match the lean Rust DetectedWorktree shape.
      return toDetectedWorktree(input as Parameters<typeof toDetectedWorktree>[0])
    case 'shouldShowWorktree':
      return shouldShowWorktree(input as Parameters<typeof shouldShowWorktree>[0])
    case 'areRuntimePathsEqual': {
      const { leftPath, rightPath } = input as { leftPath: string; rightPath: string }
      return areRuntimePathsEqual(leftPath, rightPath)
    }
    case 'matchesStrongOrcaCreatePath': {
      const { worktreePath, knownOrcaLayouts, repo } = input as {
        worktreePath: string
        knownOrcaLayouts: OrcaWorkspaceLayout[]
        repo: Pick<Repo, 'path'>
      }
      return matchesStrongOrcaCreatePath(worktreePath, knownOrcaLayouts, repo)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
