// The daemon parity corpus: a single stateful RPC sequence driven over the real
// socket against BOTH daemons. Each step projects the raw RpcResponse into a
// volatile-free STRUCTURAL fingerprint — the contract the Rust and Node daemons
// must agree on. Engine-rendered fields (snapshotAnsi bytes, exact pid,
// createdAt) are NOT byte-compared: the two daemons render through different VT
// engines (aterm vs @xterm/headless), so those legitimately differ and are
// reduced to semantic tags (has-marker, is-number). This gate proves
// wire-protocol + behavioral parity, not engine-render equality (that is the
// aterm conformance gauntlet's job).

const SID = 's-parity-1'
const CWD = '/tmp/dparity'
const MARKER = 'MARKER_PARITY_XYZ'
// OSC-7 sets the engine's cwd to CWD; the marker lands on the rendered grid.
const DRIVE_LINE = `printf '\\033]7;file://${CWD}\\007${MARKER}\\n'\n`

const typeTag = (v) => (v === null ? 'null' : Array.isArray(v) ? 'array' : typeof v)

async function waitFor(pred, { tries = 100, delayMs = 25 } = {}) {
  for (let i = 0; i < tries; i++) {
    if (await pred()) {
      return true
    }
    await new Promise((r) => setTimeout(r, delayMs))
  }
  return false
}

// Drive the full sequence against a connected DaemonSocketClient and return an
// ordered list of {step, projection} fingerprints plus a couple of semantic
// facts collected across steps.
export async function driveDaemon(client) {
  const steps = []
  const record = (step, projection) => steps.push({ step, projection })

  // 1. Liveness.
  const ping = await client.rpc('ping')
  record('ping', { ok: ping.ok, pong: ping.payload?.pong ?? null })

  // 2. Create a fresh session.
  const created = await client.rpc('createOrAttach', {
    sessionId: SID,
    cols: 88,
    rows: 26
  })
  record('createOrAttach:new', {
    ok: created.ok,
    isNew: created.payload?.isNew ?? null,
    pidType: typeTag(created.payload?.pid),
    hasSnapshot: (created.payload?.snapshot ?? null) !== null,
    shellStateType: typeTag(created.payload?.shellState)
  })

  // 3. Drive the shell: OSC-7 cwd + a marker, both parsed by the engine.
  const write1 = await client.rpc('write', { sessionId: SID, data: DRIVE_LINE })
  record('write:drive', { ok: write1.ok })

  // 4. getCwd reflects the OSC-7 once the engine parses it — a real readiness
  //    signal (stronger than echoed command text).
  let lastCwd = null
  const cwdReady = await waitFor(async () => {
    const r = await client.rpc('getCwd', { sessionId: SID })
    lastCwd = r.payload?.cwd ?? null
    return r.payload?.cwd === CWD
  })
  record('getCwd', { ok: cwdReady, cwd: lastCwd })

  // 5. Snapshot: dims exact, cwd exact, marker present (rendered), modes shape.
  const snap = await client.rpc('getSnapshot', { sessionId: SID })
  const s = snap.payload?.snapshot ?? null
  record('getSnapshot', {
    ok: snap.ok,
    hasSnapshot: s !== null,
    cols: s?.cols ?? null,
    rows: s?.rows ?? null,
    cwd: s?.cwd ?? null,
    snapshotHasMarker: typeof s?.snapshotAnsi === 'string' && s.snapshotAnsi.includes(MARKER),
    modeKeys: s?.modes ? Object.keys(s.modes).sort() : null
  })

  // 6. Size mirrors the created grid. Wire shape: payload.size.{cols,rows}.
  const size = await client.rpc('getSize', { sessionId: SID })
  record('getSize', {
    ok: size.ok,
    cols: size.payload?.size?.cols ?? null,
    rows: size.payload?.size?.rows ?? null
  })

  // 7. listSessions shows the live session with the right shape.
  const list = await client.rpc('listSessions')
  const info = (list.payload?.sessions ?? []).find((x) => x.sessionId === SID) ?? null
  record('listSessions', {
    ok: list.ok,
    found: info !== null,
    isAlive: info?.isAlive ?? null,
    pidType: typeTag(info?.pid),
    cols: info?.cols ?? null,
    rows: info?.rows ?? null,
    stateType: typeTag(info?.state)
  })

  // 8. Live output streamed to the stream socket carries the marker.
  const streamHasMarker = client.streamData(SID).includes(MARKER)
  record('streamData', { hasMarker: streamHasMarker })

  // 9. Reattach on the live id → isNew:false (idempotency).
  const reattach = await client.rpc('createOrAttach', {
    sessionId: SID,
    cols: 88,
    rows: 26
  })
  record('createOrAttach:reattach', { ok: reattach.ok, isNew: reattach.payload?.isNew ?? null })

  // 10. Resize is honored by the engine + reported by getSize.
  const resize = await client.rpc('resize', { sessionId: SID, cols: 100, rows: 30 })
  const sizeAfter = await client.rpc('getSize', { sessionId: SID })
  record('resize', {
    ok: resize.ok,
    cols: sizeAfter.payload?.size?.cols ?? null,
    rows: sizeAfter.payload?.size?.rows ?? null
  })

  // 11. clearScrollback succeeds.
  const clear = await client.rpc('clearScrollback', { sessionId: SID })
  record('clearScrollback', { ok: clear.ok })

  // 12. Error cases: unknown-session write + snapshot must both fail cleanly.
  const badWrite = await client.rpc('write', { sessionId: 'nope', data: 'x' })
  record('write:unknown', { ok: badWrite.ok, errorType: typeTag(badWrite.error) })
  // Unknown-session getSnapshot is graceful in the reference daemon: ok:true
  // with snapshot=null (not an error), so both daemons must agree on that.
  const badSnap = await client.rpc('getSnapshot', { sessionId: 'nope' })
  record('getSnapshot:unknown', {
    ok: badSnap.ok,
    snapshotIsNull: (badSnap.payload?.snapshot ?? null) === null
  })

  // 13. Kill ends the session; an exit event should reach the stream and the
  //     session should stop reporting alive.
  const kill = await client.rpc('kill', { sessionId: SID })
  const exited = await waitFor(async () => {
    const r = await client.rpc('listSessions')
    const i = (r.payload?.sessions ?? []).find((x) => x.sessionId === SID)
    return !i || i.isAlive === false
  })
  const sawExit = client.events().some((e) => e.event === 'exit' && e.sessionId === SID)
  record('kill', { ok: kill.ok, noLongerAlive: exited, sawExitEvent: sawExit })

  // 14. Daemon still healthy after the session's lifecycle.
  const ping2 = await client.rpc('ping')
  record('ping:after', { ok: ping2.ok, pong: ping2.payload?.pong ?? null })

  return { steps }
}

