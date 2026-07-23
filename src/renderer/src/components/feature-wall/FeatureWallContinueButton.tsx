import type { JSX } from 'react'
import { ShortcutKeyCombo } from '@/components/ShortcutKeyCombo'
import { Button } from '@/components/ui/button'

export function FeatureWallContinueButton(props: {
  label: string
  enableKeyboardShortcut: boolean
  shortcutModifierLabel: string
  onClick: () => void
}): JSX.Element {
  const ariaKeyShortcut = navigator.userAgent.includes('Mac') ? 'Meta+Enter' : 'Control+Enter'
  return (
    <Button
      type="button"
      variant="default"
      className="gap-2 px-5"
      aria-keyshortcuts={props.enableKeyboardShortcut ? ariaKeyShortcut : undefined}
      onClick={props.onClick}
    >
      {props.label}
      {props.enableKeyboardShortcut ? (
        <span aria-hidden>
          {/* Why: aria-keyshortcuts already exposes the binding; hiding the
              decorative caps keeps the button's accessible name concise. */}
          <ShortcutKeyCombo
            keys={[props.shortcutModifierLabel, 'Enter']}
            className="ml-1 gap-0.5"
            separatorClassName="mx-0 text-[10px] text-primary-foreground/70"
            keyCapClassName="min-w-0 border-primary-foreground/20 bg-primary-foreground/10 px-1 py-0.5 text-[10px] leading-none text-primary-foreground shadow-none"
          />
        </span>
      ) : null}
    </Button>
  )
}
