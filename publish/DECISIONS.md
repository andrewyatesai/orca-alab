# Publish-boundary decisions — orca-alab development

Initial classification: 2026-07-21.

## Scope of the initial snapshot

The public candidate is a project landing snapshot: `README.md`, `LICENSE`,
`NOTICE`, `THIRD-PARTY-NOTICES.md`, the mandatory Gitleaks configuration, and
the app icon and hero image the README embeds. It intentionally does not publish
the application source tree, walkthrough, build system, internal documentation,
release machinery, or development-agent instructions.

`FEATURE_WALKTHROUGH.md` stays in the development repository because its
provenance commands and file citations require the full source tree.

## Why app source is excluded for now

The dev repository `andrewyatesai/orca-alab` is itself public, so the source is
already visible under the development identity. Publishing it under the public
org is still a separate decision: the tree carries a matched submodule gitlink
(`rust/aterm`, which the exporter rejects by design), internal working notes,
platform fixtures, and a committed test-only RSA private key
(`tests/e2e/helpers/local-https-test-certificate.ts`) that public-repo secret
scanning (including this engine's mandatory gitleaks pass) will flag. A source
snapshot needs its own manifest closure, transform set, and public-clone build
strategy before it can stage.

## Versioning

The public constellation version of this snapshot is `0.2.0` (committed as
VERSION_DEFAULT in `publish/config.sh`), following the constellation's
`major.minor.dev` scheme so `promote` accepts it. The app itself versions
independently as `X.Y.Z-fork.N` (see `docs/reference/fork-versioning.md`);
release binaries (dmg/zip) are distributed through the public ALab release
repository `alabsystems/orca-alab`, not from the development source repository
or via this landing-snapshot pipeline.

## Verification policy

The public-clone check validates the landing files exist and are non-empty; no
build is attempted because the snapshot ships no source.

## Source-publication audit (2026-07-22)

Full-source staging was requested and measured against the central guard
baseline. It is structurally blocked today on three independent walls:

1. **229 files** under shippable paths contain `/Users/<name>` fixture or
   comment paths — every one a central `forbidden-content` hit with no
   exception path.
2. The **pinned aterm WASM binaries** embed an engine doc-string containing a
   centrally forbidden term (`ultracode`); the pin system forbids altering
   these bytes, so clearing it requires an aterm-side source change, wasm
   rebuild, and re-pin.
3. Secret-shaped test fixtures (PEM headers, `ghp_`/`xox`/`AKIA`/`sk-` tokens)
   in ~7 files — gitleaks and the baseline both refuse them, by design.
   The former fourth wall is closed: `rust/aterm` now has the public home
   `alabsystems/aterm`, and the pinned submodule uses it.

Until the remaining three campaigns run, the landing snapshot remains the
staged boundary.

## v0.2.0 boundary revision (2026-07-22 release audit)

The v0.1.0 snapshot shipped the dev repo's README/walkthrough verbatim, which
made false claims on a 6-file snapshot (build instructions with no source and
"built from this repository"). Fixes:

- Transform T1 now **replaces** README.md at export with a purpose-written
  public landing page: downloads point at THIS repo's Releases (binaries are
  mirrored there, since the org rewrite forbids referencing the dev org), the
  two version lines (v0.x snapshot tags vs 1.4.x-fork.N app versions) are
  explained, and aterm links to its public `alabsystems/aterm` repository.
- FEATURE_WALKTHROUGH.md is **excluded** until a public-appropriate edition
  exists — its provenance commands and file citations dangle without source.
- The README hero image moved to `resources/readme-hero.jpg` (exported), so
  the landing page keeps its product visual.
- Relicensed: LICENSE is Apache-2.0, NOTICE carries fork copyright, upstream
  MIT notice preserved in THIRD-PARTY-NOTICES.md (which also re-quotes the
  aterm NOTICE at the current pin).
