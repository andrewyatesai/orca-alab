#!/usr/bin/env node
// Model-check the PTY producer flow-control protocol spec with ty (the Trust
// TLA+ toolchain). Two runs, both load-bearing:
//   1. PtyFlowControl.cfg        — full protocol w/ failsafe fairness: TypeOK,
//      PauseImpliesArmed, NoWedge, QuiescentResume must all HOLD (exit 0).
//   2. .nofailsafe.cfg           — negative control: without the failsafe timer
//      NoWedge must FAIL (a lost resume wedges the daemon). If this run ever
//      passes, the model has rotted and can no longer detect the wedge class.
// SKIPs (exit 0) when ty is absent — it ships in the local ~/trust stage2 build.
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'

const here = import.meta.dirname
const ty = process.env.TY_BIN ?? join(homedir(), 'trust', 'build', 'host', 'stage2', 'bin', 'ty')
if (!existsSync(ty)) {
  console.log('[flow-spec] SKIP — ty not found (build ~/trust stage2 or set TY_BIN)')
  process.exit(0)
}
const run = (cfg) =>
  spawnSync(
    ty,
    ['check', join(here, 'PtyFlowControl.tla'), '--config', join(here, cfg), '--workers', '1'],
    {
      encoding: 'utf8'
    }
  )
const fair = run('PtyFlowControl.cfg')
if (fair.status !== 0) {
  console.error(
    `[flow-spec] FAIL — fair spec no longer verifies:\n${`${fair.stdout}${fair.stderr}`.slice(-2000)}`
  )
  process.exit(1)
}
const noFailsafe = run('PtyFlowControl.nofailsafe.cfg')
const out = `${noFailsafe.stdout}${noFailsafe.stderr}`
if (noFailsafe.status === 0 || !/[Ll]iveness violation/u.test(out)) {
  console.error(
    '[flow-spec] FAIL — negative control did not fail: the model no longer detects the wedge class'
  )
  process.exit(1)
}
console.log(
  '[flow-spec] PASS — NoWedge+QuiescentResume hold with failsafe; wedge detected without it'
)
