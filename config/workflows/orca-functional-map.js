export const meta = {
  name: 'orca-functional-map',
  description: 'Map every Orca subsystem (functional surface + deps + Rust portability) for the TS to Rust rewrite',
  phases: [
    { title: 'Map subsystems', detail: 'one Explore agent per subsystem' },
    { title: 'Synthesize', detail: 'dependency to crate map + migration phasing' },
  ],
}

const SPEC_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['name', 'area', 'purpose', 'capabilities', 'publicApi', 'externalDeps', 'persistence', 'crossPlatform', 'rustPortability'],
  properties: {
    name: { type: 'string' },
    area: { type: 'string', enum: ['backend', 'renderer', 'cli', 'relay', 'preload', 'shared'] },
    purpose: { type: 'string', description: '1-3 sentences: what this subsystem is for' },
    capabilities: { type: 'array', items: { type: 'string' }, description: 'Concrete behaviors/features this code implements' },
    publicApi: { type: 'array', items: { type: 'string' }, description: 'Key exported functions, IPC channel names, RPC methods actually present' },
    externalDeps: { type: 'array', items: { type: 'string' }, description: 'Real external deps: npm packages, native addons, shelled-out binaries, network services, Electron/OS APIs' },
    persistence: { type: 'array', items: { type: 'string' }, description: 'sqlite tables, on-disk files, electron-store keys, caches' },
    crossPlatform: { type: 'array', items: { type: 'string' }, description: 'Platform-specific concerns (macOS/Linux/Windows/SSH)' },
    rustPortability: {
      type: 'object',
      additionalProperties: false,
      required: ['tier', 'targetCrate', 'effort', 'notes'],
      properties: {
        tier: { type: 'string', enum: ['pure', 'io', 'platform', 'ui-native', 'ffi', 'mixed'] },
        targetCrate: { type: 'string', description: 'Suggested Rust crate in the new workspace + any vendorable third-party crate' },
        effort: { type: 'string', enum: ['S', 'M', 'L', 'XL'] },
        notes: { type: 'string' },
      },
    },
  },
}

