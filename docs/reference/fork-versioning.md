# Fork Versioning, Update Feed, and Build Identity

This development repository builds **Orca: ALab Edition**, versioned on an
ALab-owned scheme and updated from the public ALab release feed. Development
source lives at `andrewyatesai/orca-alab`; release artifacts and update
manifests live separately at `alabsystems/orca-alab`. Neither location is the
production Orca vendor repository (`stablyai/orca`).

This document defines the scheme and the ship-vehicle guarantees around it
(staging-launch audit F1, F13, F14, F2).

## The ALab `MAJOR.MINOR.0` version scheme

```
<major>.<minor>.0              e.g.  0.3.0
```

- ALab Edition versions itself **independently of upstream**. The line
  restarted at `0.1.0` in `66d754673`, and `package.json`'s top-level
  `"version"` is the one authoritative value — read it there rather than trusting
  this sentence, which was last checked at **0.3.0** on 2026-08-30. `pub version`
  and `pub bump` read and write that field, and the mirror-head promote reads it
  from the source commit to name, tag, and ledger-map each release.
- The patch slot is **pinned at 0**. `latestStableDesktopReleaseTag`
  (`config/scripts/latest-stable-release.mjs`) keys the release line on that
  zero patch slot, so imported upstream-style tags (patch != 0) can never
  advance an ALab install.
- **`MINOR`** increments once per ALab cut. Upstream alignment is tracked
  separately, in prose and in the merge history — it is not encoded in the
  version.

Release tags on `alabsystems/orca-alab` are `v<version>`, e.g. `v0.3.0`.
Only `-rc.N` tags carry GitHub's prerelease flag
(`isDesktopReleasePrerelease` in `config/scripts/desktop-release-tag.mjs`);
every other cut publishes as a full GitHub release so GitHub's `latest`
endpoint and the updater fallback advance with the ALab train.

Ordering still runs through `compareVersions` in
`src/main/updater-fallback.ts`, but with no prerelease suffix in play a plain
numeric comparison decides it. This is **safe by construction**: the updater
consults the ALab release feed, not the production vendor feed, so an upstream
production tag can never win a version comparison against an ALab build.

### The retired `-fork.N` scheme

Builds used to be versioned `<upstreamBase>-fork.<N>` — last published as
`v1.4.147-fork.1`. `7277b375e` retired it: SemVer reads `-fork.N` as a
prerelease, which satisfies no plain version constraint, and it paired with a
nonzero patch slot that this line pins at 0. Two gates had to be re-anchored in
the same commit or the change would have failed silently — the updater's
release-line filter (which keyed on `fork !== null`) and the telemetry staging
preflight (which classified a build as staging from the suffix, and now fails
closed: every build is staging unless `ORCA_STAGING=0`).

`parseDesktopReleaseTag` still **parses** `-fork.N` so already-published tags
keep resolving, and the tests in `config/scripts/latest-stable-release.test.mjs`
hold that behaviour. Nothing cuts new `-fork.N` tags.

### Release gate and artifact profiles

Release notes are generated from `andrewyatesai/orca-alab`, while draft
releases and assets live in `alabsystems/orca-alab`. Draft creation fails closed
unless the exact tag already exists in both repositories; GitHub is never
allowed to synthesize a missing public tag from its default branch.

The publication gate defaults to
`ORCA_RELEASE_ASSET_PROFILE=alab-macos`, matching the current nine-asset,
dual-architecture macOS release. A maintainer can explicitly select
`alab-full` only after current macOS, Linux, and Windows outputs are uploaded.
`legacy-full` is reserved for old upstream-style artifacts and is the only
profile that requires RPMs. Each update manifest must name same-release assets,
match the tag version, and carry SHA-512 values that match the uploaded bytes.
That integrity check intentionally streams every manifest-referenced archive
and installer before publication.

## Update feed: ALab-owned, dormant-if-unconfigured (F1)

All updater endpoints derive from one constant —
`UPDATE_FEED_REPO_SLUG = 'alabsystems/orca-alab'` in
`src/main/updater-feed-endpoints.ts` — which must match the `publish` block in
`config/electron-builder.config.cjs`. There are no other feed URLs in the
updater; the atom feed, the pinned `releases/download/<tag>` feeds, and the
`releases/latest/download` fallback are all built from that slug.

**Dormant posture:** if the slug is blank, the updater wires nothing at
startup — no feed, no handlers, no timers — and both manual and background
checks answer "not available" without touching the network. The checked-in
slug is the ALab release repository, and the updater never falls back to the
production vendor feed. If that release repository has no releases yet,
checks fail benignly (electron-updater's missing-manifest errors are
classified as benign) and stay quiet.

