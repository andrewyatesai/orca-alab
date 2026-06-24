// aterm is the authoritative terminal query responder: after each chunk of PTY
// output is processed, the engine may have queued replies (DA1/DA2/DSR/CPR/DECRQM/
// OSC colour/CSI 14t-16t). We drain them and forward to the PTY via the input sink
// — which routes through the same input-injection flag + onData replay/presence
// guards as keystrokes. The xterm shim's own auto-replies are dropped for aterm
// panes (see pty-connection), so this is the single source of replies.

/** Drain + forward the engine's pending query replies. Reply bytes are ASCII, so
 *  latin1 decode (char code === byte) preserves them exactly for the PTY write. */
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
    out += String.fromCharCode(reply[i])
  }
  inputSink(out)
}
