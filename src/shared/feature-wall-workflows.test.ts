import { describe, expect, it } from 'vitest'
import {
  DEFAULT_FEATURE_WALL_STEP_ID,
  DEFAULT_FEATURE_WALL_WORKFLOW_ID,
  FEATURE_WALL_STEP_IDS,
  FEATURE_WALL_WORKFLOWS
} from './feature-wall-workflows'

describe('ALab feature wall catalog', () => {
  it('starts with the terminal and covers six lifecycle chapters', () => {
    expect(DEFAULT_FEATURE_WALL_WORKFLOW_ID).toBe('start')
    expect(DEFAULT_FEATURE_WALL_STEP_ID).toBe('terminal')
    expect(FEATURE_WALL_WORKFLOWS.map((workflow) => workflow.id)).toEqual([
      'start',
      'plan',
      'build',
      'ship',
      'scale',
      'anywhere'
    ])
  })

  it('defines fourteen unique, outcome-led screens', () => {
    expect(FEATURE_WALL_STEP_IDS).toHaveLength(14)
    expect(new Set(FEATURE_WALL_STEP_IDS)).toHaveLength(14)
    for (const workflow of FEATURE_WALL_WORKFLOWS) {
      expect(workflow.steps.length).toBeGreaterThan(0)
      for (const step of workflow.steps) {
        expect(step.title.trim().length).toBeGreaterThan(0)
        expect(step.description.trim().length).toBeGreaterThan(0)
      }
    }
  })

  it('labels Computer Use as beta without hiding supported platforms', () => {
    const computerUse = FEATURE_WALL_WORKFLOWS.flatMap((workflow) => workflow.steps).find(
      (step) => step.id === 'computer-use'
    )
    expect(computerUse?.availabilityLabel).toBe('Beta')
    expect(computerUse?.description).toContain('native helpers per platform')
    expect(computerUse?.description).toContain('On macOS, grant Accessibility and Screen Recording')
    expect(computerUse?.description).toContain('on every platform, check capabilities')
  })

  it('keeps host types, agent support, and per-step docs precise', () => {
    const steps = FEATURE_WALL_WORKFLOWS.flatMap((workflow) => workflow.steps)
    const terminal = steps.find((step) => step.id === 'terminal')
    const addProject = steps.find((step) => step.id === 'add-project')
    const workspaces = steps.find((step) => step.id === 'workspaces')
    const agents = steps.find((step) => step.id === 'agents')
    const browserDesign = steps.find((step) => step.id === 'browser-design')
    const cliSkills = steps.find((step) => step.id === 'cli-skills')
    const remoteMobile = steps.find((step) => step.id === 'remote-mobile')
    const mobileEmulators = steps.find((step) => step.id === 'mobile-emulators')
    const computerUse = steps.find((step) => step.id === 'computer-use')

    expect(terminal?.description).toContain('When a workspace is active')
    expect(terminal?.description).not.toContain('before you choose a project')
    expect(terminal?.description).toContain('after a host reboot')
    expect(addProject?.description).toContain('native or WSL')
    expect(addProject?.description).toContain('an SSH host, or a paired Orca runtime')
    expect(workspaces?.description).toContain('For a Git project')
    expect(workspaces?.description).toContain('Folder-only projects continue to share')
    expect(agents?.description).toContain('supported or custom terminal agents')
    expect(agents?.description).toContain('not a machine-security sandbox')
    expect(browserDesign?.description).toContain('DOM, CSS, source, and cropped screenshot')
    expect(cliSkills?.description).toContain('local, SSH, or paired runtime')
    expect(remoteMobile?.description).toContain('After one-time pairing')
    expect(remoteMobile?.description).toContain('desktop/runtime remains the source of truth')
    expect(remoteMobile?.availabilityLabel).toBe('Mobile beta')
    expect(remoteMobile?.docsUrl).toBe('https://www.onorca.dev/docs/mobile')
    expect(mobileEmulators?.description).toContain('workspace-scoped iOS Simulator pane')
    expect(mobileEmulators?.description).toContain('macOS, Linux, or Windows')
    expect(mobileEmulators?.description).toContain('iOS control is local to the Mac')
    expect(computerUse?.description).toContain('invoking advertised actions')
    expect(computerUse?.docsUrl).toBe('https://www.onorca.dev/docs/cli/computer-use')
    expect(steps.every((step) => step.docsUrl?.startsWith('https://www.onorca.dev/docs/'))).toBe(
      true
    )
  })
})
