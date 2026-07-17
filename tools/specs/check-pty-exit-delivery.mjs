#!/usr/bin/env node
// Model-check the PTY exit-delivery exactly-once protocol with ty. Two runs:
//   1. PtyExitDelivery.cfg   — the real protocol: NeverDoubled + TombstoneSound +
//      EventuallyDelivered must all HOLD (exit 0).
//   2. _nobar variant        — negative control: DROP the consumed-tombstone guard
//      on the buffer-drain / recent-exit replay; NeverDoubled MUST be violated
//      (delivered reaches 2 via a remount + replay after a live delivery). If this
//      ever passes, the model no longer detects the double-delivery class.
// SKIPs (exit 0) when ty is absent (it ships in the local ~/trust stage2 build).
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'

const here = import.meta.dirname
const ty = process.env.TY_BIN ?? join(homedir(), 'trust', 'build', 'host', 'stage2', 'bin', 'ty')
if (!existsSync(ty)) {
  console.log('[exit-spec] SKIP — ty not found (build ~/trust stage2 or set TY_BIN)')
  process.exit(0)
}
const check = (tla, cfg) =>
  spawnSync(ty, ['check', join(here, tla), '--config', join(here, cfg), '--workers', '1'], {
    encoding: 'utf8'
  })
const fair = check('PtyExitDelivery.tla', 'PtyExitDelivery.cfg')
if (fair.status !== 0) {
  console.error(
    `[exit-spec] FAIL — protocol no longer verifies:\n${`${fair.stdout}${fair.stderr}`.slice(-2000)}`
  )
  process.exit(1)
}
const ctrl = check('PtyExitDelivery_nobar.tla', 'PtyExitDelivery_nobar.cfg')
if (ctrl.status === 0 || !/NeverDoubled is violated/u.test(`${ctrl.stdout}${ctrl.stderr}`)) {
  console.error(
    '[exit-spec] FAIL — negative control did not fail: the model no longer detects double-delivery'
  )
  process.exit(1)
}
console.log(
  '[exit-spec] PASS — exactly-once holds with the tombstone; double-delivery detected without it'
)
