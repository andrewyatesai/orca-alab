// Why: ORCA_E2E_FORCE_DPR forces Chromium's device scale factor so a spec can
// exercise the Retina (devicePixelRatio=2) render path the rest of the headless
// suite never hits (it runs at dpr=1). Strictly additive and OFF by default: when
// the env value is unset the launch args are byte-for-byte what they were before.
//
// The value is read from the per-test launch env (the orca-app fixture's launchEnv,
// falling back to process.env) — NOT from a module-scope process.env mutation — so
// forcing dpr stays scoped to the one spec that opts in and never leaks to other
// specs sharing the same Playwright worker.
function getForcedDeviceScaleFactorArgs(env: NodeJS.ProcessEnv): string[] {
  const raw = env.ORCA_E2E_FORCE_DPR
  if (raw === undefined || raw === '') {
    return []
  }
  const value = Number(raw)
  if (!Number.isFinite(value) || value <= 0) {
    console.warn(
      `[orca-e2e] ORCA_E2E_FORCE_DPR="${raw}" is not a positive number; ignoring (no forced DPR).`
    )
    return []
  }
  return [`--force-device-scale-factor=${value}`]
}

export function getOrcaElectronLaunchArgs(
  mainPath: string,
  headful: boolean,
  env: NodeJS.ProcessEnv = process.env
): string[] {
  // Why: Chromium switches must precede the app entry path (mainPath), so prepend
  // the forced-DPR switch to whichever base arg list applies.
  const forcedDpr = getForcedDeviceScaleFactorArgs(env)

  if (headful || process.platform !== 'linux') {
    return [...forcedDpr, mainPath]
  }

  // Why: Ubuntu CI can fail headless Electron when Chromium's GPU subprocess
  // cannot initialize; keep E2E on a low-process software path under Xvfb.
  return [
    ...forcedDpr,
    '--disable-gpu',
    '--disable-gpu-compositing',
    '--disable-gpu-sandbox',
    '--disable-dev-shm-usage',
    '--in-process-gpu',
    mainPath
  ]
}
