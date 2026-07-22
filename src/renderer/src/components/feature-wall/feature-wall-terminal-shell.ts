import { translate } from '@/i18n/i18n'

export type FeatureWallTerminalShell = {
  banner: string
  prompt: '>'
}

export function getFeatureWallTerminalShell(): FeatureWallTerminalShell {
  // Why: the active shell may live on an SSH or paired runtime whose platform
  // differs from this client, so the walkthrough must not infer host chrome.
  return {
    banner: translate(
      'auto.components.feature.wall.feature-wall-terminal-shell.h130000001',
      'Persistent shell session ready'
    ),
    prompt: '>'
  }
}