const UNITS = [
  { name: 'main-runtime', area: 'backend', paths: ['src/main/runtime'], hint: 'workspace/session orchestration, RPC, remote runtime. LARGE - list dir, read orchestration/ and rpc/ entry files, sample; map at capability level.' },
  { name: 'main-ipc', area: 'backend', paths: ['src/main/ipc'], hint: 'IPC handler surface between renderer and main. LARGE - enumerate the IPC channel names/handlers; sample handlers.' },
  { name: 'main-browser', area: 'backend', paths: ['src/main/browser'], hint: 'embedded browser / agent-browser integration, screencast.' },
  { name: 'main-daemon', area: 'backend', paths: ['src/main/daemon'], hint: 'background daemon, headless terminal emulator (xterm headless), session buffers.' },
  { name: 'main-github', area: 'backend', paths: ['src/main/github'], hint: 'GitHub API, PRs, project view, hosted review.' },
  { name: 'main-ssh', area: 'backend', paths: ['src/main/ssh'], hint: 'SSH remote runtime via ssh2.' },
  { name: 'main-agent-hooks', area: 'backend', paths: ['src/main/agent-hooks'], hint: 'agent lifecycle hooks/listeners.' },
  { name: 'main-git', area: 'backend', paths: ['src/main/git'], hint: 'git operations, worktrees, diff, branch cleanup, uncommitted stats.' },
  { name: 'main-providers', area: 'backend', paths: ['src/main/providers'], hint: 'agent provider abstraction (claude/codex/gemini dispatch).' },
  { name: 'main-claude-accounts', area: 'backend', paths: ['src/main/claude-accounts', 'src/main/claude'], hint: 'Claude account/auth management.' },
  { name: 'main-codex-accounts', area: 'backend', paths: ['src/main/codex-accounts', 'src/main/codex', 'src/main/codex-cli', 'src/main/codex-usage'], hint: 'Codex accounts, CLI integration, usage.' },
  { name: 'main-rate-limits', area: 'backend', paths: ['src/main/rate-limits'], hint: 'rate limit tracking across providers.' },
  { name: 'main-gitlab', area: 'backend', paths: ['src/main/gitlab'], hint: 'GitLab API + hosted review.' },
  { name: 'main-window', area: 'backend', paths: ['src/main/window', 'src/main/menu', 'src/main/dock'], hint: 'Electron BrowserWindow, menus, dock - become native Swift shell concerns.' },
  { name: 'main-computer', area: 'backend', paths: ['src/main/computer'], hint: 'computer-use (native helpers per OS), screenshots, input injection.' },
  { name: 'main-observability', area: 'backend', paths: ['src/main/observability'], hint: 'logging/metrics/tracing.' },
  { name: 'main-linear', area: 'backend', paths: ['src/main/linear'], hint: 'Linear SDK integration.' },
  { name: 'main-automations', area: 'backend', paths: ['src/main/automations'], hint: 'automation rules/triggers.' },
  { name: 'main-startup', area: 'backend', paths: ['src/main/startup'], hint: 'app startup sequence, diagnostics.' },
  { name: 'main-text-generation', area: 'backend', paths: ['src/main/text-generation', 'src/main/hermes'], hint: 'text generation helpers, commit messages.' },
  { name: 'main-cli-internal', area: 'backend', paths: ['src/main/cli'], hint: 'main-process side of the orca CLI bridge.' },
  { name: 'main-telemetry', area: 'backend', paths: ['src/main/telemetry'], hint: 'posthog telemetry, gated transport.' },
  { name: 'main-ports', area: 'backend', paths: ['src/main/ports'], hint: 'port allocation/forwarding.' },
  { name: 'main-usage', area: 'backend', paths: ['src/main/claude-usage', 'src/main/opencode-usage', 'src/main/stats'], hint: 'usage/cost accounting.' },
  { name: 'main-speech', area: 'backend', paths: ['src/main/speech'], hint: 'speech-to-text via sherpa-onnx, dictation.' },
  { name: 'main-source-control', area: 'backend', paths: ['src/main/source-control'], hint: 'provider-agnostic source-control abstraction.' },
  { name: 'main-git-providers-small', area: 'backend', paths: ['src/main/jira', 'src/main/azure-devops', 'src/main/gitea', 'src/main/bitbucket'], hint: 'smaller issue/SCM provider integrations.' },
  { name: 'main-misc-infra', area: 'backend', paths: ['src/main/attribution', 'src/main/ghostty', 'src/main/pty', 'src/main/memory', 'src/main/crash-reporting', 'src/main/network', 'src/main/sqlite', 'src/main/lib'], hint: 'pty (node-pty), ghostty config, attribution, crash reporting, sqlite, network proxy.' },
  { name: 'main-agent-integrations', area: 'backend', paths: ['src/main/opencode', 'src/main/pi', 'src/main/copilot', 'src/main/cursor', 'src/main/grok', 'src/main/droid', 'src/main/gemini', 'src/main/amp', 'src/main/command-code', 'src/main/antigravity', 'src/main/openclaude'], hint: 'thin per-agent-CLI integration adapters.' },
  { name: 'main-platform-misc', area: 'backend', paths: ['src/main/project-groups', 'src/main/skills', 'src/main/keybindings', 'src/main/star-nag'], hint: 'project groups, skills, keybindings, star-nag.' },
  { name: 'main-root', area: 'backend', paths: ['src/main'], hint: 'ONLY top-level src/main/*.ts files (not subdirs): app entry index.ts, wiring, global state.' },
  { name: 'ui-sidebar', area: 'renderer', paths: ['src/renderer/src/components/sidebar'], hint: 'left sidebar: workspaces, sessions, repos tree.' },
  { name: 'ui-terminal', area: 'renderer', paths: ['src/renderer/src/components/terminal-pane', 'src/renderer/src/components/floating-terminal', 'src/renderer/src/components/terminal', 'src/renderer/src/components/terminal-quick-commands'], hint: 'terminal UI (xterm.js) - maps to native alacritty terminal. Capture: tabs, search, quick commands, scrollback, ligatures.' },
  { name: 'ui-editor', area: 'renderer', paths: ['src/renderer/src/components/editor'], hint: 'code/diff editor (Monaco), diff comments.' },
  { name: 'ui-settings', area: 'renderer', paths: ['src/renderer/src/components/settings'], hint: 'settings panels - enumerate every settings category/feature.' },
  { name: 'ui-right-sidebar', area: 'renderer', paths: ['src/renderer/src/components/right-sidebar'], hint: 'right sidebar: agent chat/composer, review, activity.' },
  { name: 'ui-feature-wall', area: 'renderer', paths: ['src/renderer/src/components/feature-wall', 'src/renderer/src/components/feature-tips', 'src/renderer/src/components/contextual-tours'], hint: 'feature education/onboarding walls and tips.' },
  { name: 'ui-status-bar', area: 'renderer', paths: ['src/renderer/src/components/status-bar'], hint: 'bottom status bar indicators.' },
  { name: 'ui-browser-pane', area: 'renderer', paths: ['src/renderer/src/components/browser-pane'], hint: 'embedded browser pane UI.' },
  { name: 'ui-tabs', area: 'renderer', paths: ['src/renderer/src/components/tab-bar', 'src/renderer/src/components/tab-group'], hint: 'tab bar and tab grouping.' },
  { name: 'ui-automations', area: 'renderer', paths: ['src/renderer/src/components/automations'], hint: 'automations UI.' },
  { name: 'ui-onboarding', area: 'renderer', paths: ['src/renderer/src/components/onboarding', 'src/renderer/src/components/setup-guide', 'src/renderer/src/components/new-workspace'], hint: 'onboarding, setup guide, new workspace flow.' },
  { name: 'ui-scm-views', area: 'renderer', paths: ['src/renderer/src/components/github-project', 'src/renderer/src/components/github', 'src/renderer/src/components/gitlab', 'src/renderer/src/components/diff-comments'], hint: 'github/gitlab project + review UI.' },
  { name: 'ui-misc', area: 'renderer', paths: ['src/renderer/src/components/stats', 'src/renderer/src/components/activity', 'src/renderer/src/components/dashboard', 'src/renderer/src/components/mobile', 'src/renderer/src/components/pet', 'src/renderer/src/components/workspace-cleanup', 'src/renderer/src/components/cmd-j', 'src/renderer/src/components/dictation', 'src/renderer/src/components/agent', 'src/renderer/src/components/skills', 'src/renderer/src/components/sparse', 'src/renderer/src/components/crash-report', 'src/renderer/src/components/repo', 'src/renderer/src/components/ports'], hint: 'assorted UI: stats, activity feed, dashboard, mobile, command palette (cmd-j), dictation. Summarize each briefly.' },
  { name: 'ui-store', area: 'renderer', paths: ['src/renderer/src/store'], hint: 'client state stores. Enumerate the major stores and the state they hold - defines the native app model.' },
  { name: 'ui-lib-hooks', area: 'renderer', paths: ['src/renderer/src/lib', 'src/renderer/src/hooks'], hint: 'renderer lib + hooks: pure client logic (much portable), formatting, derived state.' },
  { name: 'ui-runtime-web', area: 'renderer', paths: ['src/renderer/src/runtime', 'src/renderer/src/web'], hint: 'renderer runtime bridge to main + web (mobile) build entry.' },
  { name: 'cli', area: 'cli', paths: ['src/cli'], hint: 'the orca CLI binary: commands, handlers, specs, runtime.' },
  { name: 'relay', area: 'relay', paths: ['src/relay'], hint: 'relay server (pairing, remote/mobile connectivity, websockets).' },
  { name: 'preload', area: 'preload', paths: ['src/preload'], hint: 'Electron preload bridge exposing typed API to renderer - defines the full main<->renderer contract.' },
  { name: 'shared', area: 'shared', paths: ['src/shared'], hint: 'cross-cutting pure logic + types. Enumerate the PURE-LOGIC modules (path/string/parsing/formatting) = highest-priority Rust core ports, vs type-only modules.' },
]

