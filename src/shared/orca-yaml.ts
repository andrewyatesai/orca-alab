import { parse } from 'yaml'
import {
  MAX_QUICK_COMMAND_AGENT_PROMPT_LENGTH,
  MAX_QUICK_COMMAND_LABEL_LENGTH,
  MAX_QUICK_COMMAND_TERMINAL_TEXT_LENGTH
} from './terminal-quick-commands'
import type {
  OrcaDefaultTabTemplate,
  OrcaHooks,
  OrcaProjectQuickCommand,
  OrcaVmRecipe,
  OrcaVmRecipeDiagnostic
} from './types'

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function asTrimmedString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

const DEFAULT_TAB_COLOR_RE = /^#[0-9a-fA-F]{3}(?:[0-9a-fA-F]{3})?$/
export const ORCA_VM_RECIPE_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{0,63}$/
export const ORCA_VM_RECIPE_ID_RULE =
  'Use 1-64 lowercase letters, numbers, dots, underscores, or hyphens, starting with a letter or number.'

function normalizeDefaultTabs(value: unknown): OrcaDefaultTabTemplate[] {
  if (!Array.isArray(value)) {
    return []
  }

  return value
    .map((entry) => {
      const record = asRecord(entry)
      if (!record) {
        return null
      }
      const title = asTrimmedString(record.title)
      const command = asTrimmedString(record.command)
      const color = asTrimmedString(record.color)
      const normalizedColor = color && DEFAULT_TAB_COLOR_RE.test(color) ? color : undefined
      if (!title && !command && !normalizedColor) {
        return null
      }
      return {
        ...(title ? { title } : {}),
        ...(normalizedColor ? { color: normalizedColor } : {}),
        ...(command ? { command } : {})
      }
    })
    .filter((entry): entry is OrcaDefaultTabTemplate => entry !== null)
}

type VmRecipeParseResult = {
  recipes: OrcaVmRecipe[]
  diagnostics: OrcaVmRecipeDiagnostic[]
}

function normalizeVmRecipes(value: unknown): VmRecipeParseResult {
  const diagnostics: OrcaVmRecipeDiagnostic[] = []
  if (!Array.isArray(value)) {
    return { recipes: [], diagnostics }
  }

  const seenIds = new Set<string>()
  const recipes = value
    .map((entry, index) => {
      const record = asRecord(entry)
      if (!record) {
        diagnostics.push({
          index,
          message: 'Recipe entry must be a mapping.'
        })
        return null
      }
      const id = asTrimmedString(record.id)
      const name = asTrimmedString(record.name)
      const create = asTrimmedString(record.create) ?? asTrimmedString(record.command)
      if (!id) {
        diagnostics.push({ index, field: 'id', message: 'Recipe id is required.' })
        return null
      }
      if (!ORCA_VM_RECIPE_ID_PATTERN.test(id)) {
        diagnostics.push({
          index,
          field: 'id',
          message: `Invalid recipe id "${id}". ${ORCA_VM_RECIPE_ID_RULE}`
        })
        return null
      }
      if (seenIds.has(id)) {
        diagnostics.push({
          index,
          field: 'id',
          message: `Duplicate recipe id "${id}". Recipe ids must be unique.`
        })
        return null
      }
      if (!name) {
        diagnostics.push({ index, field: 'name', message: `Recipe "${id}" is missing name.` })
        return null
      }
      if (!create) {
        diagnostics.push({ index, field: 'create', message: `Recipe "${id}" is missing create.` })
        return null
      }
      seenIds.add(id)
      const description = asTrimmedString(record.description)
      const suspend = asTrimmedString(record.suspend)
      const resume = asTrimmedString(record.resume)
      const destroyValue = asTrimmedString(record.destroy) ?? asTrimmedString(record.cleanup)
      const destroyDisabled = destroyValue === 'none'
      return {
        id,
        name,
        create,
        ...(description ? { description } : {}),
        ...(suspend ? { suspend } : {}),
        ...(resume ? { resume } : {}),
        ...(destroyValue && !destroyDisabled ? { destroy: destroyValue } : {}),
        ...(destroyDisabled ? { destroyDisabled: true } : {})
      }
    })
    .filter((entry): entry is OrcaVmRecipe => entry !== null)
  return { recipes, diagnostics }
}

// Why: shared yaml is repo-controlled input — a hard entry cap plus the settings
// text caps bound what a hostile orca.yaml can push at the trust dialog and menus.
export const ORCA_YAML_QUICK_COMMAND_CAP = 30

type QuickCommandParseResult = {
  quickCommands: OrcaProjectQuickCommand[]
  diagnostics: OrcaVmRecipeDiagnostic[]
}

