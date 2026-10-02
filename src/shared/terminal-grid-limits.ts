/**
 * The largest terminal grid dimension (rows, and separately cols): 4096 x 4096
 * = 16.7M cells. The Rust daemon (rust/crates/orca-daemon/src/rpc.rs
 * `MAX_GRID_DIM`) REFUSES a createOrAttach spawn or a resize above it with
 * `invalid terminal size: <cols|rows> <n> exceeds the maximum of 4096`, so
 * every client that sends sizes caps them here first and never trips it.
 *
 * Decision 2026-10-02 (made by the orchestrating agent under the owner's
 * "decide for yourself" instruction): refuse at the daemon, clamp at the
 * client. A 4096-column pane needs a 4096-cell-wide canvas — beyond any real
 * display at any usable font size — so the clamp changes nothing a user can see.
 */
export const TERMINAL_GRID_MAX_DIM = 4096

/** Cap one grid dimension at `TERMINAL_GRID_MAX_DIM`. Everything else —
 *  including NaN, ±Infinity and sub-1 values, which the size normalizers
 *  already map to 80x24 / 1 — passes through unchanged. */
export function capTerminalGridDim(value: number): number {
  return Number.isFinite(value) && value > TERMINAL_GRID_MAX_DIM ? TERMINAL_GRID_MAX_DIM : value
}
