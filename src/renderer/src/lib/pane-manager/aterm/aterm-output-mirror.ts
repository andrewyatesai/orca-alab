/** The output scheduler is keyed by the pane's terminal object and has no pane
 *  reference. Under the aterm facade, that terminal IS the facade and exposes a
 *  `__feedEngine(data)` entry point that processes PTY bytes through the engine
 *  (buffering until the async controller attaches). Feeding here — up front, in
 *  arrival order, before the scheduler queues/coalesces/drops anything — keeps the
 *  engine in sync with the PTY even when the scheduler drops a hidden pane's
 *  backlog. */
type AtermEngineFeed = { __feedEngine?: (data: string) => void }

export function mirrorOutputToAterm(terminal: object, data: string): void {
  ;(terminal as AtermEngineFeed).__feedEngine?.(data)
}
