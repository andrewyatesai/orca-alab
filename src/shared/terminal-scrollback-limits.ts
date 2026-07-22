export const TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT = 512 * 1024
// Why 512KB stays: this bounds the SYNCHRONOUS (renderer-blocking) tail read at
// mount; the rest of the store is hydrated afterwards via async chunk streaming
// (see terminal-scrollback-deep-restore.ts), never as one sync 5MB replay.
export const TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT = 512 * 1024
export const TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT = 5 * 1024 * 1024
/** Total bytes a restore may replay once async deep hydration completes. */
export const TERMINAL_SCROLLBACK_DEEP_REPLAY_BYTE_LIMIT = TERMINAL_SCROLLBACK_STORE_BYTE_LIMIT
/** Per-request cap for the async older-history chunk reads (bounds each main-process read and IPC payload). */
export const TERMINAL_SCROLLBACK_OLDER_CHUNK_BYTE_LIMIT = 512 * 1024
