/* eslint-disable max-lines -- Why: owns the full daemon lifecycle (init, launch, adapter wiring,
restart, teardown); the "swap the provider atomically" invariant keeps restart + singletons co-located. */
import { join } from 'node:path'
import { app } from 'electron'
import {
  chmodSync,
  mkdirSync,
  existsSync,
  lstatSync,
  readFileSync,
  unlinkSync,
  writeFileSync
} from 'node:fs'
import { spawn } from 'node:child_process'
import { connect } from 'node:net'
import {
  DaemonSpawner,
  getDaemonPidPath,
  getDaemonSocketPath,
  getDaemonTokenPath,
  serializeDaemonPidFile,
  type DaemonLauncher,
  type DaemonProcessHandle
} from './daemon-spawner'
import { DaemonPtyAdapter, type DaemonRespawnReason } from './daemon-pty-adapter'
import { DaemonPtyRouter } from './daemon-pty-router'
import { DaemonClient } from './client'
import {
  CLEAN_DISCONNECT_PROTOCOL_VERSION,
  PREVIOUS_DAEMON_PROTOCOL_VERSIONS,
  PROTOCOL_VERSION,
  type ListSessionsResult
} from './types'
import {
  getMacDaemonSystemResolverHealth,
  getDaemonLaunchIdentity,
  getProcessStartedAtMs,
  checkDaemonHealth,
  isDaemonStaleForCurrentBundle,
  killStaleDaemon,
  parseDaemonPidFile,
  queryWindowsProcessIdentity
} from './daemon-health'
import { DegradedDaemonPtyProvider } from './degraded-daemon-pty-provider'
import { trackDaemonReplaced, trackDaemonRetired } from './daemon-lifecycle-event'
import type { DaemonReplaceReason } from '../../shared/daemon-lifecycle-telemetry'
import {
  getLocalPtyProvider,
  setLocalPtyProvider,
  unbindLocalProviderListeners,
  rebindLocalProviderListeners
} from '../ipc/pty'
import { setDaemonRuntimeStatus } from '../ipc/daemon-status-registry'
import { cloneInheritedSpawnEnv } from '../pty/inherited-spawn-env'
import { hardenSecurePath } from '../../shared/secure-file'
import { isStartupDiagnosticsEnabled, logStartupDiagnostic } from '../startup/startup-diagnostics'
import { prepareDaemonSessionStoreRoot } from './history-store-layout'
import { scheduleDaemonSessionHistoryGc } from './history-retention'
import {
  confirmSeededClaudeLivePtys,
  hasSeededUnconfirmedClaudePtys
} from '../claude-accounts/live-pty-gate'

// Why: daemon init runs concurrent with window load, so an in-process t timestamp (not harness stderr timing) measures cold-start.
function logDaemonMilestone(event: string, details: Record<string, unknown> = {}): void {
  if (isStartupDiagnosticsEnabled()) {
    logStartupDiagnostic(event, { t: Math.round(performance.now()), ...details })
  }
}

// Why: extra hello+listSessions probes (~5s each) giving a wedged-but-connectable daemon ~60s grace to answer and keep its live sessions before a permanent wedge (#8689) is replaced; raise only alongside the fail-open cap.
export const WEDGED_DAEMON_GRACE_RETRIES = 11
const DAEMON_SELF_SHUTDOWN_WAIT_MS = 5_000

let spawner: DaemonSpawner | null = null
type DaemonProvider = DaemonPtyRouter | DaemonPtyAdapter | DegradedDaemonPtyProvider

let adapter: DaemonProvider | null = null
// Why: coalesce concurrent restartDaemon() calls so two entries can't race the 7-step sequence against a half-spawned replacement.
let restartInFlight: Promise<RestartDaemonResult> | null = null

// Why 0o700: the dir holds the daemon socket and its auth token; a world-readable
// dir holding a credential is the wrong default even when the files inside are 0600.
const RUNTIME_DIR_MODE = 0o700

/** Creates the daemon runtime dir 0700 and REFUSES one this user does not
 *  privately own — mkdir's mode only lands on a dir it creates, and chmod cannot
 *  change a foreign owner, so a pre-existing dir must be proven ours rather than
 *  silently adopted: by the time a chmod tightened it, whatever it already
 *  contains (socket, token) may not be ours. Ported from aterm's
 *  `ensure_private_dir` (crates/aterm-gui/src/control_auth_unix.rs). */
function getRuntimeDir(): string {
  const dir = join(app.getPath('userData'), 'daemon')
  mkdirSync(dir, { recursive: true, mode: RUNTIME_DIR_MODE })
  if (process.platform === 'win32') {
    // POSIX mode bits do not exist here: userData is already ACL'd to this user,
    // so apply the same owner-only NTFS restriction the sibling secure stores use.
    hardenSecurePath(dir, { isDirectory: true, platform: process.platform })
    return dir
  }
  // Judge BEFORE tightening: chmod follows symlinks (so it would act through the
  // very path being judged) and its success would erase the shared-mode evidence
  // the refusal rests on, turning a hostile 0777 dir into an adopted one.
  // lstat, not stat: a symlink planted here would otherwise pass as its target.
  const info = lstatSync(dir)
  const uid = process.getuid?.()
  if (!info.isDirectory() || (uid !== undefined && info.uid !== uid) || (info.mode & 0o022) !== 0) {
    throw new Error(
      `Daemon runtime directory must be a directory owned by uid ${uid} and not group/other-writable: ${dir} (remove it and restart)`
    )
  }
  // Proven ours, a real dir, and unshared — only now narrow one an older build (or
  // a loose umask) left merely readable, e.g. 0755.
  if ((info.mode & 0o777) !== RUNTIME_DIR_MODE) {
    chmodSync(dir, RUNTIME_DIR_MODE)
  }
  return dir
}

/** The current-protocol daemon endpoint (socket + token file), resolved the
 *  same way init/health resolve it. For thin daemon-socket clients (the
 *  coordinator window's byte tunnel) that speak the wire directly instead of
 *  going through the PTY adapter. */
export function getDaemonEndpointPaths(): { socketPath: string; tokenPath: string } {
  const runtimeDir = getRuntimeDir()
  return {
    socketPath: getDaemonSocketPath(runtimeDir),
    tokenPath: getDaemonTokenPath(runtimeDir)
  }
}

function getHistoryDir(): string {
  // Why the layout module: daemon session history lives in a daemon-owned
  // subdir of terminal-history (0o700/ACL-hardened, with a one-time migration
  // of dirs older builds wrote at the shared top level).
  return prepareDaemonSessionStoreRoot(join(app.getPath('userData'), 'terminal-history'))
}

