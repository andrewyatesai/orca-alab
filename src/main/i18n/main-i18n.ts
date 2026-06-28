import { app } from 'electron'
import i18next, {
  type BackendModule,
  type i18n as I18nInstance,
  type ReadCallback,
  type TOptions
} from 'i18next'

import en from '../../renderer/src/i18n/locales/en.json'
import { isPseudoLocalizationLocale, pseudoLocalizeString } from '../../shared/pseudo-localization'
import { DEFAULT_UI_LOCALE, resolveUiLocale, type SupportedUiLocale } from '../../shared/ui-locale'
import { UI_LANGUAGE_SYSTEM, type UiLanguage } from '../../shared/ui-language'

export const mainI18n: I18nInstance = i18next.createInstance()

let initialized = false

// English is bundled inline (the menu/tray default + sync fallback); every other locale
// is split into an on-demand chunk and fetched the first time the main process switches
// to it, so launch doesn't eagerly parse ~2MB of unused locale JSON. A backend (rather
// than a wrapper) loads the bundle transparently on changeLanguage.
const lazyLocaleLoaders: Partial<Record<SupportedUiLocale, () => Promise<{ default: unknown }>>> = {
  es: () => import('../../renderer/src/i18n/locales/es.json'),
  ja: () => import('../../renderer/src/i18n/locales/ja.json'),
  ko: () => import('../../renderer/src/i18n/locales/ko.json'),
  zh: () => import('../../renderer/src/i18n/locales/zh.json')
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

export function getMainSystemLocale(): string {
  try {
    return app.getLocale()
  } catch {
    return DEFAULT_UI_LOCALE
  }
}

export async function ensureMainI18n(): Promise<I18nInstance> {
  if (!initialized) {
    await mainI18n.use(lazyLocaleBackend).init({
      fallbackLng: DEFAULT_UI_LOCALE,
      lng: DEFAULT_UI_LOCALE,
      // The bundled English is a partial resource set; the backend fills in other locales.
      partialBundledLanguages: true,
      resources: {
        en: {
          translation: en
        }
      },
      interpolation: {
        escapeValue: false
      }
    })
    initialized = true
  }
  return mainI18n
}

export async function setMainUiLanguage(language: UiLanguage): Promise<SupportedUiLocale> {
  await ensureMainI18n()
  const locale = resolveUiLocale(
    language,
    language === UI_LANGUAGE_SYSTEM ? getMainSystemLocale() : DEFAULT_UI_LOCALE
  )
  if (mainI18n.language !== locale) {
    // changeLanguage triggers the lazy backend to load the locale before it resolves.
    await mainI18n.changeLanguage(locale)
  }
  return locale
}

export function translateMain(key: string, fallback: string, options?: TOptions): string {
  // Why: menu registration can run before async init finishes in tests; fall back
  // to the English default instead of returning undefined from an uninitialized i18n.
  const raw = initialized ? mainI18n.t(key, { defaultValue: fallback, ...options }) : fallback
  const value = typeof raw === 'string' && raw.length > 0 ? raw : fallback
  return isPseudoLocalizationLocale(mainI18n.language) ? pseudoLocalizeString(value) : value
}
