import { describe, expect, it } from 'vitest'
import { join } from 'node:path'
import { isPackagedElectronProcess, orcaNodeAddonCandidatePaths } from './orca-node-addon-paths'

const cwd = join('/repo', 'checkout')
const resourcesPath = join('/opt', 'Orca', 'resources')
const devAddon = join(cwd, 'native', 'orca-node', 'orca_node.node')
const packagedAddon = join(resourcesPath, 'orca_node.node')

describe('orcaNodeAddonCandidatePaths', () => {
  it('dev: probes override, then the cwd dev build, then resourcesPath', () => {
    expect(
      orcaNodeAddonCandidatePaths({
        override: '/tmp/override.node',
        isPackaged: false,
        cwd,
        resourcesPath
      })
    ).toEqual(['/tmp/override.node', devAddon, packagedAddon])
  })

  it('packaged: never probes cwd — a stale dev addon under the launch directory must not shadow the shipped engine', () => {
    const paths = orcaNodeAddonCandidatePaths({ isPackaged: true, cwd, resourcesPath })
    expect(paths).toEqual([packagedAddon])
  })

  it('packaged: still honors the explicit env override ahead of resourcesPath', () => {
    expect(
      orcaNodeAddonCandidatePaths({
        override: '/tmp/override.node',
        isPackaged: true,
        cwd,
        resourcesPath
      })
    ).toEqual(['/tmp/override.node', packagedAddon])
  })

  it('plain Node (no resourcesPath): probes only the cwd dev build', () => {
    expect(orcaNodeAddonCandidatePaths({ isPackaged: false, cwd })).toEqual([devAddon])
  })
})

describe('isPackagedElectronProcess', () => {
  it('is false outside Electron (plain-Node test run)', () => {
    expect(isPackagedElectronProcess()).toBe(false)
  })
})
