import type {
  TerminalScrollBufferTarget,
  TerminalScrollBufferType
} from './terminal-scroll-buffer-snapshot'

export type TerminalScrollIntentKind = 'followOutput' | 'pinnedViewport'

export type TerminalScrollIntentTarget = TerminalScrollBufferTarget & {
  scrollToBottom?: () => void
  scrollToLine?: (line: number) => void
}

export type TerminalScrollIntentKey = string

export type TerminalStructuralScrollIntentSnapshot = {
  kind: TerminalScrollIntentKind
  bufferType: TerminalScrollBufferType
  viewportY: number
  baseY: number
  revision: number
}

export type TerminalScrollIntentEnforceOptions = {
  // Absolute lines are stable while content grows; a rebuild instead restores
  // the captured distance from the live bottom after rows are renumbered.
  restoreBy?: 'viewportLine' | 'bottomOffset'
  // Worker snapshots can lag a just-scrolled engine, so resume enforcement may
  // need to post the corrective scroll even when the snapshot reads the target.
  forceScroll?: boolean
}
