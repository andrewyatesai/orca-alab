import { useEffect, useState } from 'react'
import type { JSX } from 'react'
import {
  Bot,
  Eye,
  FileImage,
  FileText,
  Globe2,
  Paperclip,
  Search,
  Send,
  SquareTerminal,
  Table2
} from 'lucide-react'
import { useShortcutLabel } from '@/hooks/useShortcutLabel'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

type WorkbenchPhase = 'find' | 'edit' | 'attach' | 'preview'

const PHASES: readonly WorkbenchPhase[] = ['find', 'edit', 'attach', 'preview']
const PHASE_MS = 2300

export function WorkbenchContextWorkflowVisual(props: { reducedMotion: boolean }): JSX.Element {
  const [animatedPhase, setAnimatedPhase] = useState<WorkbenchPhase>('find')
  const phase = props.reducedMotion ? 'preview' : animatedPhase
  const quickOpenShortcut = useShortcutLabel('worktree.quickOpen')
  const jumpShortcut = useShortcutLabel('worktree.palette')

  useEffect(() => {
    if (props.reducedMotion) {
      return
    }
    let index = 0
    const interval = window.setInterval(() => {
      index = (index + 1) % PHASES.length
      setAnimatedPhase(PHASES[index])
    }, PHASE_MS)
    return () => window.clearInterval(interval)
  }, [props.reducedMotion])

  return (
    <div
      className="w-full overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      data-feature-wall-workbench-phase={phase}
      aria-hidden
    >
      <div className="flex h-10 items-center gap-1 border-b border-border bg-muted/30 px-2.5">
        <WorkbenchTab
          icon={SquareTerminal}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000008',
            'Terminal'
          )}
        />
        <WorkbenchTab
          icon={FileText}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000009',
            'session.ts'
          )}
          active={phase !== 'preview'}
        />
        <WorkbenchTab
          icon={Eye}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000001',
            'Preview'
          )}
          active={phase === 'preview'}
        />
        <span className="ml-auto text-[10px] text-muted-foreground">{jumpShortcut}</span>
      </div>

      <div className="relative grid min-h-[270px] grid-cols-[138px_minmax(0,1fr)_190px]">
        <FileExplorer phase={phase} />
        <WorkbenchDocument phase={phase} />
        <AgentComposer phase={phase} />
        <QuickOpenPalette visible={phase === 'find'} shortcut={quickOpenShortcut} />
      </div>

      <FloatingWorkspaceStrip />
      <div className="flex items-center gap-2 border-t border-border bg-muted/20 px-3 py-2 text-[10px] text-muted-foreground">
        <JourneyBeat
          active={phase === 'find'}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000010',
            '1 · Find'
          )}
        />
        <JourneyLine />
        <JourneyBeat
          active={phase === 'edit'}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000011',
            '2 · Edit'
          )}
        />
        <JourneyLine />
        <JourneyBeat
          active={phase === 'attach'}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000012',
            '3 · Attach'
          )}
        />
        <JourneyLine />
        <JourneyBeat
          active={phase === 'preview'}
          label={translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000013',
            '4 · Preview'
          )}
        />
      </div>
    </div>
  )
}

