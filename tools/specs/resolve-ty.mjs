// ty (the Trust TLA+ model checker): $TY_BIN when set, else `alab-ty` (the
// collision-free name the atpkg-managed Trust toolchain ships it under), else
// `ty` on PATH. Only a binary whose `--help` mentions TLA+ counts: Astral's
// Python type checker is ALSO called `ty` (commonly installed via uv), and
// handing it a .tla would read as a model-check FAIL instead of a missing tool.
// Never a Trust checkout's build/: that tree is the compiler repo's exclusive build output,
// and its ty goes stale (measured 2026-09-28: the build-tree ty exited 101 on
// KeepTailDropBroken, where the PATH ty reported the violation). null means
// absent: callers SKIP (exit 3).
import { spawnSync } from 'node:child_process'

function isTlaModelChecker(bin, env) {
  const probe = spawnSync(bin, ['--help'], { encoding: 'utf8', env })
  return probe.status === 0 && /TLA\+/.test(`${probe.stdout}${probe.stderr}`)
}

export function resolveTy(env = process.env) {
  if (env.TY_BIN) {
    if (isTlaModelChecker(env.TY_BIN, env)) {
      return env.TY_BIN
    }
    console.error(
      `[specs] TY_BIN=${env.TY_BIN} is not a runnable Trust TLA+ model checker (its --help never mentions TLA+)`
    )
    return null
  }
  return ['alab-ty', 'ty'].find((bin) => isTlaModelChecker(bin, env)) ?? null
}
