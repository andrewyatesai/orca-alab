#!/usr/bin/env node
// Model-check the background-session keep-tail DROP protocol with ty. Two runs:
//   1. KeepTailDrop.cfg        — the real protocol: TypeOK + BoundedMemory +
//      NeverBelowFloor + EventualDrain must all HOLD, with no deadlock outside the
//      declared Drained terminal (exit 0).
//   2. KeepTailDropBroken.tla  — negative control: DROP the per-session trim on
//      Enqueue (grow without thinning to the keep-tail); BoundedMemory MUST be
//      violated (the queue climbs past the drop cap). If this ever passes, the
//      model no longer detects the unbounded-memory class and the spec's safety no
//      longer depends on the drop rule.
// SKIPs (exit 0) when ty is absent (it ships in the local ~/trust stage2 build).
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'

const here = import.meta.dirname
const ty = process.env.TY_BIN ?? join(homedir(), 'trust', 'build', 'host', 'stage2', 'bin', 'ty')
if (!existsSync(ty)) {
  console.log('[keep-tail-spec] SKIP — ty not found (build ~/trust stage2 or set TY_BIN)')
  process.exit(0)
}
const check = (tla, cfg) =>
  spawnSync(ty, ['check', join(here, tla), '--config', join(here, cfg), '--workers', '1'], {
    encoding: 'utf8'
  })
const fair = check('KeepTailDrop.tla', 'KeepTailDrop.cfg')
if (fair.status !== 0) {
  console.error(
    `[keep-tail-spec] FAIL — protocol no longer verifies:\n${`${fair.stdout}${fair.stderr}`.slice(-2000)}`
  )
  process.exit(1)
}
const ctrl = check('KeepTailDropBroken.tla', 'KeepTailDropBroken.cfg')
if (
  ctrl.status === 0 ||
  !/Invariant BoundedMemory is violated/u.test(`${ctrl.stdout}${ctrl.stderr}`)
) {
  console.error(
    '[keep-tail-spec] FAIL — negative control did not fail: the model no longer detects unbounded memory'
  )
  process.exit(1)
}
console.log(
  '[keep-tail-spec] PASS — bounded memory + floor + eventual drain hold with the drop; unbounded memory detected without it'
)