// Phase 2 — the daemon's raison d'être: a session SURVIVES full client
// disconnect (both sockets), and a later reattach (same clientId, as after an
// app reload) finds it live with its engine state intact. `connectClient` is a
// factory `(clientId) => Promise<connected DaemonSocketClient>` so this can drop
// and re-open sockets against the same running daemon.
const SID2 = 's-parity-2'
const MARKER2 = 'MARKER_SURVIVES_RELOAD'

export async function driveDetachReattach(connectClient) {
  const steps = []
  const record = (step, projection) => steps.push({ step, projection })
  const CLIENT = 'parity-reload-client'

  // First attachment: create a session and print a durable marker.
  const c1 = await connectClient(CLIENT)
  const created = await c1.rpc('createOrAttach', { sessionId: SID2, cols: 80, rows: 24 })
  record('reload:create', { ok: created.ok, isNew: created.payload?.isNew ?? null })
  await c1.rpc('write', { sessionId: SID2, data: `printf '${MARKER2}\\n'\n` })
  const marked = await waitFor(async () => {
    const r = await c1.rpc('getSnapshot', { sessionId: SID2 })
    return (r.payload?.snapshot?.snapshotAnsi ?? '').includes(MARKER2)
  })
  record('reload:marked', { ok: marked })

  // Full disconnect — both sockets close, as when the renderer/window goes away.
  c1.close()
  await new Promise((r) => setTimeout(r, 150))

  // Reattach with the SAME clientId (the reload path).
  const c2 = await connectClient(CLIENT)
  const reattach = await c2.rpc('createOrAttach', { sessionId: SID2, cols: 80, rows: 24 })
  record('reload:reattach', {
    ok: reattach.ok,
    // The session must still be there → isNew:false, not a fresh spawn.
    isNew: reattach.payload?.isNew ?? null
  })

  // The engine state (the marker) survived the disconnect.
  const snap = await c2.rpc('getSnapshot', { sessionId: SID2 })
  record('reload:snapshotSurvived', {
    ok: snap.ok,
    hasMarker: (snap.payload?.snapshot?.snapshotAnsi ?? '').includes(MARKER2)
  })

  // listSessions (from the new client) still reports it alive.
  const list = await c2.rpc('listSessions')
  const info = (list.payload?.sessions ?? []).find((x) => x.sessionId === SID2) ?? null
  record('reload:stillListed', { found: info !== null, isAlive: info?.isAlive ?? null })

  await c2.rpc('kill', { sessionId: SID2 })
  c2.close()
  return { steps }
}

export const parityConstants = { SID, SID2, CWD, MARKER, MARKER2 }
