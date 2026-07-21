#!/usr/bin/env node
// Model-check the keystroke -> visible-echo pipeline (mosh-style predictive echo)
// with ty (the Trust TLA+ toolchain). Two runs, both load-bearing:
//   1. EchoLiveness.cfg (ARMED=TRUE)  — the armed predictor: TypeOK + Liveness +
//      BoundedFeltLatency ([](typed /\ clock>=GHOST_MAX => Visible)) must all HOLD
//      (exit 0). This is the proof that felt latency stays within the local-predict
//      bound INDEPENDENT of the PTY/SSH round-trip.
//   2. EchoLiveness.inert.cfg (ARMED=FALSE) — negative control = the EXACT
//      inert-worker-facade regression (predict_* went missing, the controller's
//      capability probe saw engine=null, ShowGhost never fires). BoundedFeltLatency
//      MUST be violated (felt latency degrades to the full round-trip up to
//      RTT_MAX). If this run ever passes, the model has rotted and can no longer
//      detect the silent typing-lag class.
// The gate FAILS if the positive run doesn't hold OR the negative control passes.
// SKIPs (exit 0) when ty is absent — it ships in the local ~/trust stage2 build.
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { homedir } from 'node:os'

const here = import.meta.dirname
const ty = process.env.TY_BIN ?? join(homedir(), 'trust', 'build', 'host', 'stage2', 'bin', 'ty')
if (!existsSync(ty)) {
  console.log('[echo-spec] SKIP — ty not found (build ~/trust stage2 or set TY_BIN)')
  process.exit(0)
}
const check = (cfg) =>
  spawnSync(
    ty,
    ['check', join(here, 'EchoLiveness.tla'), '--config', join(here, cfg), '--workers', '1'],
    {
      encoding: 'utf8'
    }
  )
const armed = check('EchoLiveness.cfg')
if (armed.status !== 0) {
  console.error(
    `[echo-spec] FAIL — armed spec no longer verifies:\n${`${armed.stdout}${armed.stderr}`.slice(-2000)}`
  )
  process.exit(1)
}
const inert = check('EchoLiveness.inert.cfg')
if (
  inert.status === 0 ||
  !/BoundedFeltLatency is violated/u.test(`${inert.stdout}${inert.stderr}`)
) {
  console.error(
    '[echo-spec] FAIL — negative control did not fail: the model no longer detects the inert-predictor typing-lag class'
  )
  process.exit(1)
}
console.log(
  '[echo-spec] PASS — bounded felt latency holds when armed; unbounded typing lag detected when inert'
)