function mapPrompt(u) {
  return [
    'You are mapping the "' + u.name + '" subsystem of Orca - an Electron + React + TypeScript "IDE for parallel agentic development" - as input to a faithful full rewrite into native Rust (thin Swift/SwiftUI shell on macOS, alacritty_terminal for terminal emulation).',
    '',
    'Scope (read ONLY these paths, in the primary working directory): ' + u.paths.join(', '),
    'Hint: ' + u.hint,
    '',
    'Method: list files first; read entry/index files and a representative sample of implementation files. For LARGE dirs, SAMPLE - do not read every file; infer capabilities from names + key files. Do NOT read outside the scoped paths.',
    '',
    'Rules for the spec:',
    '- capabilities = real behaviors/features this code implements (what it DOES), not vague restatements.',
    '- publicApi = actual exported function names, IPC channel string literals, or RPC method names you saw.',
    '- externalDeps = real imports of npm packages, native addons (node-pty, sherpa-onnx, @parcel/watcher, ssh2, better-sqlite3), binaries shelled out via child_process, and network services. Only list what you actually see.',
    "- rustPortability.tier: 'pure' (no IO), 'io' (fs/net/process), 'platform' (OS-specific), 'ui-native' (DOM/React to rebuild natively), 'ffi' (needs native lib binding), 'mixed'.",
    '- Do NOT invent. If unsure, say so in notes. Return ONLY the structured spec.',
  ].join('\n')
}

