import type { GlobalSettings } from '../../../../shared/types'
import { TerminalEngineEffectsSection } from './TerminalEngineEffectsSection'
import { TerminalEngineRenderingSection } from './TerminalEngineRenderingSection'
import { TerminalEngineTextSection } from './TerminalEngineTextSection'
import {
  TerminalEngineClipboardSection,
  TerminalEngineInputSection,
  TerminalEngineScrollbackSection
} from './TerminalEngineBehaviorSections'

// The dedicated aterm Terminal Engine panel: every engine-level setting in one
// place, grouped Effects / Rendering / Text / Scrollback / Input / Clipboard.
// Settings that also live in other sections (Terminal, Appearance) re-surface the
// SAME store values here — both surfaces read and write the same keys, so they
// can never disagree.

type TerminalEnginePaneProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void
  systemPrefersDark: boolean
}

export function TerminalEnginePane({
  settings,
  updateSettings,
  systemPrefersDark
}: TerminalEnginePaneProps): React.JSX.Element {
  return (
    <div className="space-y-8">
      <TerminalEngineEffectsSection
        settings={settings}
        updateSettings={updateSettings}
        systemPrefersDark={systemPrefersDark}
      />
      <TerminalEngineRenderingSection settings={settings} updateSettings={updateSettings} />
      <TerminalEngineTextSection settings={settings} updateSettings={updateSettings} />
      <TerminalEngineScrollbackSection settings={settings} updateSettings={updateSettings} />
      <TerminalEngineInputSection settings={settings} updateSettings={updateSettings} />
      <TerminalEngineClipboardSection settings={settings} updateSettings={updateSettings} />
    </div>
  )
}
