import type { JSX } from 'react'
import { ShieldCheck, ToggleLeft, UserRound } from 'lucide-react'
import { ClaudeIcon } from '@/components/status-bar/icons'
import { translate } from '@/i18n/i18n'

const PAYLOAD_ITEMS = [
  {
    key: 'auto.components.feature.wall.BrowserDesignPayloadSummary.a140000001',
    fallback: 'DOM'
  },
  {
    key: 'auto.components.feature.wall.BrowserDesignPayloadSummary.a150000001',
    fallback: 'Computed styles'
  },
  {
    key: 'auto.components.feature.wall.BrowserDesignPayloadSummary.a150000002',
    fallback: 'Source hint · when available'
  },
  {
    key: 'auto.components.feature.wall.BrowserDesignPayloadSummary.a150000003',
    fallback: 'Cropped PNG · when available'
  }
] as const

export function BrowserDesignPayloadSummary(props: { mode?: 'compose' | 'sent' }): JSX.Element {
  const sent = props.mode === 'sent'
  return (
    <div
      className={sent ? 'space-y-1.5' : 'space-y-1.5 border-t border-popover-foreground/10 pt-1.5'}
      data-feature-wall-browser-context-receipt={sent ? 'sent' : undefined}
    >
      <div className="flex items-center gap-1.5">
        <span className="shrink-0 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate(
            'auto.components.feature.wall.BrowserDesignPayloadSummary.a140000005',
            'Attached'
          )}
        </span>
        <div className="flex min-w-0 flex-wrap gap-1">
          {PAYLOAD_ITEMS.map((item) => (
            <span
              key={item.key}
              className="rounded-full border border-border bg-background/70 px-1.5 py-0.5 text-[11px] leading-none text-popover-foreground"
            >
              {translate(item.key, item.fallback)}
            </span>
          ))}
        </div>
      </div>
      <div className="flex items-center gap-1.5 text-[11px] text-popover-foreground">
        <ClaudeIcon size={11} />
        <span>
          {translate(
            'auto.components.feature.wall.BrowserDesignPayloadSummary.a140000006',
            'Destination · Claude in this workspace'
          )}
        </span>
      </div>
      <div
        className="grid gap-1.5 rounded-md border border-border bg-background/70 px-2 py-1.5 text-[11px] sm:grid-cols-2"
        data-feature-wall-browser-profile-choice
      >
        <span className="flex min-w-0 items-center gap-1.5">
          <UserRound className="size-3 shrink-0 text-muted-foreground" />
          <span className="text-muted-foreground">
            {translate(
              'auto.components.feature.wall.BrowserDesignPayloadSummary.profile.label',
              'Browser profile'
            )}
          </span>
          <span className="truncate font-medium">
            {translate(
              'auto.components.feature.wall.BrowserDesignPayloadSummary.profile.value',
              'Workspace QA'
            )}
          </span>
        </span>
        <span className="flex min-w-0 items-center gap-1.5">
          <ToggleLeft className="size-3 shrink-0 text-muted-foreground" />
          <span className="text-muted-foreground">
            {translate(
              'auto.components.feature.wall.BrowserDesignPayloadSummary.cookies.label',
              'Cookie import'
            )}
          </span>
          <span className="truncate font-medium">
            {translate(
              'auto.components.feature.wall.BrowserDesignPayloadSummary.cookies.value',
              'Off · choose to enable'
            )}
          </span>
        </span>
      </div>
      {/* Why: a DOM grab can include visible customer or account content even
          though credential-like attribute values are filtered. */}
      <div className="flex items-start gap-1.5 rounded-md border border-border bg-muted/30 px-2 py-1.5 text-[11px] leading-snug text-muted-foreground">
        <ShieldCheck className="mt-px size-3 shrink-0" />
        <span>
          {sent
            ? translate(
                'auto.components.feature.wall.BrowserDesignPayloadSummary.a150000005',
                'Sensitive-context boundary · captured context may include visible site content. Profile or cookie reuse is opt-in.'
              )
            : translate(
                'auto.components.feature.wall.BrowserDesignPayloadSummary.a150000004',
                'Review before sending—captured context may include visible site content. Profile or cookie reuse is opt-in.'
              )}
        </span>
      </div>
    </div>
  )
}
