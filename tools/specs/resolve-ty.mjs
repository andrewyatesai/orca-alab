// ty (the Trust TLA+ model checker): $TY_BIN when set, else `ty` on PATH — the
// atpkg-managed Trust toolchain ships it. Never ~/trust/build: that tree is the
// compiler repo's exclusive build output, and its ty goes stale (measured
// 2026-09-28: the build-tree ty exited 101 on KeepTailDropBroken, where the
// PATH ty reported the violation). null means absent: callers SKIP (exit 3).
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'

export function resolveTy(env = process.env) {
  if (env.TY_BIN) {
    return existsSync(env.TY_BIN) ? env.TY_BIN : null
  }
  const probe = spawnSync('ty', ['--version'], { encoding: 'utf8' })
  return probe.status === 0 ? 'ty' : null
}
