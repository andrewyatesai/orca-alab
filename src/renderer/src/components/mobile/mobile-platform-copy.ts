import type { Platform } from './MobileHero'
import { translate } from '@/i18n/i18n'

// Why: ALab does not publish mobile binaries yet, so every install target must
// identify the compatible upstream artifact instead of implying ALab ownership.
export type IosChannel = 'stable' | 'preview'

export type InstallCopy = { ctaLabel: string; url: string }

const IOS_CHANNEL_COPY: Record<IosChannel, InstallCopy> = {
  stable: {
    ctaLabel: 'Open upstream App Store',
    url: 'https://apps.apple.com/app/orca-ide/id6766130217'
  },
  preview: {
    ctaLabel: 'Open upstream TestFlight',
    url: 'https://testflight.apple.com/join/YjeGMQBA'
  }
}

const ANDROID_COPY: InstallCopy = {
  // Why: ALab currently relies on the compatible upstream mobile binary;
  // its public release repository does not publish an Android artifact.
  ctaLabel: 'Download upstream APK',
  url: 'https://github.com/stablyai/orca/releases/download/mobile-android-v0.0.31/app-release.apk'
}

export function getInstallCopy(platform: Platform, iosChannel: IosChannel): InstallCopy {
  return platform === 'ios' ? IOS_CHANNEL_COPY[iosChannel] : ANDROID_COPY
}

export function getChannelTagline(iosChannel: IosChannel): string {
  return iosChannel === 'preview'
    ? translate(
        'auto.components.mobile.mobile.platform.copy.preview.tagline',
        'Compatible upstream preview, updated daily.'
      )
    : translate(
        'auto.components.mobile.mobile.platform.copy.stable.tagline',
        'Compatible upstream release, updated weekly.'
      )
}
