import type { SshTarget, SshConnectionState } from '../../shared/ssh-types'
import { SshConnection, type SshConnectionCallbacks } from './ssh-connection'

// ── Connection Manager ──────────────────────────────────────────────
// Why: extracted from ssh-connection.ts to keep each file under the
// 300-line oxlint max-lines threshold while preserving a clear
// single-responsibility boundary (connection lifecycle vs. pool management).

export class SshConnectionManager {
  private connections = new Map<string, SshConnection>()
  private callbacks: SshConnectionCallbacks
  // Why: attempt identity lets disconnect unblock a replacement without the
  // cancelled attempt later clearing the replacement's state.
  private connectingTargets = new Map<string, symbol>()

  constructor(callbacks: SshConnectionCallbacks) {
    this.callbacks = callbacks
  }

  setCallbacks(callbacks: SshConnectionCallbacks): void {
    this.callbacks = callbacks
    for (const connection of this.connections.values()) {
      connection.setCallbacks(callbacks)
    }
  }

  async connect(target: SshTarget): Promise<SshConnection> {
    const existing = this.connections.get(target.id)
    // Why: only reuse when the pooled connection is bound to the SAME endpoint;
    // updateTarget() mutates host/port under a stable id, so a matching id with
    // a changed endpoint must rebuild rather than serve the pre-edit host.
    if (existing?.getState().status === 'connected' && existing.matchesTarget(target)) {
      return existing
    }

    if (this.connectingTargets.has(target.id)) {
      throw new Error(`Connection to ${target.label} is already in progress`)
    }

    const attempt = Symbol(target.id)
    this.connectingTargets.set(target.id, attempt)

    try {
      if (existing) {
        await existing.disconnect()
        // Why: a concurrent disconnect()/disconnectAll() invalidates this attempt
        // (clearing connectingTargets); bail before re-inserting so the user's
        // teardown is not silently resurrected as a zombie connection.
        if (this.connectingTargets.get(target.id) !== attempt) {
          throw this.createSupersededError(target)
        }
      }

      const conn = new SshConnection(target, this.callbacks)
      this.connections.set(target.id, conn)

      try {
        await conn.connect()
      } catch (err) {
        if (this.connections.get(target.id) === conn) {
          this.connections.delete(target.id)
        }
        throw err
      }

      // Why: a disconnect that raced this connect() succeeding must win —
      // tear the fresh connection down instead of leaving it live under a
      // torn-down id.
      if (this.connectingTargets.get(target.id) !== attempt) {
        if (this.connections.get(target.id) === conn) {
          this.connections.delete(target.id)
        }
        await conn.disconnect()
        throw this.createSupersededError(target)
      }

      return conn
    } finally {
      if (this.connectingTargets.get(target.id) === attempt) {
        this.connectingTargets.delete(target.id)
      }
    }
  }

  private createSupersededError(target: SshTarget): Error {
    return new Error(`Connection to ${target.label} was superseded by a disconnect`)
  }

  async disconnect(targetId: string): Promise<void> {
    // Why: disconnect invalidates the old attempt immediately so a reconnect
    // need not wait for the cancelled socket's late completion.
    this.connectingTargets.delete(targetId)
    const conn = this.connections.get(targetId)
    if (!conn) {
      return
    }
    await conn.disconnect()
    if (this.connections.get(targetId) === conn) {
      this.connections.delete(targetId)
    }
  }

  async reconnect(targetId: string): Promise<void> {
    const conn = this.connections.get(targetId)
    if (!conn) {
      return
    }
    await conn.reconnect()
  }

  getConnection(targetId: string): SshConnection | undefined {
    return this.connections.get(targetId)
  }

  getState(targetId: string): SshConnectionState | null {
    return this.connections.get(targetId)?.getState() ?? null
  }

  getAllStates(): Map<string, SshConnectionState> {
    const states = new Map<string, SshConnectionState>()
    for (const [id, conn] of this.connections) {
      states.set(id, conn.getState())
    }
    return states
  }

  async disconnectAll(): Promise<void> {
    // Why: invalidate every in-flight connect() attempt so one suspended mid
    // teardown cannot re-insert a live connection into the just-cleared pool.
    this.connectingTargets.clear()
    const disconnects = Array.from(this.connections.values()).map((c) => c.disconnect())
    await Promise.allSettled(disconnects)
    this.connections.clear()
  }
}
