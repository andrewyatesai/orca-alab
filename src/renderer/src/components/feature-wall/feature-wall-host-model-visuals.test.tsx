import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ComputerUseWorkflowVisual, RemoteMobileWorkflowVisual } from './AnywhereWorkflowVisuals'
import { MobileEmulatorsWorkflowVisual } from './MobileEmulatorsWorkflowVisual'
import {
  AddProjectWorkflowVisual,
  TerminalFirstWorkflowVisual
} from './TerminalProjectWorkflowVisuals'

describe('feature wall host-model visuals', () => {
  it('keeps the terminal preview scoped to the active workspace', () => {
    const html = renderToStaticMarkup(<TerminalFirstWorkflowVisual />)

    expect(html).toContain('Persistent shell session ready')
    expect(html).toContain('Ready in the active workspace terminal.')
    expect(html).toContain('Warm restart')
    expect(html).toContain('Live process reattaches')
    expect(html).toContain('Host reboot')
    expect(html).toContain('Layout + scrollback restore')
    expect(html).toContain('aterm · Rust')
    expect(html).not.toContain('Last login')
    expect(html).not.toContain('>$</span>')
    expect(html).not.toContain('before you choose a project')
  })

  it('shows the explicit add-project, workspace-creation, setup, and ready result', () => {
    const html = renderToStaticMarkup(<AddProjectWorkflowVisual />)
    const actionIndex = html.indexOf('data-add-project-story-stage="action"')
    const progressIndex = html.indexOf('data-add-project-story-stage="progress"')
    const resultIndex = html.indexOf('data-add-project-story-stage="result"')

    expect(html).toContain('Illustrative action → progress → result')
    expect(html).toContain('1 · Execution host')
    expect(html).toContain('This computer')
    expect(html).toContain('Native host or WSL on Windows')
    expect(html).toContain('SSH host')
    expect(html).toContain('Paired Orca runtime')
    expect(html).toContain('2 · Codebase path')
    expect(html).toContain('Open existing folder')
    expect(html).toContain('Clone repository')
    expect(html).toContain('Create project')
    expect(html.match(/data-selected="true"/g)).toHaveLength(2)
    expect(html).toContain('User confirms · Add this project')
    expect(html).toContain('Project added')
    expect(html).toContain('Existing checkout · branch unchanged')
    expect(html).toContain('User action · Create workspace')
    expect(html).toContain('New Git worktree')
    expect(html).toContain('Approved shared setup runs')
    expect(html).toContain('orca.yaml scripts.setup · pnpm install')
    expect(html).toContain('Workspace ready')
    expect(html).toContain('Terminal open · repository setup complete')
    expect(html).toContain('only after the repository command content is approved')
    expect(html).toContain('changes require re-review')
    expect(actionIndex).toBeGreaterThan(-1)
    expect(progressIndex).toBeGreaterThan(actionIndex)
    expect(resultIndex).toBeGreaterThan(progressIndex)
    expect(html).not.toMatch(/text-\[(?:9|10)(?:\.5)?px\]/)
  })

  it('connects the full desktop client and mobile companion through the runtime', () => {
    const html = renderToStaticMarkup(<RemoteMobileWorkflowVisual />)

    expect(html).toContain('Full client')
    expect(html).toContain('Companion')
    expect(html).toContain('Orca desktop')
    expect(html).toContain('Orca Mobile')
    expect(html).toContain('Orca runtime')
    expect(html).toContain('Coordination + session authority')
    expect(html).toContain('data-feature-wall-mobile-beta="true"')
    expect(html).toContain('Beta')
    expect(html).toContain('Orca desktop')
    expect(html).toContain('Choose and guide work')
    expect(html).toContain('Optional downstream')
    expect(html).toContain('SSH project host')
    expect(html).toContain('Operational SSH path')
    expect(html).toContain('Terminal')
    expect(html).toContain('Git')
    expect(html).toContain('Files')
    expect(html).toContain('Forwarded port :3000')
    expect(html).toContain('Preview in the Orca browser')
    expect(html).toContain('SSH disconnects')
    expect(html).toContain('Reconnect + recover')
    expect(html).toContain('SSH owner retained')
    expect(html).toContain('saved port forwards return after reconnect')
    expect(html).toContain('never as local execution')
    expect(html).toContain('Workspace environment')
    expect(html).toContain('Provisioned from orca.yaml')
    expect(html).toContain('Quick Commands')
    expect(html).not.toContain('SSH / remote')
  })

  it('shows cross-platform Computer Use boundaries with a macOS-only permission note', () => {
    const html = renderToStaticMarkup(<ComputerUseWorkflowVisual />)

    expect(html).toContain('Capabilities checked')
    expect(html).toContain('Native helper + advertised actions')
    expect(html).toContain('macOS only · Accessibility + Screen Recording')
    expect(html).toContain('Visible app scope')
    expect(html).toContain('Advertised actions only')
    expect(html).not.toContain('macOS permissions')
  })

  it('connects an inspected Computer Use action to its visible result', () => {
    const html = renderToStaticMarkup(<ComputerUseWorkflowVisual />)
    const inspectIndex = html.indexOf('data-computer-use-stage="inspect"')
    const invokeIndex = html.indexOf('data-computer-use-action="reconnect"')
    const resultIndex = html.indexOf('data-computer-use-result="agent-connected"')

    expect(html).toContain('data-computer-use-flow="inspect-invoke-result"')
    expect(html).toContain('Invoke advertised action')
    expect(html).toContain('button “Reconnect”')
    expect(html).toContain('Visible result')
    expect(html).toContain('Agent connected')
    expect(inspectIndex).toBeGreaterThan(-1)
    expect(invokeIndex).toBeGreaterThan(inspectIndex)
    expect(resultIndex).toBeGreaterThan(invokeIndex)
  })

  it('separates the local iOS pane from cross-platform Android control', () => {
    const html = renderToStaticMarkup(<MobileEmulatorsWorkflowVisual />)

    expect(html).toContain('Local Mac · Xcode required')
    expect(html).toContain('Live Orca emulator pane')
    expect(html).toContain('orca-emulator')
    expect(html).toContain('macOS · Linux · Windows')
    expect(html.match(/Live Orca emulator pane/g)).toHaveLength(2)
    expect(html).toContain('Stream, install, launch, inspect logs')
    expect(html).toContain('orca-emulator-android')
    expect(html).toContain('Retry with explicit device ID')
    expect(html).toContain('iOS control stays on the Mac that owns Simulator')
  })

  it('carries stale emulator recovery through action and verified app state', () => {
    const html = renderToStaticMarkup(<MobileEmulatorsWorkflowVisual />)
    const staleIndex = html.indexOf('data-emulator-recovery-stage="stale-target"')
    const retryIndex = html.indexOf('data-emulator-retry-device="emulator-5554"')
    const actionIndex = html.indexOf('data-emulator-action="tap-type"')
    const resultIndex = html.indexOf('data-emulator-result="profile-email-updated"')

    expect(html).toContain(
      'data-emulator-recovery-flow="stale-target-explicit-retry-action-verified"'
    )
    expect(html).toContain('No active or stale target')
    expect(html).toContain('Retry with explicit device ID')
    expect(html).toContain('Device connected')
    expect(html).toContain('Tap Email · type agent@example.com')
    expect(html).toContain('Verified app state')
    expect(html).toContain('Profile email · agent@example.com')
    expect(staleIndex).toBeGreaterThan(-1)
    expect(retryIndex).toBeGreaterThan(staleIndex)
    expect(actionIndex).toBeGreaterThan(retryIndex)
    expect(resultIndex).toBeGreaterThan(actionIndex)
  })
})