// Why null on failure: the history GC treats unknown liveness as "only prune
// dirs that are provably dead (stamped endedAt)", so a daemon hiccup can
// never cost restorable crash scrollback.
async function collectDaemonLiveSessionIdsForHistoryGc(): Promise<Set<string> | null> {
  const provider = getDaemonProvider()
  if (!provider) {
    return null
  }
  try {
    const adapters = [getCurrentDaemonAdapter(provider), ...getLegacyDaemonAdapters(provider)]
    const ids = new Set<string>()
    for (const daemonAdapter of adapters) {
      for (const session of await daemonAdapter.listSessions()) {
        ids.add(session.sessionId)
      }
    }
    return ids
  } catch {
    return null
  }
}

// Why: a socket that accepts a connection proves a daemon survived a previous app session and can be reused.
function probeSocket(socketPath: string): Promise<boolean> {
  return new Promise((resolve) => {
    if (process.platform !== 'win32' && !existsSync(socketPath)) {
      resolve(false)
      return
    }
    const sock = connect({ path: socketPath })
    let settled = false
    let timer: ReturnType<typeof setTimeout>
    function finish(alive: boolean, options?: { destroy?: boolean }): void {
      if (settled) {
        return
      }
      settled = true
      clearTimeout(timer)
      sock.removeListener('connect', onConnect)
      sock.removeListener('error', onError)
      if (options?.destroy) {
        sock.destroy()
      }
      resolve(alive)
    }

    function onConnect(): void {
      finish(true, { destroy: true })
    }

    function onError(err?: NodeJS.ErrnoException): void {
      // EBUSY (Windows ERROR_PIPE_BUSY) means every pipe instance is taken — a
      // LIVE daemon is holding them, so the socket is alive, not dead. Treating
      // it as dead would let reconcile kill a healthy daemon mid-burst.
      finish(err?.code === 'EBUSY')
    }

    timer = setTimeout(() => {
      finish(false, { destroy: true })
    }, 1000)
    sock.on('connect', onConnect)
    sock.on('error', onError)
  })
}

async function getAliveDaemonSessionCount(
  socketPath: string,
  tokenPath: string,
  protocolVersion = PROTOCOL_VERSION
): Promise<number | null> {
  const client = new DaemonClient({ socketPath, tokenPath, protocolVersion })
  try {
    await client.ensureConnected()
    const result = await client.request<ListSessionsResult>('listSessions', undefined)
    return result.sessions.filter((session) => session.isAlive).length
  } catch {
    return null
  } finally {
    client.disconnect()
  }
}

function createPreservedDaemonHandle(
  runtimeDir: string,
  protocolVersion = PROTOCOL_VERSION,
  mode?: 'degraded-new-pty-fallback'
): DaemonProcessHandle {
  const handle: DaemonProcessHandle = {
    shutdown: async () => {
      await cleanupDaemonForProtocol(runtimeDir, protocolVersion)
    }
  }
  if (mode) {
    handle.mode = mode
  }
  return handle
}

async function holdDaemonAdoptionLease(
  handle: DaemonProcessHandle,
  socketPath: string,
  tokenPath: string,
  connectedClient?: DaemonClient
): Promise<DaemonProcessHandle> {
  const client = connectedClient ?? new DaemonClient({ socketPath, tokenPath })
  try {
    await client.ensureConnected()
  } catch (error) {
    client.disconnect()
    throw error
  }
  handle.releaseAdoptionLease = () => client.disconnect()
  return handle
}

function releaseDaemonAdoptionLease(handle: DaemonProcessHandle | null): void {
  takeDaemonAdoptionLeaseRelease(handle)?.()
}

function takeDaemonAdoptionLeaseRelease(
  handle: DaemonProcessHandle | null
): (() => void) | undefined {
  const release = handle?.releaseAdoptionLease
  if (!release || !handle) {
    return undefined
  }
  delete handle.releaseAdoptionLease
  return release
}

async function cleanupFailedDaemonAdoption(
  failedSpawner: DaemonSpawner,
  current: DaemonPtyAdapter,
  legacy: DaemonPtyAdapter[] = []
): Promise<void> {
  const handle = failedSpawner.getHandle()
  const results = await Promise.allSettled([
    Promise.resolve().then(() => releaseDaemonAdoptionLease(handle)),
    ...legacy.map((entry) => entry.disconnectOnly()),
    (async () => {
      try {
        // Why: other authenticated clients may win, so only daemon-side shutdownIfIdle can prove a failed adoption is killable.
        await current.disconnectOnly()
      } catch (error) {
        current.dispose()
        throw error
      }
    })()
  ])
  const failures = results.flatMap((result) =>
    result.status === 'rejected' ? [result.reason] : []
  )
  if (failures.length > 0) {
    throw new AggregateError(failures, 'Daemon adoption cleanup failed')
  }
}

async function shouldPreserveDaemonWithLiveSessions(
  socketPath: string,
  tokenPath: string,
  replacementLabel: string
): Promise<boolean> {
  const liveSessionCount = await getAliveDaemonSessionCount(socketPath, tokenPath)
  if (liveSessionCount === 0) {
    return false
  }
  console.warn(
    liveSessionCount === null
      ? `[daemon] Preserving daemon ${replacementLabel} because live session state could not be verified`
      : `[daemon] Preserving daemon ${replacementLabel} because it owns ${liveSessionCount} live session${liveSessionCount === 1 ? '' : 's'}`
  )
  return true
}

// Why: the adapter decides a runtime resolver replacement, but the launcher completes it — and by
// then the daemon has usually self-retired (dropping its last authenticated client is enough), so
// there is nothing left to kill and the launcher's own confirmed-kill gate would report nothing.
// The adapter hands the reason across so the launch it triggers reports what actually drove it.
let attributedReplaceReason: DaemonReplaceReason | null = null

// Resolve the orca-daemon binary. ORCA_RUST_DAEMON_BIN overrides. The Rust daemon
// is THE terminal daemon on every platform — its Windows transport is a real
// named-pipe `serve` (orca-winpipe), and there is no Node fallback anywhere: a
// missing binary or startup timeout makes launchRustDaemon throw, which propagates
// through DaemonSpawner.ensureRunning and degrades the app to the in-process,
// non-persistent LocalPtyProvider.
function getRustDaemonBinPath(): string | null {
  const explicit = process.env.ORCA_RUST_DAEMON_BIN
  if (explicit && existsSync(explicit)) {
    return explicit
  }
  // Cargo emits orca-daemon.exe on Windows; match it in the packaged resource
  // name and every dev candidate, else existsSync misses the binary and we strand
  // on the in-process LocalPtyProvider.
  const binName = process.platform === 'win32' ? 'orca-daemon.exe' : 'orca-daemon'
  // Packaged: the binary is shipped to the resources root (electron-builder
  // rustDaemonResource). NEVER probe app.getAppPath()-relative paths here — in a
  // packaged app that is `…/app.asar` (a file), so a candidate inside it can pass
  // existsSync via the asar fs shim yet fail spawn() with ENOTDIR (you cannot exec
  // a path through the archive).
  if (app.isPackaged) {
    const packaged = join(process.resourcesPath ?? '', binName)
    return existsSync(packaged) ? packaged : null
  }
  // Dev: the cargo build output, relative to the app root / cwd.
  const rel = join('rust', 'target', 'release', binName)
  const relDebug = join('rust', 'target', 'debug', binName)
  const candidates = [
    join(app.getAppPath(), rel),
    join(app.getAppPath(), relDebug),
    join(app.getAppPath(), '..', rel),
    join(app.getAppPath(), '..', relDebug),
    join(process.cwd(), rel),
    join(process.cwd(), relDebug)
  ]
  return candidates.find((p) => existsSync(p)) ?? null
}

