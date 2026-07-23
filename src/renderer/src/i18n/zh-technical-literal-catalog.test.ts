import { describe, expect, it } from 'vitest'
import en from './locales/en.json'
import zh from './locales/zh.json'

// Regression pin for upstream #9574: the zh bootstrap-translate step rendered
// technical literals / commands / brand names as prose (e.g. `GH PR` ->
// "growth hormone receptor", `--model sonnet` -> "模范十四行诗", `pnpm install`
// -> "即插即用安装"), and translated short UI words with the wrong sense
// (`Move` -> "手机"/phone, `State` -> "州"/province). Some of these render as
// live labels, so keep the corrected values pinned.

type Node = Record<string, unknown>

function leaf(catalog: Node, path: readonly string[]): string {
  let node: unknown = catalog
  for (const seg of path) {
    node = (node as Node)[seg]
  }
  expect(typeof node).toBe('string')
  return node as string
}

// True technical literals / commands / filenames whose zh value must equal the
// English source text unchanged.
const englishLiteralPaths: readonly (readonly string[])[] = [
  ['auto', 'components', 'sidebar', 'WorktreeMetaDialog', '1b91db7e14'], // GH PR
  ['auto', 'components', 'right', 'sidebar', 'SourceControlAgentActionDialogForm', 'fe119187bb'], // --model sonnet
  ['auto', 'components', 'right', 'sidebar', 'SourceControlTextGenerationDialogForm', '551ffd111b'], // --model sonnet
  ['auto', 'components', 'feature', 'wall', 'FeatureWallSetupWorkflowActions', '5c5b65044e'], // pnpm install
  ['auto', 'components', 'sidebar', 'OrcaYamlTrustDialog', '79afc6772b'], // orca.yaml
  [
    'auto',
    'components',
    'terminal',
    'quick',
    'commands',
    'TerminalQuickCommandDialog',
    '97e96cc027'
  ], // /goal
  ['auto', 'components', 'editor', 'RichMarkdownCodeBlock', '5af8251002'], // SCSS
  ['auto', 'components', 'right', 'sidebar', 'SourceControl', 'f62ce91ade'] // origin
]

// Short UI words previously translated with an unrelated sense.
const wrongSensePaths: readonly (readonly [readonly string[], string])[] = [
  [['auto', 'components', 'WorktreeJumpPalette', 'ac037cfac2'], '移动'], // Move (was 手机)
  [['auto', 'components', 'status', 'bar', 'ResourceUsageStatusSegment', '1b24a32d3a'], '内存'], // Memory (was 记忆)
  [['auto', 'components', 'status', 'bar', 'WorkspaceSpaceManagerPanel', 'a998501630'], '强制'], // Force (was 力量)
  [['auto', 'components', 'right', 'sidebar', 'PortsPanel', '5dd86dcf2f'], '进程'], // Process (was 过程)
  [['auto', 'components', 'right', 'sidebar', 'PortsPanel', 'c9d106547a'], '转发'], // Forward (was 向前)
  [['auto', 'components', 'right', 'sidebar', 'SourceControl', '8cde1a2fb0'], '暂存'], // Stage (was 阶段)
  [
    ['auto', 'components', 'sidebar', 'WorktreeCardMetadataStatusBadges', 'af2b07bda5'],
    '状态：{{value0}}'
  ], // State (was 州)
  [['auto', 'components', 'settings', 'TerminalAppearanceSection', 'e070e8aeba'], '竖线'], // Bar cursor (was 酒吧)
  [['auto', 'components', 'settings', 'TerminalAppearanceSection', '52854a5608'], '块状'] // Block cursor (was 堵塞)
]

// Brand names must survive un-transliterated inside their localized phrase.
const brandNamePaths: readonly (readonly [readonly string[], string])[] = [
  [['auto', 'components', 'settings', 'KagiSessionLinkForm', 'ff450194cd'], 'Kagi'], // not 卡吉
  [['auto', 'components', 'automations', 'AutomationEditorDialogHeader', '0a75e5e2fa'], 'Hermes'], // not 爱马仕
  [['auto', 'components', 'settings', 'TerminalAppearanceSection', '855a76343a'], 'Ghostty'] // not 幽灵
]

describe('zh catalog technical-literal integrity (#9574)', () => {
  it.each(englishLiteralPaths)('%j keeps the English literal', (...path) => {
    expect(leaf(zh, path)).toBe(leaf(en, path))
  })

  it.each(wrongSensePaths)('%j uses the correct-sense translation', (path, expected) => {
    expect(leaf(zh, path)).toBe(expected)
  })

  it.each(brandNamePaths)('%j preserves the brand name', (path, brand) => {
    expect(leaf(zh, path)).toContain(brand)
  })
})
