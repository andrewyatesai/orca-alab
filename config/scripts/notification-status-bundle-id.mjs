// Why: must mirror the appId identity split in config/electron-builder.config.cjs —
// macOS keys notification records to CFBundleIdentifier, so a fork helper embedding
// upstream's id would read public Orca's notification authorization.
export function resolveNotificationStatusBundleId(env = process.env) {
  return env.ORCA_PUBLIC_IDENTITY === '1' ? 'com.stablyai.orca' : 'com.stablyai.orca.staging'
}
