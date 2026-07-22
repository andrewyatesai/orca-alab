import { ipcMain, shell, dialog } from 'electron'
import { spawn } from 'node:child_process'
import { readFile, stat } from 'node:fs/promises'
import { basename, extname, isAbsolute, normalize } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { Store } from '../persistence'
import type { ShellOpenLocalPathResult } from '../../shared/shell-open-types'
import { MAX_REPO_ICON_UPLOAD_BYTES } from '../../shared/repo-icon'
import { getSpawnArgsForWindows } from '../win32-utils'
import {
  EXTERNAL_EDITOR_CLI_COMMAND,
  resolveExternalEditorLaunchSpec
} from '../external-editor-launch'

export { EXTERNAL_EDITOR_CLI_COMMAND }

const REPO_ICON_IMAGE_MIME_TYPES: Record<string, string> = {
  '.png': 'image/png'
}

// Why: KeybindingsFileActions launches these two without a settings entry, so they stay trusted even when never configured.
const BUILT_IN_EDITOR_COMMANDS = new Set([EXTERNAL_EDITOR_CLI_COMMAND, 'cursor'])

function isTrustedExternalEditorCommand(command: string, store: Store): boolean {
  if (BUILT_IN_EDITOR_COMMANDS.has(command)) {
    return true
  }
  const applications = store.getSettings().openInApplications ?? []
  return applications.some((application) => application.command.trim() === command)
}

async function pathExists(pathValue: string): Promise<boolean> {
  try {
    await stat(pathValue)
    return true
  } catch {
    return false
  }
}

async function validateLocalPathTarget(
  pathValue: string
): Promise<{ ok: true; path: string } | { ok: false; reason: 'not-absolute' | 'not-found' }> {
  const normalizedPath = normalize(pathValue)
  if (!isAbsolute(normalizedPath)) {
    return { ok: false, reason: 'not-absolute' }
  }
  if (!(await pathExists(normalizedPath))) {
    return { ok: false, reason: 'not-found' }
  }
  return { ok: true, path: normalizedPath }
}

async function openInFileManager(pathValue: string): Promise<ShellOpenLocalPathResult> {
  const target = await validateLocalPathTarget(pathValue)
  if (!target.ok) {
    return target
  }
  try {
    // Why: the file-manager action uses reveal semantics, matching the
    // previous sidebar behavior while still validating the path per click.
    shell.showItemInFolder(target.path)
    return { ok: true }
  } catch {
    return { ok: false, reason: 'launch-failed' }
  }
}

async function launchExternalEditor(pathValue: string, command?: string): Promise<void> {
  const launchSpec = resolveExternalEditorLaunchSpec(command, pathValue)
  const { spawnCmd, spawnArgs } =
    launchSpec.kind === 'executable'
      ? getSpawnArgsForWindows(launchSpec.spawnCmd, launchSpec.spawnArgs)
      : { spawnCmd: launchSpec.spawnCmd, spawnArgs: launchSpec.spawnArgs }

  await new Promise<void>((resolvePromise, rejectPromise) => {
    const child = spawn(spawnCmd, spawnArgs, {
      detached: true,
      stdio: 'ignore',
      // Why: terminal editors such as nvim need a visible console on Windows;
      // GUI editor launches stay hidden to avoid command-shim flashes.
      windowsHide: launchSpec.hideWindowsConsole
    })
    let settled = false

    function cleanup(): void {
      child.off('error', onError)
      child.off('spawn', onSpawn)
    }

    function settle(callback: () => void): void {
      if (settled) {
        return
      }
      settled = true
      cleanup()
      callback()
    }

    function onError(error: Error): void {
      settle(() => rejectPromise(error))
    }

    function onSpawn(): void {
      child.unref()
      settle(resolvePromise)
    }
    child.once('error', onError)
    child.once('spawn', onSpawn)
  })
}

async function openInExternalEditor(
  pathValue: string,
  command?: string
): Promise<ShellOpenLocalPathResult> {
  const target = await validateLocalPathTarget(pathValue)
  if (!target.ok) {
    return target
  }
  try {
    await launchExternalEditor(target.path, command)
    return { ok: true }
  } catch {
    return { ok: false, reason: 'launch-failed' }
  }
}

async function openWithSystemDefault(pathValue: string): Promise<boolean> {
  const target = await validateLocalPathTarget(pathValue)
  if (!target.ok) {
    return false
  }
  try {
    const errorMessage = await shell.openPath(target.path)
    return errorMessage.length === 0
  } catch {
    return false
  }
}