function FloatingWorkspaceStrip(): JSX.Element {
  const actions = [
    [Bot, translate('auto.fw.workbenchContext.floating.agent', 'Cross-repo agent')],
    [SquareTerminal, translate('auto.fw.workbenchContext.floating.terminal', 'Scratch terminal')],
    [FileText, translate('auto.fw.workbenchContext.floating.note', 'Markdown note')],
    [Globe2, translate('auto.fw.workbenchContext.floating.browser', 'Browser tab')]
  ] as const
  return (
    <section
      className="grid gap-2 border-t border-border bg-background/50 px-3 py-2 sm:grid-cols-[190px_minmax(0,1fr)] sm:items-center"
      data-feature-wall-floating-workspace="local-scratchpad"
    >
      <div className="min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="truncate text-[10px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
            {translate('auto.fw.workbenchContext.floating.title', 'Floating Workspace')}
          </span>
          <span className="rounded-full border border-border bg-muted/30 px-1.5 py-px text-[8px] font-medium text-muted-foreground">
            {translate('auto.fw.workbenchContext.floating.default', 'Default on')}
          </span>
        </div>
        <p className="mt-0.5 truncate text-[9px] text-muted-foreground">
          {translate(
            'auto.fw.workbenchContext.floating.boundary',
            'Chosen local directory · stays local during SSH/runtime focus'
          )}
        </p>
      </div>
      <div className="grid grid-cols-4 gap-1.5">
        {actions.map(([Icon, label]) => (
          <span
            key={label}
            className="flex min-w-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 text-[9px] text-muted-foreground"
          >
            <Icon className="size-2.5 shrink-0" aria-hidden />
            <span className="truncate">{label}</span>
          </span>
        ))}
      </div>
    </section>
  )
}

function WorkbenchTab(props: {
  icon: typeof FileText
  label: string
  active?: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <span
      className={cn(
        'inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px]',
        props.active
          ? 'border border-border bg-background text-foreground'
          : 'text-muted-foreground'
      )}
    >
      <Icon className="size-3" />
      {props.label}
    </span>
  )
}

function FileExplorer(props: { phase: WorkbenchPhase }): JSX.Element {
  const fileLabels = {
    src: translate('auto.fw.workbenchContext.file.src', 'src'),
    terminal: translate('auto.fw.workbenchContext.file.terminal', 'terminal'),
    session: translate('auto.fw.workbenchContext.file.session', 'session.ts'),
    readme: translate('auto.fw.workbenchContext.file.readme', 'README.md'),
    image: translate('auto.fw.workbenchContext.file.image', 'flow.png'),
    csv: translate('auto.fw.workbenchContext.file.csv', 'results.csv')
  }
  return (
    <div className="border-r border-border bg-muted/15 p-2.5">
      <p className="px-1 text-[10px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
        {translate(
          'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000002',
          'Files'
        )}
      </p>
      <div className="mt-2 space-y-0.5 text-[10px]">
        <ExplorerRow label={fileLabels.src} folder />
        <ExplorerRow label={fileLabels.terminal} folder nested />
        <ExplorerRow
          label={fileLabels.session}
          nested
          active={props.phase !== 'preview'}
          modified
        />
        <ExplorerRow label={fileLabels.readme} active={props.phase === 'preview'} />
        <ExplorerRow label={fileLabels.image} />
        <ExplorerRow label={fileLabels.csv} />
      </div>
    </div>
  )
}

function ExplorerRow(props: {
  label: string
  folder?: boolean
  nested?: boolean
  active?: boolean
  modified?: boolean
}): JSX.Element {
  return (
    <div
      className={cn(
        'flex h-6 items-center gap-1.5 rounded px-1.5',
        props.nested && 'pl-4',
        props.active ? 'bg-accent text-accent-foreground' : 'text-muted-foreground'
      )}
    >
      <span className="truncate">
        {props.folder ? '▾' : '·'} {props.label}
      </span>
      {props.modified ? (
        <span className="ml-auto font-mono text-[var(--git-decoration-modified)]">M</span>
      ) : null}
    </div>
  )
}

function WorkbenchDocument(props: { phase: WorkbenchPhase }): JSX.Element {
  if (props.phase === 'preview') {
    return <RichPreview />
  }
  return (
    <div className="bg-[var(--editor-surface)] p-3 font-mono text-[10px] leading-5">
      <CodeLine
        number="18"
        text={translate(
          'auto.fw.workbenchContext.code.function',
          'export async function restoreSession(id: string) {'
        )}
      />
      <CodeLine
        number="19"
        text={translate(
          'auto.fw.workbenchContext.code.reconnect',
          '  const session = await reconnect(id)'
        )}
      />
      <CodeLine
        number="20"
        text={translate(
          'auto.fw.workbenchContext.code.verify',
          '  return verifyScrollback(session)'
        )}
        highlighted={props.phase === 'edit' || props.phase === 'attach'}
      />
      <CodeLine number="21" text="}" />
      <div
        className={cn(
          'mt-4 rounded-md border border-border bg-card p-2 font-sans text-[10px] transition-opacity',
          props.phase === 'attach' ? 'opacity-100' : 'opacity-45'
        )}
      >
        <span className="font-medium">
          {translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000003',
            'Selected context'
          )}
        </span>
        <span className="ml-1 text-muted-foreground">
          {translate('auto.fw.workbenchContext.selection', 'session.ts:20')}
        </span>
      </div>
    </div>
  )
}

function CodeLine(props: { number: string; text: string; highlighted?: boolean }): JSX.Element {
  return (
    <div className={cn('grid grid-cols-[24px_1fr] px-1', props.highlighted && 'bg-accent')}>
      <span className="text-right text-muted-foreground">{props.number}</span>
      <span className="ml-3 truncate">{props.text}</span>
    </div>
  )
}

