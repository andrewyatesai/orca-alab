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

  it('separates project location from codebase method', () => {
    const html = renderToStaticMarkup(<AddProjectWorkflowVisual />)

    expect(html).toContain('1 · Location')
    expect(html).toContain('This computer')
    expect(html).toContain('Native host or WSL on Windows')
    expect(html).toContain('SSH host')
    expect(html).toContain('Paired Orca runtime')
    expect(html).toContain('2 · Codebase')
    expect(html).toContain('Open existing folder')
    expect(html).toContain('Clone repository')
    expect(html).toContain('Create project')
    expect(html).not.toContain('lucide-chevron-right')
  })

  it('connects the full desktop client and mobile companion through the runtime', () => {
    const html = renderToStaticMarkup(<RemoteMobileWorkflowVisual />)

    expect(html).toContain('Full client')
    expect(html).toContain('Companion')
    expect(html).toContain('Orca desktop')
    expect(html).toContain('Orca Mobile')
    expect(html).toContain('Orca runtime')
    expect(html).toContain('Execution + session authority')
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

  it('separates the local iOS pane from cross-platform Android control', () => {
    const html = renderToStaticMarkup(<MobileEmulatorsWorkflowVisual />)

    expect(html).toContain('Local Mac · Xcode required')
    expect(html).toContain('Live Orca emulator pane')
    expect(html).toContain('orca-emulator')
    expect(html).toContain('macOS · Linux · Windows')
    expect(html).toContain('Visible Android emulator window')
    expect(html).toContain('orca-emulator-android')
    expect(html).toContain('Retry with explicit device ID')
    expect(html).toContain('iOS control stays on the Mac that owns Simulator')
  })
})