export function registerShellHandlers(store: Store): void {
  ipcMain.handle('shell:openPath', async (_event, path: string): Promise<void> => {
    // Why: keep the legacy fire-and-forget renderer contract while reusing the
    // same absolute/existing path validation as the explicit file-manager API.
    void (await openInFileManager(path))
  })

  ipcMain.handle(
    'shell:openInFileManager',
    (_event, path: string): Promise<ShellOpenLocalPathResult> => openInFileManager(path)
  )

  ipcMain.handle(
    'shell:openInExternalEditor',
    (_event, path: string, command?: string): Promise<ShellOpenLocalPathResult> => {
      // Why: command reaches spawn() (compound commands run via `sh -c`), so only main-trusted
      // launchers may run — a raw renderer string would hand a compromised renderer arbitrary
      // process execution (same threat model as the fs:* path confinement).
      const trimmedCommand = command?.trim()
      if (trimmedCommand && !isTrustedExternalEditorCommand(trimmedCommand, store)) {
        return Promise.resolve({ ok: false, reason: 'launch-failed' })
      }
      return openInExternalEditor(path, command)
    }
  )

  ipcMain.handle('shell:openUrl', (_event, rawUrl: string) => {
    let parsed: URL
    try {
      parsed = new URL(rawUrl)
    } catch {
      return
    }

    if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') {
      return
    }

    return shell.openExternal(parsed.toString())
  })

  ipcMain.handle('shell:openFilePath', async (_event, filePath: string): Promise<boolean> => {
    return openWithSystemDefault(filePath)
  })

  ipcMain.handle('shell:openFileUri', async (_event, rawUri: string) => {
    let parsed: URL
    try {
      parsed = new URL(rawUri)
    } catch {
      return
    }

    if (parsed.protocol !== 'file:') {
      return
    }

    // Only local files are supported. Remote hosts are intentionally rejected.
    if (parsed.hostname && parsed.hostname !== 'localhost') {
      return
    }

    let filePath: string
    try {
      filePath = fileURLToPath(parsed)
    } catch {
      return
    }

    const target = await validateLocalPathTarget(filePath)
    if (!target.ok) {
      return
    }

    await openWithSystemDefault(target.path)
  })

  ipcMain.handle('shell:pathExists', async (_event, filePath: string): Promise<boolean> => {
    return pathExists(filePath)
  })

  ipcMain.handle(
    'shell:pickDirectory',
    async (_event, args: { defaultPath?: string }): Promise<string | null> => {
      const result = await dialog.showOpenDialog({
        defaultPath: args.defaultPath,
        // Why: callers only need an existing folder grant; enabling native
        // creation can leave typed prefix directories behind on macOS.
        properties: ['openDirectory']
      })
      if (result.canceled || result.filePaths.length === 0) {
        return null
      }
      return result.filePaths[0]
    }
  )

  // Why: window.prompt() and <input type="file"> are unreliable in Electron,
  // so we use the native OS dialog to let the user pick any attachment file.
  ipcMain.handle('shell:pickAttachment', async (): Promise<string | null> => {
    const result = await dialog.showOpenDialog({
      properties: ['openFile']
    })
    if (result.canceled || result.filePaths.length === 0) {
      return null
    }
    return result.filePaths[0]
  })

  // Why: window.prompt() and <input type="file"> are unreliable in Electron,
  // so we use the native OS dialog to let the user pick an image file.
  ipcMain.handle('shell:pickImage', async (): Promise<string | null> => {
    const result = await dialog.showOpenDialog({
      properties: ['openFile'],
      filters: [
        { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico'] }
      ]
    })
    if (result.canceled || result.filePaths.length === 0) {
      return null
    }
    return result.filePaths[0]
  })

  ipcMain.handle(
    'shell:pickRepoIconImage',
    async (): Promise<{ dataUrl: string; fileName: string } | null> => {
      const result = await dialog.showOpenDialog({
        properties: ['openFile'],
        filters: [{ name: 'Repo icon images', extensions: ['png'] }]
      })
      if (result.canceled || result.filePaths.length === 0) {
        return null
      }

      const filePath = result.filePaths[0]
      const extension = extname(filePath).toLowerCase()
      const mimeType = REPO_ICON_IMAGE_MIME_TYPES[extension]
      if (!mimeType) {
        throw new Error('Repo icons must be PNG files.')
      }

      const stats = await stat(filePath)
      if (stats.size > MAX_REPO_ICON_UPLOAD_BYTES) {
        throw new Error('Repo icon image must be 256KB or smaller.')
      }

      const buffer = await readFile(filePath)
      return {
        dataUrl: `data:${mimeType};base64,${buffer.toString('base64')}`,
        fileName: basename(filePath)
      }
    }
  )

  ipcMain.handle('shell:pickAudio', async (): Promise<string | null> => {
    const result = await dialog.showOpenDialog({
      properties: ['openFile'],
      filters: [{ name: 'Audio', extensions: ['ogg', 'mp3', 'wav', 'm4a', 'aac', 'flac'] }]
    })
    if (result.canceled || result.filePaths.length === 0) {
      return null
    }
    return result.filePaths[0]
  })

  // Note: shell:copyFile was removed — it had no live caller (markdown image
  // insert and file duplication both go through the confined fs:* handlers).
}
