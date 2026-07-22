import type { JSX } from 'react'
import { Search } from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

export function WorkbenchQuickOpenPalette(props: {
  visible: boolean
  quickOpenShortcut: string
  jumpShortcut: string
}): JSX.Element {
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
        <span className="ml-auto text-muted-foreground">{props.quickOpenShortcut}</span>
      </div>
      <div className="mt-1 rounded-md border border-border bg-accent px-2 py-2 text-[11px]">
        <span className="font-medium">
          {translate('auto.fw.workbenchContext.search.session', 'session.ts')}
        </span>
        <span className="ml-2 font-mono text-muted-foreground">
          {translate('auto.fw.workbenchContext.search.path', 'src/terminal')}
        </span>
      </div>
      <div className="px-2 py-1.5 text-[11px] text-muted-foreground">
        {translate('auto.fw.workbenchContext.search.test', 'session.test.ts')}
      </div>
      <div className="mt-1 flex items-center gap-2 rounded-md border border-border bg-background px-2 py-2 text-[11px]">
        <span className="font-medium">
          {translate('auto.fw.workbenchContext.jump.title', 'Jump Palette')}
        </span>
        <span className="text-muted-foreground">
          {translate('auto.fw.workbenchContext.jump.action', 'Ports → Open :3000')}
        </span>
        <span className="ml-auto text-muted-foreground">{props.jumpShortcut}</span>
      </div>
    </div>
  )
}