function RichPreview(): JSX.Element {
  return (
    <div className="bg-[var(--editor-surface)] p-4">
      <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground">
        <PreviewKind
          icon={FileText}
          label={translate('auto.fw.workbenchContext.preview.markdown', 'Markdown')}
          active
        />
        <PreviewKind
          icon={FileText}
          label={translate('auto.fw.workbenchContext.preview.pdf', 'PDF')}
        />
        <PreviewKind
          icon={FileImage}
          label={translate('auto.fw.workbenchContext.preview.image', 'Image')}
        />
        <PreviewKind
          icon={Table2}
          label={translate('auto.fw.workbenchContext.preview.csv', 'CSV')}
        />
      </div>
      <div className="mt-5 rounded-lg border border-border bg-card p-4">
        <p className="text-base font-semibold">
          {translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000004',
            'Session recovery'
          )}
        </p>
        <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
          {translate(
            'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000005',
            'Layout and scrollback remain available after a host restart.'
          )}
        </p>
      </div>
    </div>
  )
}

function PreviewKind(props: {
  icon: typeof FileText
  label: string
  active?: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded px-1.5 py-1',
        props.active && 'bg-accent'
      )}
    >
      <Icon className="size-3" />
      {props.label}
    </span>
  )
}

function AgentComposer(props: { phase: WorkbenchPhase }): JSX.Element {
  const attached = props.phase === 'attach' || props.phase === 'preview'
  return (
    <div className="flex flex-col border-l border-border p-3">
      <p className="text-[10px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
        {translate(
          'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000006',
          'Agent context'
        )}
      </p>
      <div className="mt-3 flex-1 rounded-md border border-border bg-background p-2 text-[10px] leading-relaxed">
        {translate(
          'auto.components.feature.wall.WorkbenchContextWorkflowVisual.i130000007',
          'Check the recovery behavior and update the tests.'
        )}
        {attached ? (
          <div className="mt-3 inline-flex items-center gap-1 rounded-md border border-border bg-muted/40 px-2 py-1 font-mono text-[9px]">
            <Paperclip className="size-2.5" />
            {translate('auto.fw.workbenchContext.attachment', 'session.ts:20')}
          </div>
        ) : null}
      </div>
      <div className="mt-2 flex justify-end">
        <span
          className={cn(
            'rounded-md p-1.5',
            attached ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground'
          )}
        >
          <Send className="size-3" />
        </span>
      </div>
    </div>
  )
}

function QuickOpenPalette(props: { visible: boolean; shortcut: string }): JSX.Element {
  return (
    <div
      className={cn(
        'absolute left-1/2 top-4 z-20 w-[330px] -translate-x-1/2 rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-[0_10px_24px_rgba(0,0,0,0.18)] transition-[opacity,transform] motion-reduce:transition-none',
        props.visible ? 'translate-y-0 opacity-100' : '-translate-y-1 opacity-0'
      )}
    >
      <div className="flex items-center gap-2 border-b border-border px-2 py-2 text-[11px]">
        <Search className="size-3.5 text-muted-foreground" />
        <span className="font-mono">
          {translate('auto.fw.workbenchContext.search.query', 'session')}
        </span>
        <span className="ml-auto text-[9px] text-muted-foreground">{props.shortcut}</span>
      </div>
      <div className="mt-1 rounded-md border border-border bg-accent px-2 py-2 text-[11px]">
        <span className="font-medium">
          {translate('auto.fw.workbenchContext.search.session', 'session.ts')}
        </span>
        <span className="ml-2 font-mono text-[9px] text-muted-foreground">
          {translate('auto.fw.workbenchContext.search.path', 'src/terminal')}
        </span>
      </div>
      <div className="px-2 py-1.5 text-[10px] text-muted-foreground">
        {translate('auto.fw.workbenchContext.search.test', 'session.test.ts')}
      </div>
    </div>
  )
}

function JourneyBeat(props: { active: boolean; label: string }): JSX.Element {
  return (
    <span
      className={cn('rounded-full px-2 py-0.5', props.active && 'bg-accent text-accent-foreground')}
    >
      {props.label}
    </span>
  )
}

function JourneyLine(): JSX.Element {
  return <span className="h-px min-w-4 flex-1 bg-border" />
}