type DaemonReconcileOutcome = {
  /** Handle to REUSE/PRESERVE the existing daemon; null = kill any stale process and launch fresh. */
  reused: DaemonProcessHandle | null
  /** Replace reason decided here; the kill proving it may still happen in the caller (STA-2376). */
  pendingReplacement?: { reason: DaemonReplaceReason; liveSessionCount: number | null }
  /** True once a cleanup here actually tore a live daemon down. */
  confirmedReplacement: boolean
}

// Reconcile the daemon already sitting on the socket before launching a fresh
// one. Shared by both launch paths (Rust on Unix, Node on Windows) so the
// reuse/replace/preserve decision lives in exactly one place; identityMarker is
// the launcher's own identity (the Rust bin path or the Node entry path) that
// getDaemonLaunchIdentity checks the pid-file against.
async function reconcileExistingDaemon(
  runtimeDir: string,
  socketPath: string,
  tokenPath: string,
  identityMarker: string
): Promise<DaemonReconcileOutcome> {
  let pendingReplacement: DaemonReconcileOutcome['pendingReplacement']
  let confirmedReplacement = false
  const health = await checkDaemonHealth(socketPath, tokenPath)
  if (health === 'healthy') {
    const resolverHealth = await getMacDaemonSystemResolverHealth(socketPath, tokenPath)
    if (resolverHealth === 'unhealthy') {
      const liveSessionCount = await getAliveDaemonSessionCount(socketPath, tokenPath)
      if (liveSessionCount !== 0) {
        console.warn(
          liveSessionCount === null
            ? '[daemon] Preserving daemon with unavailable macOS system resolver because live session state could not be verified'
            : `[daemon] Preserving daemon with unavailable macOS system resolver because it owns ${liveSessionCount} live session${liveSessionCount === 1 ? '' : 's'}`
        )
        return { reused: createPreservedDaemonHandle(runtimeDir), confirmedReplacement: false }
      }
      console.warn('[daemon] Replacing daemon with unavailable macOS system resolver')
      pendingReplacement = { reason: 'unhealthy_resolver', liveSessionCount }
      confirmedReplacement = (await cleanupDaemonForProtocol(runtimeDir, PROTOCOL_VERSION)).cleaned
    } else {
      // Why: a protocol-healthy daemon can outlive the app bundle that
      // launched it. In dev this happens after deleting/rebuilding a
      // worktree; in packaged apps it happens when the stable
      // /Applications/Orca.app path is replaced during update.
      const identity = await getDaemonLaunchIdentity(
        runtimeDir,
        socketPath,
        tokenPath,
        identityMarker
      )
      const stalePackagedBundle =
        app.isPackaged &&
        (await isDaemonStaleForCurrentBundle(runtimeDir, socketPath, tokenPath, app.getVersion()))
      if (identity === 'mismatch' || stalePackagedBundle) {
        // Why: replacing a healthy daemon kills its child PTYs; defer code
        // freshness until no live terminal sessions would be lost.
        const replacementLabel = stalePackagedBundle
          ? 'launched before the current app bundle was installed'
          : 'launched from a different app path'
        if (await shouldPreserveDaemonWithLiveSessions(socketPath, tokenPath, replacementLabel)) {
          return { reused: createPreservedDaemonHandle(runtimeDir), confirmedReplacement: false }
        }
        console.warn(
          stalePackagedBundle
            ? '[daemon] Replacing daemon launched before the current app bundle was installed'
            : '[daemon] Replacing daemon launched from a different app path'
        )
        // liveSessionCount is 0: shouldPreserveDaemonWithLiveSessions() only falls through at exactly 0.
        pendingReplacement = {
          reason: stalePackagedBundle ? 'stale_bundle' : 'different_app_path',
          liveSessionCount: 0
        }
        confirmedReplacement = (await cleanupDaemonForProtocol(runtimeDir, PROTOCOL_VERSION))
          .cleaned
      } else {
        // Why: daemon is already running from a previous app session and
        // responded to a protocol-level ping. Safe to reuse.
        return { reused: createPreservedDaemonHandle(runtimeDir), confirmedReplacement: false }
      }
    }
  } else {
    // Why: a busy machine (e.g. right after an update) can time out the
    // health check while the daemon is alive and owning terminals. Killing
    // it would destroy every live session, so re-verify with a session list
    // first.
    let liveSessionCount = await getAliveDaemonSessionCount(socketPath, tokenPath)
    // Why: on a Windows update relaunch the daemon can be transiently wedged
    // past every RPC budget (final checkpoint flush + installer/AV disk
    // pressure) while its sessions are still alive — replacing it here is what
    // killed those sessions. A pipe that still accepts connections proves a
    // live daemon, so give a wedged-but-connectable daemon a bounded grace to
    // drain and answer before deciding. A PERMANENTLY wedged daemon (accepts
    // connections but its event loop never answers hello — #8689) exhausts the
    // grace and falls through to replacement below, instead of being preserved
    // forever, which strands the app with zero working terminals. 'rejected'
    // means the daemon answered and refused the handshake — it can never be
    // adopted, so it skips the grace and replacement stays the only recovery.
    let graceRetry = 0
    while (
      liveSessionCount === null &&
      health !== 'rejected' &&
      graceRetry < WEDGED_DAEMON_GRACE_RETRIES &&
      (await probeSocket(socketPath))
    ) {
      liveSessionCount = await getAliveDaemonSessionCount(socketPath, tokenPath)
      graceRetry++
    }
    if (liveSessionCount !== null && liveSessionCount > 0) {
      if (health === 'pty-spawn-unhealthy') {
        console.warn(
          `[daemon] DEGRADED MODE: preserving daemon that failed the PTY spawn health check because it owns ${liveSessionCount} live session${liveSessionCount === 1 ? '' : 's'}. Existing sessions keep working; fresh terminals run on the local provider WITHOUT daemon persistence until you restart the daemon (Manage Sessions → Restart).`
        )
        return {
          reused: createPreservedDaemonHandle(
            runtimeDir,
            PROTOCOL_VERSION,
            'degraded-new-pty-fallback'
          ),
          confirmedReplacement: false
        }
      }
      console.warn(
        `[daemon] Preserving daemon that failed the health check because it owns ${liveSessionCount} live session${liveSessionCount === 1 ? '' : 's'}`
      )
      return { reused: createPreservedDaemonHandle(runtimeDir), confirmedReplacement: false }
    }
    // Why: the sibling replace branches announce themselves, but this one used
    // to kill a daemon silently — leaving no way to tell a replacement apart
    // from an adoption after the fact. A cold start also lands here with
    // nothing to replace, so only speak up once something actually answered:
    // a probe that returned a count, a socket that survived a grace retry, or
    // a refused hello.
    if (liveSessionCount !== null || graceRetry > 0 || health === 'rejected') {
      console.warn(
        `[daemon] Replacing daemon that failed the health check (health=${health}, liveSessions=${liveSessionCount ?? 'unverifiable'}, graceRetries=${graceRetry})`
      )
    }
    // Why: unlike the log above, telemetry gates on confirmedReplacement in the caller — the
    // post-kill truth — so a cold start that killed nothing never reports a replacement.
    pendingReplacement = { reason: 'failed_health_check', liveSessionCount }
  }
  // No reusable daemon on the socket — caller must kill any stale process and
  // launch a fresh one.
  return { reused: null, pendingReplacement, confirmedReplacement }
}