phase('Map subsystems')
log('Mapping ' + UNITS.length + ' subsystems across ~908K LOC...')

const thunks = UNITS.map((u) => () =>
  agent(mapPrompt(u), { label: 'map:' + u.name, phase: 'Map subsystems', agentType: 'Explore', schema: SPEC_SCHEMA })
)
const mapped = await parallel(thunks)
const specs = mapped.filter(Boolean)

log('Mapped ' + specs.length + '/' + UNITS.length + ' subsystems. Synthesizing...')
phase('Synthesize')

const compact = specs.map((s) => ({
  name: s.name, area: s.area, purpose: s.purpose,
  deps: s.externalDeps, tier: s.rustPortability && s.rustPortability.tier,
  crate: s.rustPortability && s.rustPortability.targetCrate, effort: s.rustPortability && s.rustPortability.effort,
}))
const depInput = specs.map((s) => ({ name: s.name, area: s.area, deps: s.externalDeps, tier: s.rustPortability && s.rustPortability.tier }))

const DEP_SCHEMA = {
  type: 'object', additionalProperties: false, required: ['dependencies', 'notes'],
  properties: {
    dependencies: { type: 'array', items: { type: 'object', additionalProperties: false,
      required: ['name', 'kind', 'role', 'usedBy', 'rustReplacement', 'vendorStrategy', 'risk'],
      properties: {
        name: { type: 'string' }, kind: { type: 'string', enum: ['npm', 'native-addon', 'binary', 'electron', 'service', 'browser-api'] },
        role: { type: 'string' }, usedBy: { type: 'array', items: { type: 'string' } },
        rustReplacement: { type: 'string' }, vendorStrategy: { type: 'string' },
        risk: { type: 'string', enum: ['low', 'medium', 'high'] },
      } } },
    notes: { type: 'string' },
  },
}
const PHASE_SCHEMA = {
  type: 'object', additionalProperties: false, required: ['domains', 'phases', 'criticalPath', 'risks'],
  properties: {
    domains: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['domain', 'subsystems', 'targetCrate', 'summary'],
      properties: { domain: { type: 'string' }, subsystems: { type: 'array', items: { type: 'string' } }, targetCrate: { type: 'string' }, summary: { type: 'string' } } } },
    phases: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['phase', 'goal', 'includes', 'rationale'],
      properties: { phase: { type: 'string' }, goal: { type: 'string' }, includes: { type: 'array', items: { type: 'string' } }, rationale: { type: 'string' } } } },
    criticalPath: { type: 'array', items: { type: 'string' } },
    risks: { type: 'array', items: { type: 'string' } },
  },
}

const depPrompt = 'Per-subsystem dependency lists from Orca (Electron/TS) being rewritten to native Rust with vendored, stripped deps:\n'
  + JSON.stringify(depInput)
  + '\n\nProduce a consolidated external-dependency to Rust replacement map. Dedup across subsystems. For each real external dependency (npm package, native addon, shelled-out binary, electron API, browser API, network service), recommend the best vendorable Rust crate (or "rewrite"/"native FFI"), how to vendor+strip it to essentials, who uses it, and risk. Cover: node-pty, ssh2, sherpa-onnx, @parcel/watcher, sqlite, ws, electron BrowserWindow/IPC, xterm, monaco, react, tiptap, posthog, agent-browser, child_process (git/gh/agent CLIs). Be concrete.'

const phasePrompt = 'Per-subsystem map of Orca (name, area, purpose, deps, Rust tier, crate, effort):\n'
  + JSON.stringify(compact)
  + '\n\nTarget: native Rust core workspace, alacritty_terminal for terminal emulation (replacing xterm), thin Swift/SwiftUI shell for native macOS (React renderer rebuilt natively), all deps vendored & stripped.\n\nProduce: (1) domains = groupings of subsystems into proposed Rust crates; (2) phases = ordered migration plan (leaf pure-logic first, then IO/core services, then UI shell last) with rationale; (3) criticalPath; (4) risks. Order lowest-risk-highest-leverage first.'

const synth = await parallel([
  () => agent(depPrompt, { label: 'synth:dependency-map', phase: 'Synthesize', schema: DEP_SCHEMA }),
  () => agent(phasePrompt, { label: 'synth:migration-phasing', phase: 'Synthesize', schema: PHASE_SCHEMA }),
])

return { specs, depMap: synth[0], phasing: synth[1] }
