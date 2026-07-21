/**
 * Preserve a caller's no-colour preference without passing Node the conflicting
 * NO_COLOR + FORCE_COLOR pair that Playwright creates for TTY child processes.
 */
export function normalizeChildColorEnv(source = process.env) {
  const env = { ...source }
  if (Object.hasOwn(env, 'NO_COLOR')) {
    if (!Object.hasOwn(env, 'FORCE_COLOR')) {
      env.FORCE_COLOR = '0'
      env.DEBUG_COLORS ??= '0'
    }
    delete env.NO_COLOR
  }
  return env
}