// Why: JSON pid file carries pid + process start time so later killStaleDaemon()
// can verify the pid still belongs to the daemon we launched before SIGTERMing
// it — prevents the pid-recycling hazard where the OS hands the daemon's old pid
// to an unrelated process. entryPath = the launcher's identity marker so
// getDaemonLaunchIdentity recognizes our own daemon on the next launch. Shared by
// both launch paths. selfReportedStartedAtMs is the daemon's own start-time
// report (Node ready message): Windows has no cheap OS query for start time, so
// without it the recycling guard was permanently inert on win32.
function writeDaemonPidFile(
  runtimeDir: string,
  pid: number,
  entryPath: string,
  selfReportedStartedAtMs: number | null = null
): void {
  writeFileSync(
    getDaemonPidPath(runtimeDir),
    serializeDaemonPidFile({
      pid,
      startedAtMs: getProcessStartedAtMs(pid) ?? selfReportedStartedAtMs,
      entryPath,
      appVersion: app.getVersion()
    }),
    { mode: 0o600 }
  )
}

// Best-effort SIGTERM: signal a detached daemon pid, swallowing the error when
// the pid is already gone. No-op when pid is falsy.
function killPidBestEffort(pid?: number): void {
  if (pid) {
    try {
      process.kill(pid, 'SIGTERM')
    } catch {
      // already gone
    }
  }
}

// The shutdown handle both launchers return: SIGTERM the detached daemon on app
// teardown. Best-effort — the pid may already be gone.
function makeDaemonSigtermHandle(pid: number | undefined): DaemonProcessHandle {
  return {
    shutdown: async () => {
      killPidBestEffort(pid)
    }
  }
}

// Launch (or reuse) the pure-Rust daemon; returns a handle with the same contract
// as the Node path. The Rust bin isn't a Node fork, so there is no IPC 'ready'
// signal — readiness is the protocol health check (a real hello+ping using the
// token the daemon publishes on startup). A missing binary is a BUILD DEFECT (the
// binary is bundled to resources and produced by the build), so it throws rather
// than degrading — there is no Node fallback on Unix.
async function launchRustDaemon(
  runtimeDir: string,
  socketPath: string,
  tokenPath: string,
  macosLoginSessionWatch: boolean
): Promise<DaemonProcessHandle> {
  // One-shot: whichever launch consumes it owns the attribution, so a later unrelated launch can't
  // reuse it. The write in the respawn callback reaches here without an intervening launch, which is
  // what makes a bare module-scoped slot safe — keep it that way or a concurrent launch can steal it.
  const attributedReason = attributedReplaceReason
  attributedReplaceReason = null

  const binPath = getRustDaemonBinPath()
  if (!binPath) {
    throw new Error(
      'orca-daemon binary not found. It is part of the build (bundled to Resources/orca-daemon; ' +
        'in dev run `pnpm build:rust-daemon`). ' +
        'This is a build defect, not a runtime condition.'
    )
  }

  // Why: acquire the full adoption pair before control-only probes so a
  // retire-on-empty daemon can't self-shutdown in the probe-to-adoption gap
  // (upstream #9277); the pair is handed to the preserved handle's lease below.
  let adoptionClient: DaemonClient | null = new DaemonClient({ socketPath, tokenPath })
  try {
    await adoptionClient.ensureConnected()
  } catch {
    adoptionClient.disconnect()
    adoptionClient = null
  }
  try {
    const reconciled = await reconcileExistingDaemon(runtimeDir, socketPath, tokenPath, binPath)
    if (reconciled.reused) {
      // Every preserve path just proved connectivity (health check or listSessions),
      // so acquiring the adoption lease here cannot regress wedged-daemon preservation.
      const connectedClient = adoptionClient ?? undefined
      adoptionClient = null
      return await holdDaemonAdoptionLease(
        reconciled.reused,
        socketPath,
        tokenPath,
        connectedClient
      )
    }

    // Why: a raw socket can outlive a broken daemon; kill by PID before respawn
    // so the new daemon doesn't race the stale one.
    adoptionClient?.disconnect()
    adoptionClient = null
    const { pendingReplacement } = reconciled
    const confirmedReplacement =
      (await killStaleDaemon(runtimeDir, socketPath, tokenPath)) || reconciled.confirmedReplacement
    // Why: rank by how well each reason is evidenced. A confirmed kill whose reason positively
    // identified the daemon outranks the attribution, so a stale bundle caught here is not billed
    // to the resolver. failed_health_check is the residual "couldn't tell" bucket though — it also
    // absorbs wedges and crashes — so the adapter's attribution beats it. That case is not exotic:
    // the same dead login session that fails the resolver also fails the PTY spawn probe, and with
    // zero live sessions that lands here rather than in the degraded preserve above.
    const identifiedReplacement =
      pendingReplacement &&
      confirmedReplacement &&
      pendingReplacement.reason !== 'failed_health_check'
        ? pendingReplacement
        : null
    if (identifiedReplacement) {
      trackDaemonReplaced(identifiedReplacement.reason, identifiedReplacement.liveSessionCount)
    } else if (attributedReason) {
      trackDaemonReplaced(attributedReason, 0)
    } else if (pendingReplacement && confirmedReplacement) {
      trackDaemonReplaced(pendingReplacement.reason, pendingReplacement.liveSessionCount)
    }

    const userDataPath = app.getPath('userData')
    // Why the flag survives the Rust port: it marks a daemon born inside a macOS
    // GUI login session so it can retire itself once that session dies (#7936).
    // orca-daemon's argv parser skips flags it does not know, so passing it is
    // inert until the watch lands engine-side — but the launch contract, and the
    // GUI-vs-serve distinction it encodes, stay in one place.
    const child = spawn(
      binPath,
      [
        '--socket',
        socketPath,
        '--token',
        tokenPath,
        ...(macosLoginSessionWatch ? ['--login-session-watch'] : [])
      ],
      {
        // Why: match the Node daemon — start from userData so process.cwd() stays
        // valid after a worktree is deleted, and detached + ignore stdio so the daemon
        // outlives Electron and never holds the parent's stdout open.
        cwd: userDataPath,
        detached: true,
        stdio: 'ignore',
        // Why clone here and not just per-spawn envToDelete: every daemon PTY forks
        // from THIS env, and createOrAttach paths that carry no envToDelete (adapter
        // attach/reattach) would hand the pane the raw inherited value.
        env: { ...cloneInheritedSpawnEnv(), ORCA_USER_DATA_PATH: userDataPath }
      }
    )
    // Why: spawn() reports failures (ENOENT/EACCES) via an async 'error' event, not
    // a throw. Without a listener Node re-raises it as an uncaught exception that
    // crashes the main process; capture it so the poll loop bails cleanly.
    let spawnError: Error | null = null
    child.on('error', (err) => {
      spawnError = err
    })

    // Poll protocol health until the daemon answers (no IPC 'ready' channel).
    const deadlineMs = Date.now() + 10000
    let ready = false
    while (Date.now() < deadlineMs) {
      if (spawnError || child.exitCode !== null || child.signalCode !== null) {
        break
      }
      if ((await checkDaemonHealth(socketPath, tokenPath)) === 'healthy') {
        ready = true
        break
      }
      await new Promise((resolve) => setTimeout(resolve, 150))
    }
    if (!ready) {
      killPidBestEffort(child.pid)
      const err = spawnError as Error | null
      throw new Error(
        err ? `Rust daemon failed to spawn: ${err.message}` : 'Rust daemon startup timed out'
      )
    }

    if (child.pid) {
      writeDaemonPidFile(runtimeDir, child.pid, binPath)
      // win32: the pid file above has startedAtMs null (no cheap sync OS query, and
      // the Rust daemon has no IPC ready message to self-report like the old Node
      // daemon) — backfill it asynchronously so the pid-recycling guard is armed.
      void backfillWin32DaemonPidFileStartTime(runtimeDir, child.pid, binPath)
    }
    child.unref()
    // Why the lease: the launcher holds a connected client pair until the adapter
    // establishes its permanent lifecycle lease, so a retire-on-empty daemon can
    // never self-shutdown in the launch-to-adoption gap (upstream #9277).
    return await holdDaemonAdoptionLease(makeDaemonSigtermHandle(child.pid), socketPath, tokenPath)
  } catch (error) {
    adoptionClient?.disconnect()
    throw error
  }
}

