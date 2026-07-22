import type { WorkerTerminal } from './aterm-worker-terminal'
import type {
  AtermWorkerScrollLines,
  AtermWorkerScrollPx,
  AtermWorkerScrollToBottom,
  AtermWorkerScrollToLine,
  AtermWorkerScrollToTop
} from './aterm-render-worker-protocol'

/** The scrollback-viewport subset of the pane commands. */
export type AtermWorkerScrollCommand =
  | AtermWorkerScrollLines
  | AtermWorkerScrollPx
  | AtermWorkerScrollToBottom
  | AtermWorkerScrollToTop
  | AtermWorkerScrollToLine

/** Handle the scrollback-viewport subset outside the already-large pane dispatcher
 *  (rain-dispatch precedent). All five repaint via a STATE-posting draw, even with
 *  no engine yet — same no-op-safely contract as the single-engine worker. */
export function dispatchAtermWorkerScrollCommand(
  term: WorkerTerminal | null,
  scheduleDraw: () => void,
  msg: AtermWorkerScrollCommand
): void {
  switch (msg.type) {
    case 'scrollLines':
      term?.scrollLines(msg.delta)
      break
    case 'scrollPx':
      term?.scrollPx(msg.deltaPx)
      break
    case 'scrollToBottom':
      term?.scrollToBottom()
      break
    case 'scrollToTop':
      term?.scrollToTop()
      break
    case 'scrollToLine':
      term?.scrollToLine(msg.line)
  }
  scheduleDraw()
}
