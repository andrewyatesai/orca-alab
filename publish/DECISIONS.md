# Publish-boundary decisions — orc

Initial classification: 2026-07-21.

## Scope of the initial snapshot

The first public candidate is a project landing snapshot: `README.md`,
`FEATURE_WALKTHROUGH.md`, `LICENSE`, `THIRD-PARTY-NOTICES.md`, the mandatory
Gitleaks configuration, and the app icon the README embeds. It intentionally
does not publish the application source tree, build system, internal
documentation, release machinery, or development-agent instructions.

`FEATURE_WALKTHROUGH.md` ships because it is the README's guided tour and
contains no relative links outside the boundary; its provenance figures restate
the committed aterm pin manifest (verified 2026-07-21). The README's hero image
lives under centrally denied `docs/assets/` and is dropped by transform T1
rather than shipping a broken image reference.

## Why app source is excluded for now

The dev repository `andrewyatesai/orc` is itself public, so the source is
already visible under the development identity. Publishing it under the public
org is still a separate decision: the tree carries a matched submodule gitlink
(`rust/aterm`, which the exporter rejects by design), internal working notes,
platform fixtures, and a committed test-only RSA private key
(`tests/e2e/helpers/local-https-test-certificate.ts`) that public-repo secret
scanning (including this engine's mandatory gitleaks pass) will flag. A source
snapshot needs its own manifest closure, transform set, and public-clone build
strategy before it can stage.

## Versioning

The public constellation version of this snapshot is `0.1.0` (committed as
VERSION_DEFAULT in `publish/config.sh`), following the constellation's
`major.minor.dev` scheme so `promote` accepts it. The app itself versions
independently as `X.Y.Z-fork.N` (see `docs/reference/fork-versioning.md`);
release binaries (dmg/zip) are distributed via GitHub Releases on the dev
repo, not via this snapshot.

## Verification policy

The public-clone check validates the landing files exist and are non-empty; no
build is attempted because the snapshot ships no source.

## Source-publication audit (2026-07-22)

Full-source staging was requested and measured against the central guard
baseline. It is structurally blocked today on four independent walls:

1. **229 files** under shippable paths contain `/Users/<name>` fixture or
   comment paths — every one a central `forbidden-content` hit with no
   exception path.
2. The **pinned aterm WASM binaries** embed an engine doc-string containing a
   centrally forbidden term (`ultracode`); the pin system forbids altering
   these bytes, so clearing it requires an aterm-side source change, wasm
   rebuild, and re-pin.
3. Secret-shaped test fixtures (PEM headers, `ghp_`/`xox`/`AKIA`/`sk-` tokens)
   in ~7 files — gitleaks and the baseline both refuse them, by design.
4. `rust/` and `native/orca-node` cannot pass the cargo-closure guard until
   the `rust/aterm` submodule has a public home (`alabsystems/aterm`).

Until those campaigns run, the landing snapshot remains the staged boundary.