// Fire-and-forget: keeps the 300-800ms powershell CIM spawn off the launch path;
// until it lands (or if it fails) the guard just stays fail-open on null, which
// was the previous steady state on win32 for the Rust daemon.
async function backfillWin32DaemonPidFileStartTime(
  runtimeDir: string,
  pid: number,
  entryPath: string
): Promise<void> {
  if (process.platform !== 'win32') {
    // Unix start times resolve synchronously inside writeDaemonPidFile.
    return
  }
  try {
    const startedAtMs = (await queryWindowsProcessIdentity(pid))?.startedAtMs ?? null
    if (startedAtMs === null) {
      return
    }
    // Only rewrite while the pid file still describes THIS daemon — a
    // replacement launched during the query must not be clobbered.
    const current = parseDaemonPidFile(readFileSync(getDaemonPidPath(runtimeDir), 'utf8'))
    if (current?.pid !== pid) {
      return
    }
    writeDaemonPidFile(runtimeDir, pid, entryPath, startedAtMs)
  } catch {
    // Best-effort: a missing/unreadable pid file keeps the fail-open null.
  }
}

function createOutOfProcessLauncher(
  runtimeDir: string,
  macosLoginSessionWatch = false
): DaemonLauncher {
  // The Rust daemon is THE terminal daemon on every platform — no Node fallback.
  return async (socketPath, tokenPath) =>
    launchRustDaemon(runtimeDir, socketPath, tokenPath, macosLoginSessionWatch)
}

// Why: when the daemon process dies (e.g. killed by a signal, OOM, or cascading
// from a force-quit of child processes), the adapter's ensureConnected() detects
// the dead socket and calls this to fork a replacement daemon before retrying the
// connection. Shared by the initial adapter and the restart adapter. Returns the
// launcher's temporary adoption-lease release so the adapter can drop it once
// its own permanent pair is re-established.
function makeRespawnCallback(
  spawner: DaemonSpawner
): (reason: DaemonRespawnReason) => Promise<void | (() => void)> {
  return async (reason) => {
    // Why: attribute rather than emit — the launcher is the one that completes the
    // replacement, and emitting here would fire before the outcome is known.
    // Caveat: a wedged-but-alive daemon (#8689) can still report died_respawn here and
    // failed_health_check from the launcher — the app cannot tell wedged from dead at this point.
    if (reason === 'daemon_died') {
      console.warn('[daemon] Daemon process died — respawning')
      // Why: a manual restart tears the daemon down under a still-live adapter, so a pane
      // respawning on its synthetic exit would bill a user action to the crash bucket.
      if (!restartInFlight) {
        trackDaemonRetired('died_respawn')
      }
    } else if (reason === 'unhealthy_resolver') {
      // Must reach the launcher without an intervening launch; see the consume site in launchRustDaemon.
      attributedReplaceReason = 'unhealthy_resolver'
    }
    spawner.resetHandle()
    await spawner.ensureRunning()
    return takeDaemonAdoptionLeaseRelease(spawner.getHandle())
  }
}

