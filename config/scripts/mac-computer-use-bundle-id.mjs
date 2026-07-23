export const ALAB_COMPUTER_USE_BUNDLE_ID = 'com.stablyai.orca.staging.computer-use'
export const PUBLIC_COMPUTER_USE_BUNDLE_ID = 'com.stablyai.orca.computer-use'

export function resolveMacComputerUseBundleId(env = process.env) {
  // Why: the nested helper owns separate TCC grants and must not impersonate
  // the production helper when the outer app uses the ALab staging identity.
  return (
    env.ORCA_COMPUTER_MACOS_BUNDLE_ID ??
    (env.ORCA_PUBLIC_IDENTITY === '1' ? PUBLIC_COMPUTER_USE_BUNDLE_ID : ALAB_COMPUTER_USE_BUNDLE_ID)
  )
}
