import { mkdir, mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { getAppImageCliArgs, maybeRedirectAppImageCliLaunch } from './appimage-cli-redirect'

const commandNames = ['status', 'terminal']

describe('AppImage CLI redirect', () => {
  it('detects direct AppImage CLI commands', () => {
    expect(
      getAppImageCliArgs(
        ['orca-linux.AppImage', 'status', '--json'],
        { APPIMAGE: '/opt/orca' },
        {
          platform: 'linux',
          isPackaged: true,
          commandNames
        }
      )
    ).toEqual(['status', '--json'])
  })

  it('allows CLI global flags before the command', () => {
    expect(
      getAppImageCliArgs(
        ['orca-linux.AppImage', '--pairing-code', 'abc123', '--json', 'terminal', 'list'],
        {
          APPIMAGE: '/opt/orca'
        },
        {
          platform: 'linux',
          isPackaged: true,
          commandNames
        }
      )
    ).toEqual(['--pairing-code', 'abc123', '--json', 'terminal', 'list'])
  })

  it('does not redirect normal desktop AppImage launches', () => {
    expect(
      getAppImageCliArgs(
        ['AppRun', '--no-sandbox', 'file:///tmp/example.txt'],
        {
          APPIMAGE: '/opt/orca'
        },
        {
          platform: 'linux',
          isPackaged: true,
          commandNames
        }
      )
    ).toBeNull()
  })

  it('redirects a CLI command that carries a desktop flag, stripping the flag', () => {
    expect(
      getAppImageCliArgs(
        ['orca-linux.AppImage', 'serve', '--no-sandbox', '--json'],
        { APPIMAGE: '/opt/orca' },
        {
          platform: 'linux',
          isPackaged: true,
          commandNames: ['serve', 'status', 'terminal']
        }
      )
    ).toEqual(['serve', '--json'])
  })

  it('marks the child env when a redirected CLI launch disabled the sandbox', async () => {
    const root = await mkdtemp(join(tmpdir(), 'orca-appimage-cli-redirect-'))
    const cliEntryPath = join(root, 'app.asar.unpacked', 'out', 'cli', 'index.js')
    await mkdir(join(root, 'app.asar.unpacked', 'out', 'cli'), { recursive: true })
    await writeFile(cliEntryPath, '', 'utf8')
    const spawn = vi.fn((..._args: unknown[]) => ({ status: 0 }))

    maybeRedirectAppImageCliLaunch({
      argv: ['orca-linux.AppImage', 'serve', '--no-sandbox'],
      env: { APPIMAGE: '/opt/orca/orca-linux.AppImage' },
      platform: 'linux',
      isPackaged: true,
      resourcesPath: root,
      execPath: '/opt/orca/orca-ide',
      commandNames: ['serve'],
      spawn: spawn as never
    })

    expect(spawn).toHaveBeenCalledWith('/opt/orca/orca-ide', [cliEntryPath, 'serve'], {
      env: expect.objectContaining({ ORCA_APPIMAGE_NO_SANDBOX: '1' }),
      stdio: 'inherit'
    })
  })

  it('spawns the unpacked CLI entrypoint with Electron node mode', async () => {
    const root = await mkdtemp(join(tmpdir(), 'orca-appimage-cli-redirect-'))
    const cliEntryPath = join(root, 'app.asar.unpacked', 'out', 'cli', 'index.js')
    await mkdir(join(root, 'app.asar.unpacked', 'out', 'cli'), { recursive: true })
    await writeFile(cliEntryPath, '', 'utf8')
    const spawn = vi.fn((..._args: unknown[]) => ({ status: 0 }))

    const result = maybeRedirectAppImageCliLaunch({
      argv: ['orca-linux.AppImage', 'status', '--json'],
      env: {
        APPIMAGE: '/opt/orca/orca-linux.AppImage',
        NODE_OPTIONS: '--inspect',
        NODE_REPL_EXTERNAL_MODULE: '/tmp/repl.js'
      },
      platform: 'linux',
      isPackaged: true,
      resourcesPath: root,
      execPath: '/opt/orca/orca-ide',
      commandNames,
      spawn: spawn as never
    })

    expect(result).toEqual({ redirected: true, status: 0 })
    expect(spawn).toHaveBeenCalledWith('/opt/orca/orca-ide', [cliEntryPath, 'status', '--json'], {
      env: expect.objectContaining({
        APPIMAGE: '/opt/orca/orca-linux.AppImage',
        ELECTRON_RUN_AS_NODE: '1',
        ORCA_NODE_OPTIONS: '--inspect',
        ORCA_NODE_REPL_EXTERNAL_MODULE: '/tmp/repl.js'
      }),
      stdio: 'inherit'
    })
    const spawnOptions = spawn.mock.calls[0]?.[2] as { env: NodeJS.ProcessEnv } | undefined
    expect(spawnOptions?.env).not.toHaveProperty('NODE_OPTIONS')
    expect(spawnOptions?.env).not.toHaveProperty('NODE_REPL_EXTERNAL_MODULE')
  })
})