export async function initDaemonPtyProvider(
  signal?: AbortSignal,
  options: { macosLoginSessionWatch?: boolean } = {}
): Promise<void> {
  // Why (docs/reference/daemon-staleness-ux.md §Phase 2): every init outcome must land in
  // the daemon-status registry so the renderer can surface lost persistence —
  // a silent fallback to the local provider is the failure mode this prevents.
  setDaemonRuntimeStatus('starting')
  let installed: DaemonProvider | null
  try {
    installed = await runInitDaemonPtyProvider(signal, options)
  } catch (error) {
    setDaemonRuntimeStatus('failed', {
      cause: 'launch-failed',
      detail: error instanceof Error ? error.message : String(error)
    })
    throw error
  }
  if (installed instanceof DegradedDaemonPtyProvider) {
    setDaemonRuntimeStatus('degraded-fallback', { cause: 'spawn-unhealthy' })
  } else if (installed) {
    setDaemonRuntimeStatus('running')
  } else {
    // Aborted by startup fail-open: no daemon provider was installed, so fresh
    // spawns run on the in-process LocalPtyProvider without persistence.
    setDaemonRuntimeStatus('degraded-fallback', { cause: 'startup-timeout' })
  }
}

// Returns the provider that was installed, or null when the init attempt was
// aborted by the startup fail-open path (no swap happened).
async function runInitDaemonPtyProvider(
  signal?: AbortSignal,
  options: { macosLoginSessionWatch?: boolean } = {}
): Promise<DaemonProvider | null> {
  logDaemonMilestone('daemon-init-start')
  // Why: e2e coverage for the startup PTY gate (#5232) needs a daemon init that deterministically outlasts the first-window timeout.
  const e2eInitDelayMs = Number(process.env.ORCA_E2E_DAEMON_INIT_DELAY_MS)
  if (Number.isFinite(e2eInitDelayMs) && e2eInitDelayMs > 0) {
    await new Promise((resolve) => setTimeout(resolve, e2eInitDelayMs))
  }
  const runtimeDir = getRuntimeDir()

  const newSpawner = new DaemonSpawner({
    runtimeDir,
    launcher: createOutOfProcessLauncher(runtimeDir, options.macosLoginSessionWatch ?? false)
  })

  // Why: assign the module-level spawner/adapter only after both succeed, so a failed ensureRunning() leaves no stale spawner.
  const info = await newSpawner.ensureRunning()
  const launchMode = newSpawner.getHandle()?.mode
  logDaemonMilestone('daemon-current-ready')
  if (signal?.aborted) {
    // Why: fail-open may already have spawned fallback PTYs; don't install late, but retire an empty daemon (live sessions reject it and survive).
    const abortedStartupAdapter = new DaemonPtyAdapter({
      socketPath: info.socketPath,
      tokenPath: info.tokenPath
    })
    releaseDaemonAdoptionLease(newSpawner.getHandle())
    await abortedStartupAdapter.disconnectOnly()
    return null
  }

  const newAdapter = new DaemonPtyAdapter({
    socketPath: info.socketPath,
    tokenPath: info.tokenPath,
    historyPath: getHistoryDir(),
    respawn: makeRespawnCallback(newSpawner)
  })
  let legacyAdapters: DaemonPtyAdapter[] = []
  let routedAdapter: DaemonProvider = newAdapter
  try {
    // Why: the launcher's temporary pair closes only after this permanent pair is established, leaving no adoption gap.
    await newAdapter.establishLifecycleLease()
    releaseDaemonAdoptionLease(newSpawner.getHandle())

    legacyAdapters = await createLegacyDaemonAdapters(runtimeDir)
    routedAdapter =
      launchMode === 'degraded-new-pty-fallback'
        ? new DegradedDaemonPtyProvider({
            current: newAdapter,
            legacy: legacyAdapters,
            fallback: getLocalPtyProvider(),
            probeCurrentDaemonSpawn: async () =>
              (await checkDaemonHealth(info.socketPath, info.tokenPath)) === 'healthy'
          })
        : legacyAdapters.length > 0
          ? new DaemonPtyRouter({
              current: newAdapter,
              legacy: legacyAdapters
            })
          : newAdapter
    if (routedAdapter instanceof DegradedDaemonPtyProvider) {
      // Why: preserved daemon can't create fresh terminals; discover its live session ids so only they route to it (fresh panes fall back locally).
      await routedAdapter.discoverDaemonSessions()
    } else if (routedAdapter instanceof DaemonPtyRouter) {
      await routedAdapter.discoverLegacySessions()
    }
    if (signal?.aborted) {
      // Why: same late-swap guard after legacy discovery; release uninstalled adapter leases without killing live sessions.
      await routedAdapter.disconnectOnly()
      return null
    }
  } catch (error) {
    try {
      await cleanupFailedDaemonAdoption(newSpawner, newAdapter, legacyAdapters)
    } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], 'Daemon adoption and cleanup both failed')
    }
    throw error
  }
  spawner = newSpawner
  adapter = routedAdapter
  setLocalPtyProvider(routedAdapter)
  // Why: the first window may register PTY listeners before daemon init finishes; rebind so daemon PTYs still fan out events.
  rebindLocalProviderListeners()
  // Startup + periodic reclaim of stranded at-rest scrollback (age + size
  // caps). Idempotent across daemon restarts; liveness is re-queried per pass.
  scheduleDaemonSessionHistoryGc({
    getSessionsRoot: getHistoryDir,
    collectLiveSessionIds: collectDaemonLiveSessionIdsForHistoryGc
  })
  logDaemonMilestone('daemon-init-done', { legacyAdapters: legacyAdapters.length })
  await reconcileSeededClaudeLivePtys(routedAdapter)
  return routedAdapter
}

// Why: release gate ids only for daemon-confirmed-dead sessions; keep seeds on listing failure since releasing early can rotate a live CLI's refresh token.
async function reconcileSeededClaudeLivePtys(provider: DaemonProvider): Promise<void> {
  if (!hasSeededUnconfirmedClaudePtys()) {
    return
  }
  try {
    const adapters =
      provider instanceof DaemonPtyRouter || provider instanceof DegradedDaemonPtyProvider
        ? provider.getAllAdapters()
        : [provider]
    const results = await Promise.allSettled(adapters.map((entry) => entry.listSessions()))
    if (results.some((result) => result.status === 'rejected')) {
      console.warn('[daemon] Keeping seeded Claude live-PTY gate — session listing failed')
      return
    }
    confirmSeededClaudeLivePtys(
      results.flatMap((result) =>
        result.status === 'fulfilled' ? result.value.map((session) => session.sessionId) : []
      )
    )
  } catch (error) {
    // Why: gate bookkeeping must never fail daemon init; stale seeds only defer a usage refresh until next restart.
    console.warn('[daemon] Failed to reconcile seeded Claude live-PTY gate:', error)
  }
}

// Why: a narrow getter (not a raw export) keeps the "swap on restart" invariant in one place (replaceDaemonProvider).
export function getDaemonProvider(): DaemonProvider | null {
  return adapter
}

// Why: keep the module-level adapter and ipc/pty.ts's localProvider in sync so app-quit can't dispose a stale reference.
export function replaceDaemonProvider(newAdapter: DaemonProvider): void {
  adapter = newAdapter
  setLocalPtyProvider(newAdapter)
}

