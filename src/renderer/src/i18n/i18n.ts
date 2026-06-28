import i18next, {
  type BackendModule,
  type i18n as I18nInstance,
  type ReadCallback,
  type TOptions
} from 'i18next'
import { initReactI18next } from 'react-i18next'

import en from './locales/en.json'
import { isPseudoLocalizationLocale, pseudoLocalizeString } from './pseudo-localization'
import { DEFAULT_LOCALE, resolveUiLocale } from './supported-languages'
import type { SupportedUiLocale } from '../../../shared/ui-locale'
import type { UiLanguage } from '../../../shared/ui-language'

export const i18n: I18nInstance = i18next.createInstance()

// English is bundled inline (synchronous first paint + always-present fallback). Every
// other ~0.5MB locale is split into an on-demand chunk and fetched the first time i18next
// switches to it, keeping ~2MB out of the eager renderer bundle. Hooking the load into a
// backend (rather than a wrapper) means every switch path — provider, settings, tests,
// direct changeLanguage — loads the bundle transparently before resolving.
const lazyLocaleLoaders: Partial<Record<SupportedUiLocale, () => Promise<{ default: unknown }>>> = {
  es: () => import('./locales/es.json'),
  ja: () => import('./locales/ja.json'),
  ko: () => import('./locales/ko.json'),
  zh: () => import('./locales/zh.json')
}

const lazyLocaleBackend: BackendModule = {
  type: 'backend',
  init: () => {},
  read: (language: string, _namespace: string, callback: ReadCallback): void => {
    const loader = lazyLocaleLoaders[language as SupportedUiLocale]
    if (!loader) {
      // 'en' is preloaded; the pseudo locale + anything unknown fall back to English.
      callback(null, {})
      return
    }
    loader()
      .then((mod) => callback(null, mod.default as Record<string, unknown>))
      .catch((error: unknown) =>
        callback(error instanceof Error ? error : new Error(String(error)), null)
      )
  }
}

void i18n
  .use(lazyLocaleBackend)
  .use(initReactI18next)
  .init({
    fallbackLng: DEFAULT_LOCALE,
    lng: DEFAULT_LOCALE,
    // The bundled English is a partial resource set; the backend fills in other locales.
    partialBundledLanguages: true,
    resources: {
      en: {
        translation: en
      }
    },
    interpolation: {
      escapeValue: false
    },
    react: {
      useSuspense: false
    }
  })

export function translate(key: string, fallback: string, options?: TOptions): string {
  const value = i18n.t(key, { defaultValue: fallback, ...options })
  return isPseudoLocalizationLocale(i18n.language) ? pseudoLocalizeString(value) : value
}

export async function setRendererUiLanguage(language: UiLanguage): Promise<void> {
  const locale = resolveUiLocale(language)
  if (i18n.language !== locale) {
    // changeLanguage triggers the lazy backend to load the locale before it resolves.
    await i18n.changeLanguage(locale)
  }
}
