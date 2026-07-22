# Fork Versioning, Update Feed, and Build Identity

This fork ships as **Orca Staging**, versioned on a fork-owned scheme and
updated from a fork-owned release feed. This document defines the scheme and
the ship-vehicle guarantees around it (staging-launch audit F1, F13, F14, F2).

## The `-fork.N` version scheme

```
<upstreamBase>-fork.<N>        e.g.  1.4.122-fork.1
```

- **`upstreamBase`** is the upstream Orca release the fork most recently
  merged (`1.4.122` after the v1.4.122-rc.3 re-alignment). It changes only
  when an upstream sync lands.
- **`N`** starts at 1 and increments once per fork staging cut. It never
  resets except when `upstreamBase` moves, e.g.
  `1.4.122-fork.3 → 1.4.123-fork.1`.

Ordering (semver prerelease rules, as implemented by `compareVersions` in
`src/main/updater-fallback.ts`):

- `1.4.122-fork.1 < 1.4.122-fork.2 < 1.4.123-fork.1` — fork builds upgrade
  monotonically.
- `-fork.N` versions parse as prereleases, so `isPrereleaseVersion` is true
  and default update checks include every fork tag from the atom feed (there
  is no separate stable channel for staging).
- Semver-wise `1.4.122-fork.N < 1.4.122` (a prerelease precedes its base).
  That is **safe by construction**: the updater never consults any feed that
  contains public tags (see below), so public stable can never win a version
  comparison against a fork build.

Release tags on the fork repo are `v<version>`, e.g. `v1.4.122-fork.1`.

## Update feed: fork-owned, dormant-if-unconfigured (F1)

All updater endpoints derive from one constant —
`UPDATE_FEED_REPO_SLUG = 'andrewyatesai/orca-alab'` in
`src/main/updater-feed-endpoints.ts` — which must match the `publish` block in
`config/electron-builder.config.cjs`. There are no other feed URLs in the
updater; the atom feed, the pinned `releases/download/<tag>` feeds, and the
`releases/latest/download` fallback are all built from that slug.

**Dormant posture:** if the slug is blank (or set to the public
`stablyai/orca` repo, which `isUpdateFeedConfigured()` rejects), the updater
wires nothing at startup — no feed, no handlers, no timers — and both manual
and background checks answer "not available" without touching the network.
The updater never falls back to any public URL. If the fork repo simply has
no releases yet, checks fail benignly (electron-updater's missing-manifest
errors are classified as benign) and stay quiet.

The upstream vendor endpoints on `onorca.dev` (update **nudge** campaigns and
the rich **changelog** card) have no fork equivalent; their URLs are `null` in
`updater-feed-endpoints.ts` and both features are dormant. Point them at
fork-owned services before enabling.

## Windows update signatures: fail-closed (F13)

The fork has no Windows code-signing identity. Upstream's
`verifyUpdateCodeSignature` bypass (accept everything) has been replaced with
a fail-closed override in `src/main/updater.ts`: every downloaded Windows
update is **rejected** at signature-verification time and the install is
skipped, with the reason logged.

Consequences for the staging cohort:

- Windows installs update by **manual reinstall** of the new
  `orca-windows-setup.exe`, not through the in-app updater.
- Installers are unsigned, so **SmartScreen will warn** ("Windows protected
  your PC") on first run; users must choose "More info → Run anyway".

Remove the override only when fork-signed builds exist and
`win.signtoolOptions.publisherName` in `config/electron-builder.config.cjs`
names the fork's certificate — electron-updater's default verification then
checks against that publisher.

## Build identity (F14)

Fork builds default to a distinct identity so they install and run
side-by-side with public Orca instead of impersonating it:

| | Fork default | `ORCA_PUBLIC_IDENTITY=1` |
|---|---|---|
| appId / bundle id | `com.stablyai.orca.staging` | `com.stablyai.orca` |
| productName | `Orca Staging` | `Orca` |
| Windows AUMID | `com.stablyai.orca.staging` | `com.stablyai.orca` |
| userData | `…/Orca Staging` | public Orca's |

`ORCA_PUBLIC_IDENTITY=1` exists **only** for producing upstream-identity diff
builds; never ship it to the staging cohort. The runtime side lives in
`src/main/startup/dev-instance-identity.ts`, which reads the productName that
electron-builder's `extraMetadata` injected into the packaged package.json —
Electron derives `app.name`, the default userData directory, and the
single-instance lock namespace from that file, so identity, state isolation,
and the lock all fork together.

Known limitation: Linux packages keep the upstream `orca-ide` deb/rpm package
name and executable name, so a staging deb **replaces** an installed public
deb (userData still stays isolated). See remaining-work notes before shipping
Linux staging artifacts. The `after-install.sh` CLI symlink also does not yet
probe the `/opt/Orca Staging` install dir.

## macOS multi-arch builds (F2)

`config/scripts/build-rust-daemon.mjs` and
`config/scripts/build-terminal-addon.mjs` honor the same arch contract as
`config/electron-builder.config.cjs`:

- Default (dev): host-arch-only plain `cargo build`.
- `ORCA_MAC_RELEASE=1`: per-target builds for `x86_64-apple-darwin` +
  `aarch64-apple-darwin`, lipo-merged into the single artifact path the
  packager consumes (`rust/target/release/orca-daemon`,
  `native/orca-node/orca_node.node`).
- `ORCA_MAC_BUILD_ARCHES=x64,arm64` (or a single foreign arch): same
  per-target path for ad-hoc builds. Requires the rustup targets
  (`rustup target add x86_64-apple-darwin aarch64-apple-darwin --toolchain stable`);
  the scripts fail fast with that instruction when missing.

An `afterPack` assertion (`config/scripts/assert-bundled-binary-arch.cjs`)
verifies `orca-daemon` and `orca_node.node` inside every packaged bundle
actually contain the bundle's architecture (lipo on macOS, ELF/PE header
parsing elsewhere) and **fails the build** on mismatch — a wrong-arch binary
can never ship silently again.