function normalizeQuickCommands(value: unknown): QuickCommandParseResult {
  const diagnostics: OrcaVmRecipeDiagnostic[] = []
  if (!Array.isArray(value)) {
    return { quickCommands: [], diagnostics }
  }

  const quickCommands: OrcaProjectQuickCommand[] = []
  for (const [index, entry] of value.entries()) {
    if (quickCommands.length >= ORCA_YAML_QUICK_COMMAND_CAP) {
      diagnostics.push({
        index,
        message: `quickCommands is capped at ${ORCA_YAML_QUICK_COMMAND_CAP} entries; ${value.length - index} extra ignored.`
      })
      break
    }
    const record = asRecord(entry)
    if (!record) {
      diagnostics.push({ index, message: 'Quick command entry must be a mapping.' })
      continue
    }
    const label = asTrimmedString(record.label)?.slice(0, MAX_QUICK_COMMAND_LABEL_LENGTH)
    if (!label) {
      diagnostics.push({ index, field: 'label', message: 'Quick command label is required.' })
      continue
    }
    const action = record.action
    if (action !== undefined && action !== 'agent-prompt' && action !== 'terminal-command') {
      diagnostics.push({
        index,
        field: 'action',
        message: `Quick command "${label}" has an unknown action. Use "agent-prompt" or omit action.`
      })
      continue
    }
    if (action === 'agent-prompt') {
      const agent = asTrimmedString(record.agent)
      const prompt = asTrimmedString(record.prompt)?.slice(0, MAX_QUICK_COMMAND_AGENT_PROMPT_LENGTH)
      if (!agent || !prompt) {
        diagnostics.push({
          index,
          field: agent ? 'prompt' : 'agent',
          message: `Quick command "${label}" needs both agent and prompt for agent-prompt.`
        })
        continue
      }
      quickCommands.push({ label, action: 'agent-prompt', agent, prompt })
      continue
    }
    const command = asTrimmedString(record.command)?.slice(0, MAX_QUICK_COMMAND_TERMINAL_TEXT_LENGTH)
    if (!command) {
      diagnostics.push({
        index,
        field: 'command',
        message: `Quick command "${label}" is missing command.`
      })
      continue
    }
    quickCommands.push({
      label,
      command,
      ...(record.appendEnter === false ? { appendEnter: false } : {})
    })
  }
  return { quickCommands, diagnostics }
}

/**
 * Parse the supported project defaults from `orca.yaml`.
 */
export function parseOrcaYaml(content: string): OrcaHooks | null {
  let root: unknown
  try {
    root = parse(content)
  } catch {
    return null
  }

  const record = asRecord(root)
  if (!record) {
    return null
  }

  const scriptsRecord = asRecord(record.scripts)
  const preCreate = scriptsRecord ? asTrimmedString(scriptsRecord.preCreate) : undefined
  const setup = scriptsRecord ? asTrimmedString(scriptsRecord.setup) : undefined
  const archive = scriptsRecord ? asTrimmedString(scriptsRecord.archive) : undefined
  const issueCommand = asTrimmedString(record.issueCommand)
  const defaultTabs = normalizeDefaultTabs(record.defaultTabs)
  const environmentRecipeParse = normalizeVmRecipes(record.environmentRecipes)
  const environmentRecipes = environmentRecipeParse.recipes
  const environmentRecipeDiagnostics = environmentRecipeParse.diagnostics
  const quickCommandParse = normalizeQuickCommands(record.quickCommands)
  const quickCommands = quickCommandParse.quickCommands
  const quickCommandDiagnostics = quickCommandParse.diagnostics

  if (
    !preCreate &&
    !setup &&
    !archive &&
    !issueCommand &&
    defaultTabs.length === 0 &&
    environmentRecipes.length === 0 &&
    environmentRecipeDiagnostics.length === 0 &&
    quickCommands.length === 0 &&
    quickCommandDiagnostics.length === 0
  ) {
    return null
  }

  return {
    scripts: {
      ...(preCreate ? { preCreate } : {}),
      ...(setup ? { setup } : {}),
      ...(archive ? { archive } : {})
    },
    ...(issueCommand ? { issueCommand } : {}),
    ...(defaultTabs.length > 0 ? { defaultTabs } : {}),
    ...(environmentRecipes.length > 0 ? { environmentRecipes } : {}),
    ...(environmentRecipeDiagnostics.length > 0 ? { environmentRecipeDiagnostics } : {}),
    ...(quickCommands.length > 0 ? { quickCommands } : {}),
    ...(quickCommandDiagnostics.length > 0 ? { quickCommandDiagnostics } : {})
  }
}
