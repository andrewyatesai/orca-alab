// aterm is the authoritative terminal query responder: after each chunk of PTY
// output is processed, the engine may have queued replies (DA1/DA2/DSR/CPR/DECRQM/
// OSC colour/CSI 14t-16t). We drain them and forward to the PTY via the input sink
// — which routes through the same input-injection flag + onData replay/presence
// guards as keystrokes. The xterm shim's own auto-replies are dropped for aterm
// panes (see pty-connection), so this is the single source of replies.

/** Drain + forward the engine's pending query replies. The replies aterm emits
 *  (DA1/DA2/DSR/CPR/DECRQM/OSC colour/XTVERSION/DECRQSS) are all ASCII, so latin1
 *  decode (char code === byte) preserves them exactly for the PTY write. The only
 *  reply path that could carry non-ASCII — XTWINOPS title reports — requires
 *  `allow_window_ops` (off by default), so the ASCII invariant holds in practice;
 *  any byte ≥ 0x80 is dropped here rather than letting the UTF-8 PTY stream
 *  re-encode (and corrupt) it. */
export function drainAtermReplies(
  term: { take_response: () => Uint8Array | undefined },
  inputSink: (data: string) => void
): void {
  const reply = term.take_response()
  if (!reply || reply.length === 0) {
    return
  }
  let out = ''
  for (let i = 0; i < reply.length; i++) {
    // Guard the ASCII invariant: a stray non-ASCII byte would be re-encoded by the
    // UTF-8 PTY write into different bytes, so skip it rather than corrupt the reply.
    if (reply[i] < 0x80) {
      out += String.fromCharCode(reply[i])
    }
  }
  inputSink(out)
}
