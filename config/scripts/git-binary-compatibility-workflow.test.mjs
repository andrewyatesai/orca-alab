import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const projectDir = resolve(import.meta.dirname, '../..')
// This contract asserts upstream's pr.yml Git-binary-compatibility gate. The
// fork runs no hosted CI (the local gauntlet is the gate), so pr.yml is dropped
// — gate on the asserted file itself so the contract runs wherever pr.yml ships
// and skips cleanly where it doesn't, without weakening the real-binary checks.
const HAS_CI_PR_WORKFLOW = existsSync(join(projectDir, '.github/workflows/pr.yml'))

describe('Git binary compatibility PR gate', () => {
  it.skipIf(!HAS_CI_PR_WORKFLOW)(
    'runs the real-binary contract at each compatibility boundary',
    () => {
      const workflow = parse(
        readFileSync(join(projectDir, '.github/workflows/pr.yml'), 'utf8')
      )
      const step = workflow.jobs.verify.steps.find(
        (candidate) => candidate.name === 'Verify Git binary compatibility matrix'
      )

      expect(step?.run).toContain('git-2.25.5.tar.gz')
      expect(step?.run).toContain(
        '41662c52fc16fec4963bfc41075e71f8ead6b5e386797eb6f9a1111ff95a8ddf'
      )
      expect(step?.run).toContain('ORCA_GIT_COMPAT_BINARY="$source/git"')
      expect(step?.run).toContain('alpine/git:edge-2.38.1|2.38.1')
      expect(step?.run).toContain('alpine/git:v2.49.1|2.49.1')
      expect(step?.run).toContain('ORCA_GIT_COMPAT_IMAGE="$image"')
      expect(step?.run).toContain('src/shared/git-binary-compatibility.test.ts')
    }
  )
})