function getCurrentDaemonAdapter(provider: DaemonProvider): DaemonPtyAdapter {
  if (provider instanceof DaemonPtyRouter || provider instanceof DegradedDaemonPtyProvider) {
    return provider.getCurrentAdapter()
  }
  return provider
}

function getLegacyDaemonAdapters(provider: DaemonProvider): DaemonPtyAdapter[] {
  if (provider instanceof DaemonPtyRouter || provider instanceof DegradedDaemonPtyProvider) {
    return [...provider.getLegacyAdapters()]
  }
  return []
}

function disposeProviderSubscriptionsOnly(provider: DaemonProvider): void {
  if (provider instanceof DaemonPtyRouter) {
    provider.disposeRouterOnly()
    return
  }
  if (provider instanceof DegradedDaemonPtyProvider) {
    provider.disposeProviderOnly()
  }
}

export type RestartDaemonResult = {
  killedCount: number
}

// Why: the 7-step restart sequence from docs/reference/daemon-staleness-ux.md §Phase 1; current-protocol only (legacy adapters preserved).
export async function restartDaemon(): Promise<RestartDaemonResult> {
  if (restartInFlight) {
    return restartInFlight
  }
  // Why: with no provider installed (launch failed or startup fail-open) there
  // is nothing to restart — recovery is re-running init (see
  // relaunchDaemonForRecovery in ipc/daemon-status.ts). Reject before the
  // status hooks below so the registry keeps the actionable launch-failed
  // detail instead of clobbering it with this precondition as 'restart-failed'.
  if (!spawner || !adapter) {
    throw new Error('restartDaemon called before initDaemonPtyProvider')
  }
  // Why: every restart caller (settings button, status-toast Retry) must flip
  // the shared daemon-status registry, not just its own toast — the sticky
  // degraded/failed surfaces clear only on a registry transition.
  restartInFlight = runRestartDaemon()
    .then((result) => {
      // Why: key success off the installed provider — restart currently always
      // installs a healthy adapter/router, but a future degraded restart
      // outcome must not be misreported as 'running'.
      if (adapter instanceof DegradedDaemonPtyProvider) {
        setDaemonRuntimeStatus('degraded-fallback', { cause: 'spawn-unhealthy' })
      } else {
        setDaemonRuntimeStatus('running')
      }
      return result
    })
    .catch((error: unknown) => {
      setDaemonRuntimeStatus('failed', {
        cause: 'restart-failed',
        detail: error instanceof Error ? error.message : String(error)
      })
      throw error
    })
    .finally(() => {
      restartInFlight = null
    })
  return restartInFlight
}

async function runRestartDaemon(): Promise<RestartDaemonResult> {
  const currentSpawner = spawner
  const currentAdapter = adapter
  if (!currentSpawner || !currentAdapter) {
    // Unreachable: restartDaemon rejects pre-hook when these are null (and
    // calls us synchronously, so they can't be nulled in between). TS narrowing.
    throw new Error('restartDaemon called before initDaemonPtyProvider')
  }

  const runtimeDir = getRuntimeDir()
  const currentOnly = getCurrentDaemonAdapter(currentAdapter)
  const legacyAdapters = getLegacyDaemonAdapters(currentAdapter)

  // Step 1: synthesize pty:exit for every active session BEFORE teardown — the daemon's shutdown path never fans onExit to clients (session.ts:246-252), so the renderer would otherwise never see exits.
  const fallbackKilledCount =
    currentAdapter instanceof DegradedDaemonPtyProvider
      ? await currentAdapter.shutdownFallbackSessions()
      : 0
  const currentDaemonSessionIds =
    currentAdapter instanceof DegradedDaemonPtyProvider
      ? currentAdapter.getCurrentDaemonSessionIds()
      : []
  const killedCount =
    new Set([...currentOnly.getActiveSessionIds(), ...currentDaemonSessionIds]).size +
    fallbackKilledCount
  currentOnly.fanoutSyntheticExits(-1)
  if (currentAdapter instanceof DegradedDaemonPtyProvider) {
    currentAdapter.fanoutCurrentDaemonSyntheticExits(-1)
  }

  // Step 2: detach renderer listeners — after step 1 (so synthesized exits land) and before step 6 (no stale binding).
  unbindLocalProviderListeners()

  // Step 3: kill the current-protocol daemon process; legacy adapters untouched.
  let info: Awaited<ReturnType<DaemonSpawner['ensureRunning']>>
  try {
    await cleanupDaemonForProtocol(runtimeDir, PROTOCOL_VERSION)

    // Step 4: reuse the existing spawner so the respawn closure baked into long-lived adapters stays valid (do NOT new one).
    currentSpawner.resetHandle()
    info = await currentSpawner.ensureRunning()
  } catch (error) {
    // Why: old provider stays authoritative until the final swap; rebind since relaunch failed after teardown.
    rebindLocalProviderListeners()
    throw error
  }

  // Step 5: build a fresh current adapter against the respawned daemon. Its
  // respawn callback closes over the same spawner instance.
  const newCurrent = new DaemonPtyAdapter({
    socketPath: info.socketPath,
    tokenPath: info.tokenPath,
    historyPath: getHistoryDir(),
    respawn: makeRespawnCallback(currentSpawner)
  })
  let newProvider: DaemonProvider = newCurrent
  try {
    // Temporary launcher lease overlaps this permanent pair so a manual restart can't strand a newly spawned daemon during adoption.
    await newCurrent.establishLifecycleLease()
    releaseDaemonAdoptionLease(currentSpawner.getHandle())

    // Re-wrap in a router only if legacy adapters exist; they're preserved by reference and still route to their pre-upgrade daemons.
    newProvider =
      legacyAdapters.length > 0
        ? new DaemonPtyRouter({ current: newCurrent, legacy: legacyAdapters })
        : newCurrent
    if (newProvider instanceof DaemonPtyRouter) {
      await newProvider.discoverLegacySessions()
    }
  } catch (error) {
    let cleanupError: unknown
    try {
      if (newProvider instanceof DaemonPtyRouter) {
        newProvider.disposeRouterOnly()
      }
      await cleanupFailedDaemonAdoption(currentSpawner, newCurrent)
    } catch (caught) {
      cleanupError = caught
    }
    // Previous provider stays module-authoritative until the swap; restore its renderer bindings when adoption fails.
    rebindLocalProviderListeners()
    if (cleanupError) {
      throw new AggregateError([error, cleanupError], 'Daemon restart and cleanup both failed')
    }
    throw error
  }

  // Drain the old router's subscriptions via the router-only variant (plain dispose() would tear down the shared legacy adapters), after the new provider exists (no unhandled events) and before the swap (atomic for the renderer).
  disposeProviderSubscriptionsOnly(currentAdapter)

  // Step 6: swap module state (adapter + localProvider) atomically.
  replaceDaemonProvider(newProvider)

  // Step 7: rebind renderer listeners against the new provider.
  rebindLocalProviderListeners()

  return { killedCount }
}

