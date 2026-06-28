import { useEffect, type ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'

import { UI_LANGUAGE_SYSTEM } from '../../../shared/ui-language'
import { useAppStore } from '../store'
import { i18n, setRendererUiLanguage } from './i18n'

export function I18nProvider({ children }: { children: ReactNode }): React.JSX.Element {
  const uiLanguage = useAppStore((state) => state.settings?.uiLanguage ?? UI_LANGUAGE_SYSTEM)

  useEffect(() => {
    // Route through setRendererUiLanguage (not a bare changeLanguage) so a non-English
    // locale's lazily-split resource bundle is loaded BEFORE the switch — otherwise the
    // engine would fall back to the bundled English for any deferred locale.
    void setRendererUiLanguage(uiLanguage)
  }, [uiLanguage])

  return <I18nextProvider i18n={i18n}>{children}</I18nextProvider>
}
