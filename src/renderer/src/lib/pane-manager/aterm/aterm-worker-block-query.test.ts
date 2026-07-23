// CM-A3: the context menu's Copy Last Command Output reads the engine's newest
// row-sealed OSC-133 block through a feature-detected binding — worker panes ride
// the id-correlated query channel, in-process panes read the wasm export directly.

import { describe, expect, it, vi } from 'vitest'
import { createAtermWorkerQueryChannel } from './aterm-worker-query-channel'
import { answerWorkerTerminalQuery } from './aterm-worker-terminal-query'
import {
  buildAtermLastCommandOutputMember,
  parseAtermLastCommandOutput
} from './aterm-last-command-output'
import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { AtermTerminal } from './aterm_wasm.js'

type QueryCommand = Extract<AtermWorkerPaneCommand, { type: 'query' }>

function captureQueries(): {
  channel: ReturnType<typeof createAtermWorkerQueryChannel>
  posted: QueryCommand[]
} {
  const posted: QueryCommand[] = []
  const channel = createAtermWorkerQueryChannel((cmd) => {
    if (cmd.type === 'query') {
      posted.push(cmd)
    }
  })
  return { channel, posted }
}

describe('lastCommandOutput worker query (CM-A3)', () => {
  it('lastCommandOutputAsync round-trips the worker and preserves the evicted marker', async () => {
    const { channel, posted } = captureQueries()
    const result = channel.lastCommandOutputAsync()

    const query = posted.find((cmd) => cmd.kind === 'lastCommandOutput')
    expect(query).toBeDefined()
    // The worker answers with the engine's JSON drain — the eviction marker must
    // survive the round-trip verbatim so the menu can surface it honestly.
    channel.resolve(query!.id, '{"status":"evicted"}')

    expect(parseAtermLastCommandOutput(await result)).toEqual({ status: 'evicted' })
  })

  it('resolves null (not a phantom block) when the worker reports none', async () => {
    const { channel, posted } = captureQueries()
    const result = channel.lastCommandOutputAsync()
    channel.resolve(posted.find((cmd) => cmd.kind === 'lastCommandOutput')!.id, null)
    expect(await result).toBeNull()
  })

  it('answerWorkerTerminalQuery serves the binding and degrades to null without it', () => {
    const withBinding = {
      last_command_output: () => '{"status":"ok","text":"out\\n","exitCode":0}'
    } as unknown as EngineHandle['engine']
    expect(answerWorkerTerminalQuery(withBinding, 'lastCommandOutput', undefined, undefined)).toBe(
      '{"status":"ok","text":"out\\n","exitCode":0}'
    )
    // Older pins / the GPU module lack the export — the query must not throw.
    const withoutBinding = {} as unknown as EngineHandle['engine']
    expect(
      answerWorkerTerminalQuery(withoutBinding, 'lastCommandOutput', undefined, undefined)
    ).toBeNull()
  })
})

describe('parseAtermLastCommandOutput', () => {
  it('parses the ok shape and coalesces a missing exitCode to null', () => {
    expect(parseAtermLastCommandOutput('{"status":"ok","text":"done","exitCode":2}')).toEqual({
      status: 'ok',
      text: 'done',
      exitCode: 2
    })
    expect(parseAtermLastCommandOutput('{"status":"ok","text":""}')).toEqual({
      status: 'ok',
      text: '',
      exitCode: null
    })
  })

  it('returns null for absent, malformed, and unknown-status payloads', () => {
    expect(parseAtermLastCommandOutput(undefined)).toBeNull()
    expect(parseAtermLastCommandOutput(null)).toBeNull()
    expect(parseAtermLastCommandOutput('not json')).toBeNull()
    expect(parseAtermLastCommandOutput('{"status":"???"}')).toBeNull()
    // ok without text is malformed — never surface a copy with undefined text.
    expect(parseAtermLastCommandOutput('{"status":"ok"}')).toBeNull()
  })
})

describe('buildAtermLastCommandOutputMember', () => {
  it('prefers the worker facade (the sync snapshot cannot read blocks)', async () => {
    const syncBinding = vi.fn(() => '{"status":"ok","text":"stale","exitCode":0}')
    const term = {
      last_command_output: syncBinding,
      lastCommandOutputAsync: () => Promise.resolve('{"status":"ok","text":"fresh","exitCode":0}')
    } as unknown as AtermTerminal
    expect(await buildAtermLastCommandOutputMember(term)()).toEqual({
      status: 'ok',
      text: 'fresh',
      exitCode: 0
    })
    expect(syncBinding).not.toHaveBeenCalled()
  })

  it('reads the engine binding in-process, and resolves null when the pin lacks it', async () => {
    const inProcess = {
      last_command_output: () => '{"status":"ok","text":"out","exitCode":1}'
    } as unknown as AtermTerminal
    expect(await buildAtermLastCommandOutputMember(inProcess)()).toEqual({
      status: 'ok',
      text: 'out',
      exitCode: 1
    })
    // Feature-detect: no worker facade, no binding — the menu item stays hidden.
    expect(await buildAtermLastCommandOutputMember({} as unknown as AtermTerminal)()).toBeNull()
  })
})