// Disconnect without killing: the daemon survives app quit so sessions stay warm for reattach.
// Leave history sessions marked "unclean" so a daemon crash while Orca is closed stays recoverable.
export async function disconnectDaemon(): Promise<void> {
  await adapter?.disconnectOnly()
  adapter = null
}

/** Kill the daemon and all its sessions. Use for full cleanup only. */
export async function shutdownDaemon(): Promise<void> {
  adapter?.dispose()
  adapter = null
  await spawner?.shutdown()
  spawner = null
}

export type OrphanedDaemonCleanupResult = {
  /** True when a live daemon socket was found and torn down; false when none was running. */
  cleaned: boolean
  /** Number of live PTY sessions killed during cleanup (surfaced to the user). */
  killedCount: number
}

export async function cleanupDaemonForProtocol(
  runtimeDir: string,
  protocolVersion: number
): Promise<OrphanedDaemonCleanupResult> {
  const socketPath = getDaemonSocketPath(runtimeDir, protocolVersion)
  const tokenPath = getDaemonTokenPath(runtimeDir, protocolVersion)
  const pidPath = getDaemonPidPath(runtimeDir, protocolVersion)

  const alive = await probeSocket(socketPath)
  if (!alive) {
    if (protocolVersion >= CLEAN_DISCONNECT_PROTOCOL_VERSION) {
      // Endpoint absence doesn't prove the PID record belongs to the current protocol; leave artifact cleanup to the owning daemon.
      return { cleaned: false, killedCount: 0 }
    }
    // Best-effort remove a stale socket so a future launch doesn't hit EADDRINUSE on bind.
    if (process.platform !== 'win32' && existsSync(socketPath)) {
      try {
        unlinkSync(socketPath)
      } catch {
        // Best-effort
      }
    }
    try {
      unlinkSync(pidPath)
    } catch {
      // Best-effort
    }
    return { cleaned: false, killedCount: 0 }
  }

  const client = new DaemonClient({ socketPath, tokenPath, protocolVersion })
  let killedCount = 0
  let didRequestShutdown = false
  let didKillStaleDaemon = false
  try {
    await client.ensureConnected()
    const sessions = await client
      .request<ListSessionsResult>('listSessions', undefined)
      .catch(() => ({ sessions: [] }))
    killedCount = sessions.sessions.filter((s) => s.isAlive).length

    // Use the single-shot `shutdown` RPC (kills all sessions then exits) to avoid racing per-session `kill` calls against the daemon exiting.
    await client.request('shutdown', { killSessions: true }).catch(() => {
      // Daemon exits immediately after the RPC, so the socket may close before the reply arrives; treat as success.
    })
    didRequestShutdown = true
  } catch {
    // Previous-protocol daemons may be wedged or too old for the RPC path; fall back to PID cleanup (only unlinks a live socket after proving the process is killed).
    didKillStaleDaemon = await killStaleDaemon(runtimeDir, socketPath, tokenPath, protocolVersion)
  } finally {
    client.disconnect()
  }

  if (didRequestShutdown && protocolVersion >= CLEAN_DISCONNECT_PROTOCOL_VERSION) {
    if (!(await waitForDaemonEndpointExit(socketPath))) {
      // Never fork a replacement while the old incarnation may still own the endpoint or be disposing terminal children.
      throw new Error('Timed out waiting for daemon self-shutdown')
    }
    return { cleaned: true, killedCount }
  }

  // Defensively unlink the socket: the daemon normally removes it after `shutdown`, but on some crash paths it lingers and blocks a later rebind.
  if (didRequestShutdown && process.platform !== 'win32' && existsSync(socketPath)) {
    try {
      unlinkSync(socketPath)
    } catch {
      // Best-effort
    }
  }
  try {
    unlinkSync(pidPath)
  } catch {
    // Best-effort
  }

  return { cleaned: didRequestShutdown || didKillStaleDaemon, killedCount }
}

async function waitForDaemonEndpointExit(socketPath: string): Promise<boolean> {
  const deadline = Date.now() + DAEMON_SELF_SHUTDOWN_WAIT_MS
  while (Date.now() < deadline) {
    if (!(await probeSocket(socketPath))) {
      return true
    }
    await new Promise((resolve) => setTimeout(resolve, 50))
  }
  return !(await probeSocket(socketPath))
}

function legacyDaemonProcessMayBeAlive(runtimeDir: string, protocolVersion: number): boolean {
  try {
    const parsed = parseDaemonPidFile(
      readFileSync(getDaemonPidPath(runtimeDir, protocolVersion), 'utf8')
    )
    if (!parsed) {
      return false
    }
    process.kill(parsed.pid, 0)
    return true
  } catch {
    return false
  }
}

// Why: callers that own an isolated runtime namespace must keep discovery history out of app userData.
export async function createLegacyDaemonAdapters(
  runtimeDir: string,
  historyPath = getHistoryDir()
): Promise<DaemonPtyAdapter[]> {
  const adapters: DaemonPtyAdapter[] = []
  for (const protocolVersion of PREVIOUS_DAEMON_PROTOCOL_VERSIONS) {
    const socketPath = getDaemonSocketPath(runtimeDir, protocolVersion)
    const tokenPath = getDaemonTokenPath(runtimeDir, protocolVersion)
    if (!(await probeSocket(socketPath))) {
      // Why: a recycled stale pid later turns an identity check into a PowerShell spawn, so delete leaked pid/token files — but only when the pid-process is provably gone (a live daemon can transiently fail the probe, and dropping its token makes its sessions permanently unadoptable).
      if (!legacyDaemonProcessMayBeAlive(runtimeDir, protocolVersion)) {
        for (const stalePath of [
          getDaemonPidPath(runtimeDir, protocolVersion),
          getDaemonTokenPath(runtimeDir, protocolVersion)
        ]) {
          try {
            unlinkSync(stalePath)
          } catch {
            // Best-effort
          }
        }
        if (process.platform !== 'win32' && existsSync(socketPath)) {
          try {
            unlinkSync(socketPath)
          } catch {
            // Best-effort
          }
        }
      }
      continue
    }
    // Keep old-protocol PTYs routed to their original daemon during upgrade; legacy adapters never respawn (new code would recreate stale env semantics).
    // historyPath is still needed for cleanup — without it a later v4 session reusing the same ID could false-restore stale scrollback.bin.
    adapters.push(
      new DaemonPtyAdapter({
        socketPath,
        tokenPath,
        protocolVersion,
        historyPath
      })
    )
  }
  return adapters
}