The upstream vendor endpoints on `onorca.dev` (update **nudge** campaigns and
the rich **changelog** card) have no fork equivalent; their URLs are `null` in
`updater-feed-endpoints.ts` and both features are dormant. Point them at
fork-owned services before enabling.

### Platform install modes

`getUpdateInstallMode()` in `src/main/updater-install-policy.ts` returns one of
`'automatic' | 'manual' | 'bundle-swap'`. Every mode except `'automatic'`
resolves the target release itself instead of letting electron-updater's
provider decide (`usesSelfManagedCheck`).

- **macOS — `'bundle-swap'`** (since `0988d0605`). Orca checks the public ALab
  release feed itself and applies the update **in place by swapping the `.app`
  bundle** (`src/main/updater-bundle-swap.ts`), rather than through
  Squirrel.Mac. Squirrel refuses an update whose code signature does not match
  the running app, so it cannot serve an ad-hoc-signed build at all; swapping
  the bundle ourselves makes signing a distribution concern, not an update one.
  Trust is tiered: SHA-512 over the downloaded artifact is **always** enforced,
  and an Ed25519 signature over the feed is additionally enforced whenever a
  public key was pinned at build time. Startup sweeps any bundle left beside the
  app by a swap interrupted by a crash or power loss. macOS may still ask users
  to approve privacy permissions again after installing a rebuilt or newer ALab
  bundle, because its ad-hoc code identity is not stable across builds.
- **Windows — `'manual'`:** manual reinstall until ALab has a code-signing
  publisher, as detailed below.
- **Linux — `'manual'`:** also manual. Linux spans four package topologies
  (writable AppImage, root-owned AppImage, deb, rpm) that each need a different
  apply path; none is built yet, so the native `electron-updater` flow is **not**
  enabled there either.

The native updater stays dormant under every self-managed mode, logging
`install mode '<mode>'; native updater stays dormant` (`src/main/updater.ts`).

## Windows updates: manual until signed (F13)

The fork has no Windows code-signing identity. Orca therefore uses the
self-managed release-discovery flow and never initializes the native Windows
updater. This avoids downloading an installer that cannot be authenticated
against an ALab publisher. (macOS shares the discovery half of that flow but
now applies the result itself — see the bundle-swap mode above.)

Consequences for the staging cohort:

- Windows installs update by **manual reinstall** of the new
  `orca-windows-setup.exe`, not through the in-app updater.
- Installers are unsigned, so **SmartScreen will warn** ("Windows protected
  your PC") on first run; users must choose "More info → Run anyway".

Enable native installation only when fork-signed builds exist and
`win.signtoolOptions.publisherName` in `config/electron-builder.config.cjs`
names the fork's certificate — electron-updater's default verification then
checks against that publisher.

## Build identity (F14)

Fork builds default to a distinct identity so they install and run
side-by-side with public Orca instead of impersonating it:

|                   | Fork default                | `ORCA_PUBLIC_IDENTITY=1` |
| ----------------- | --------------------------- | ------------------------ |
| appId / bundle id | `com.stablyai.orca.staging` | `com.stablyai.orca`      |
| productName       | `Orca ALab Edition`         | `Orca`                   |
| Windows AUMID     | `com.stablyai.orca.staging` | `com.stablyai.orca`      |
| userData          | `…/Orca ALab Edition`       | public Orca's            |

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
probe the `/opt/Orca ALab Edition` install dir.

## macOS multi-arch builds (F2)

`config/scripts/build-rust-daemon.mjs` and
`config/scripts/build-terminal-addon.mjs` honor the same arch contract as
`config/electron-builder.config.cjs`:

- Default (dev): host-arch-only `targo --unverified build` (Trust toolchain).
- `ORCA_MAC_RELEASE=1`: per-target builds for `x86_64-apple-darwin` +
  `aarch64-apple-darwin`, lipo-merged into the single artifact path the
  packager consumes (`rust/target/release/orca-daemon`,
  `native/orca-node/orca_node.node`).
- `ORCA_MAC_BUILD_ARCHES=x64,arm64` (or a single foreign arch): same
  per-target path for ad-hoc builds. The `aarch64-apple-darwin` slice builds
  with targo; the `x86_64-apple-darwin` slice is a labelled STOCK EXCEPTION
  (the Trust toolchain ships no x86_64-apple-darwin std) and needs an installed
  stock toolchain with that target
  (`rustup target add x86_64-apple-darwin --toolchain stable`); the scripts
  never install one and fail fast with that instruction when it is missing.

An `afterPack` assertion (`config/scripts/assert-bundled-binary-arch.cjs`)
verifies `orca-daemon` and `orca_node.node` inside every packaged bundle
actually contain the bundle's architecture (lipo on macOS, ELF/PE header
parsing elsewhere) and **fails the build** on mismatch — a wrong-arch binary
can never ship silently again.
