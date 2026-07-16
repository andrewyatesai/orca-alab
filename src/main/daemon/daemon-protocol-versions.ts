// Daemon protocol version constants — re-exported via ./types (the line-capped
// wire-shape entry point), so importers keep one types entry point.

// Why: daemons can survive app updates. Bump for IPC wire-shape changes, or
// when daemon-baked behavior cannot be delivered by on-disk wrapper refresh.
// Why: bump when adding daemon wire behavior so same-version old daemons do
// not silently accept the handshake and then reject new RPCs.
// Why 10xx: the fork reserves the 1000+ namespace. Socket/token/pid names key
// off this number (daemon-spawner.ts), so the fork's Rust daemon and any
// public Orca install (v18–v22) get disjoint endpoints — a public build can
// never adopt the fork daemon after a downgrade, and the fork never
// impersonates the public Node daemon at its socket. Must equal
// PROTOCOL_VERSION in rust/crates/orca-daemon/src/protocol.rs.
// Why 1019: adds the read-only SUBSCRIBER role (subscribe/unsubscribe +
// output fan-out; see daemon-subscriber-protocol.ts). Additive only.
// Why 1020: adds the OPT-IN binary stream plane (streamFormat:'binary' on the
// stream hello; see daemon-binary-stream-protocol.ts). Additive only — the
// Rust daemon still accepts a 1018 hello, a hello that doesn't request binary
// keeps NDJSON, and a preserved older daemon keeps working via the
// legacy-adapter path (1018/1019 listed as previous versions below), so
// nothing on the TS side requires 1020.
export const PROTOCOL_VERSION = 1020

// The protocol version at which the binary stream plane exists. A client only
// requests it when negotiating at >= this version; the daemon only grants at
// >=. Must equal BINARY_STREAM_PROTOCOL_VERSION in
// rust/crates/orca-daemon/src/protocol.rs.
export const BINARY_STREAM_PROTOCOL_VERSION = 1020

// Fork daemon protocol versions live at 1000+; public Orca versions sit below.
// Gates that mean "an attached PUBLIC daemon" (not just "not current") must
// compare against this boundary, or a preserved fork daemon one rev behind
// would satisfy them (see daemon-pty-adapter.ts).
export const FORK_DAEMON_PROTOCOL_NAMESPACE_START = 1000

// Min attached-daemon protocol that implements the git-credential-guard HOST
// compose (upstream #7986). Only a public Node daemon at this version (or newer)
// completes the deferred git-config; the fork's own Rust daemon passes env
// verbatim, so daemon-pty-adapter gates the fork daemon out (see
// supportsGitCredentialGuardHost).
export const GIT_CREDENTIAL_GUARD_HOST_PROTOCOL_VERSION = 22

// Why 18–22 are listed: a live public Node daemon (with running agent
// sessions) found at daemon-v18..v22.* is attached via the legacy-adapter path
// instead of being killed or impersonated, so installing the fork over public
// Orca preserves in-flight terminals across the public protocol range (upstream
// v1.4.142 ships public protocol 22).
// Why 1018/1019 are listed: a fork daemon preserved across an app update to
// 1020 keeps its sessions via the same legacy-adapter path (it lives at
// daemon-v1018.* / daemon-v1019.*).
// prettier-ignore
export const PREVIOUS_DAEMON_PROTOCOL_VERSIONS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 1018, 1019] as const
