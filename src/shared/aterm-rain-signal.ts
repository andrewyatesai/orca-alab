export const ATERM_RAIN_SIGNAL_CODES = {
  assistant: 0,
  inspect: 1,
  modify: 2,
  execute: 3,
  network: 4,
  branch: 5,
  waiting: 6,
  success: 7,
  failure: 8,
  interrupted: 9,
  turn_start: 10
} as const

export type AtermRainSignal = keyof typeof ATERM_RAIN_SIGNAL_CODES

export type AtermRainPulse = {
  signal: AtermRainSignal
  weight: number
}

export type AtermRainPulseBuffer = Readonly<{
  strongest: AtermRainPulse | null
  turnStart: AtermRainPulse | null
}>

export const EMPTY_ATERM_RAIN_PULSE_BUFFER: AtermRainPulseBuffer = {
  strongest: null,
  turnStart: null
}

const RAIN_SIGNAL_PRIORITY: Record<AtermRainSignal, number> = {
  assistant: 0,
  turn_start: 0,
  inspect: 1,
  modify: 1,
  execute: 1,
  network: 1,
  branch: 1,
  waiting: 2,
  success: 2,
  failure: 3,
  interrupted: 3
}

/** Match the engine's allocation-free pending-slot semantics while a renderer
 * swaps: stronger outcomes survive lower-priority work, with no event queue. */
export function coalesceAtermRainPulse(
  current: AtermRainPulse | null,
  next: AtermRainPulse
): AtermRainPulse {
  if (!current || RAIN_SIGNAL_PRIORITY[current.signal] <= RAIN_SIGNAL_PRIORITY[next.signal]) {
    return next
  }
  return { signal: current.signal, weight: Math.max(current.weight, next.weight) }
}

/** Buffer the engine's two independent pending facts across a renderer swap:
 * one strongest phase/outcome plus the latest turn boundary. */
export function bufferAtermRainPulse(
  current: AtermRainPulseBuffer,
  next: AtermRainPulse
): AtermRainPulseBuffer {
  return {
    strongest: coalesceAtermRainPulse(current.strongest, next),
    turnStart: next.signal === 'turn_start' ? next : current.turnStart
  }
}

/** Restore a turn boundary before the strongest coalesced phase. The array is
 * bounded to two entries and allocated only on the rare renderer-recovery path. */
export function resumeAtermRainPulses(buffered: AtermRainPulseBuffer): AtermRainPulse[] {
  if (!buffered.strongest) {
    return []
  }
  if (!buffered.turnStart || buffered.strongest.signal === 'turn_start') {
    return [buffered.strongest]
  }
  return [buffered.turnStart, buffered.strongest]
}

type ObservableAgentState = 'working' | 'blocked' | 'waiting' | 'done'

const INSPECT_TOOLS = new Set([
  'read',
  'readfile',
  'grep',
  'glob',
  'search',
  'find',
  'list',
  'ls',
  'view',
  'inspect',
  'explore'
])
const MODIFY_TOOLS = new Set([
  'edit',
  'write',
  'writefile',
  'patch',
  'applypatch',
  'multiedit',
  'notebookedit',
  'create'
])
const EXECUTE_TOOLS = new Set([
  'bash',
  'shell',
  'shellcommand',
  'exec',
  'execcommand',
  'execute',
  'executecode',
  'run',
  'runcommand',
  'runshellcommand',
  'runterminalcmd',
  'terminal',
  'test'
])
const NETWORK_TOOLS = new Set(['webfetch', 'websearch', 'fetch', 'browser', 'http', 'curl', 'wget'])
const BRANCH_TOOLS = new Set(['task', 'agent', 'subagent', 'dispatch', 'spawn', 'delegate'])

function compactIdentifier(value: string | undefined): string {
  return value?.replace(/[^a-z]/gi, '').toLowerCase() ?? ''
}

/** Convert normalized, observable hook metadata into a payload-free visual pulse.
 *  Prompt text, tool input, paths, URLs, assistant text, and network data are
 *  intentionally absent from this interface. */
export function classifyAtermRainPulse(input: {
  state: ObservableAgentState
  hookEventName?: string
  toolName?: string
  interrupted?: boolean
}): AtermRainPulse {
  if (input.interrupted) {
    return { signal: 'interrupted', weight: 7 }
  }
  const event = compactIdentifier(input.hookEventName)
  if (/(error|fail)/.test(event)) {
    return { signal: 'failure', weight: 8 }
  }
  if (input.state === 'done') {
    return { signal: 'success', weight: 6 }
  }
  if (input.state === 'waiting' || input.state === 'blocked') {
    return { signal: 'waiting', weight: 2 }
  }

  const tool = compactIdentifier(input.toolName)
  if (NETWORK_TOOLS.has(tool)) {
    return { signal: 'network', weight: 5 }
  }
  if (BRANCH_TOOLS.has(tool)) {
    return { signal: 'branch', weight: 6 }
  }
  if (MODIFY_TOOLS.has(tool)) {
    return { signal: 'modify', weight: 5 }
  }
  if (INSPECT_TOOLS.has(tool)) {
    return { signal: 'inspect', weight: 3 }
  }
  if (EXECUTE_TOOLS.has(tool)) {
    return { signal: 'execute', weight: 6 }
  }
  if (/^(userpromptsubmit|turnstart|agentstart|prellmcall)$/.test(event)) {
    return { signal: 'turn_start', weight: 4 }
  }
  return { signal: 'assistant', weight: 2 }
}
