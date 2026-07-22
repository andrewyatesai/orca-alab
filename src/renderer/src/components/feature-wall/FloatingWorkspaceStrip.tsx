import type { JSX } from 'react'
import { ArrowRight, Bot, FileText, Globe2, Mic2, SquareTerminal } from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function FloatingWorkspaceStrip(): JSX.Element {
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
          <span className="truncate text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
            {translate('auto.fw.workbenchContext.floating.title', 'Floating Workspace')}
          </span>
          <span className="rounded-full border border-border bg-muted/30 px-1.5 py-px text-[11px] font-medium text-muted-foreground">
            {translate('auto.fw.workbenchContext.floating.default', 'Default on')}
          </span>
        </div>
        <p className="mt-0.5 truncate text-[11px] text-muted-foreground">
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
            className="flex min-w-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 text-[11px] text-muted-foreground"
          >
            <Icon className="size-2.5 shrink-0" aria-hidden />
            <span className="truncate">{label}</span>
          </span>
        ))}
      </div>
      <div
        className="flex min-w-0 items-center gap-2 border-t border-border pt-2 text-[11px] text-muted-foreground sm:col-span-2"
        data-feature-wall-voice-dictation="focused-pane"
      >
        <Mic2 className="size-3.5 shrink-0" />
        <span className="font-semibold text-foreground">
          {translate('auto.fw.workbenchContext.voice.title', 'Optional Voice Dictation')}
        </span>
        <span>{translate('auto.fw.workbenchContext.voice.model', 'Model + microphone ready')}</span>
        <ArrowRight className="size-3 shrink-0" />
        <span className="truncate">
          {translate('auto.fw.workbenchContext.voice.target', 'Transcript → focused pane')}
        </span>
      </div>
    </section>
  )
}
