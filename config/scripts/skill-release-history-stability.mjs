import { isDeepStrictEqual } from 'node:util'

const SNAPSHOT_REGISTRY_SCHEMA_VERSION = 1
const RELEASE_MAPPING_SCHEMA_VERSION = 1

function hasCompatibleHistory(registry, mapping) {
  return (
    registry?.schemaVersion === SNAPSHOT_REGISTRY_SCHEMA_VERSION &&
    mapping?.schemaVersion === RELEASE_MAPPING_SCHEMA_VERSION
  )
}

function snapshotAt(registry, name, revision) {
  return registry.skills[name]?.find((snapshot) => snapshot.releaseRevision === revision)
}

function sameSnapshotIdentity(left, right) {
  if (!left || !right) {
    return false
  }
  const { releaseRevision: _leftRevision, ...leftIdentity } = left
  const { releaseRevision: _rightRevision, ...rightIdentity } = right
  return isDeepStrictEqual(leftIdentity, rightIdentity)
}

function releasedRevisions(mapping) {
  const revisions = new Map()
  for (const release of mapping.releases ?? []) {
    for (const [name, revision] of Object.entries(release.skills ?? {})) {
      const skillRevisions = revisions.get(name) ?? new Set()
      skillRevisions.add(revision)
      revisions.set(name, skillRevisions)
    }
  }
  return revisions
}

function seedReleasedRegistry(committedRegistry, committedMapping) {
  const registry = { schemaVersion: SNAPSHOT_REGISTRY_SCHEMA_VERSION, skills: {} }
  for (const [name, revisions] of releasedRevisions(committedMapping)) {
    const snapshots = [...revisions]
      .sort((left, right) => left - right)
      .map((revision) => {
        const snapshot = snapshotAt(committedRegistry, name, revision)
        if (!snapshot) {
          throw new Error(
            `Committed release mapping references unknown snapshot ${name}@${revision}.`
          )
        }
        return snapshot
      })
    registry.skills[name] = snapshots
  }
  return registry
}

function appendSnapshot(registry, name, source) {
  const snapshots = registry.skills[name] ?? []
  const releaseRevision =
    snapshots.reduce((maximum, snapshot) => Math.max(maximum, snapshot.releaseRevision), 0) + 1
  const snapshot = { ...source, releaseRevision }
  snapshots.push(snapshot)
  registry.skills[name] = snapshots
  return snapshot
}

// Why: late-fetched historical tags may insert identities before already shipped
// revisions; keep the shipped ledger stable and append only newly discovered bytes.
function stabilizeReleasedHistory(derived, committedRegistry, committedMapping) {
  if (!hasCompatibleHistory(committedRegistry, committedMapping)) {
    return derived
  }

  const registry = seedReleasedRegistry(committedRegistry, committedMapping)
  const mapping = { schemaVersion: RELEASE_MAPPING_SCHEMA_VERSION, releases: [] }
  const committedByVersion = new Map(
    committedMapping.releases.map((release) => [release.appVersion, release])
  )
  const encounteredCommittedVersions = new Set()
  const previousRevisionBySkill = new Map()

  for (const release of derived.mapping.releases) {
    const committedRelease = committedByVersion.get(release.appVersion)
    const revisions = {}
    for (const [name, rawRevision] of Object.entries(release.skills)) {
      const rawSnapshot = snapshotAt(derived.registry, name, rawRevision)
      if (!rawSnapshot) {
        throw new Error(
          `Derived release mapping references unknown snapshot ${name}@${rawRevision}.`
        )
      }

      let stableSnapshot
      if (committedRelease) {
        const committedRevision = committedRelease.skills[name]
        const committedSnapshot = snapshotAt(committedRegistry, name, committedRevision)
        if (
          !Number.isInteger(committedRevision) ||
          !sameSnapshotIdentity(rawSnapshot, committedSnapshot)
        ) {
          throw new Error(
            `Released snapshot history changed for ${name} at ${release.appVersion}. ` +
              'Released snapshots are append-only.'
          )
        }
        stableSnapshot = committedSnapshot
      } else {
        const previousRevision = previousRevisionBySkill.get(name)
        const previousSnapshot = snapshotAt(registry, name, previousRevision)
        stableSnapshot = sameSnapshotIdentity(rawSnapshot, previousSnapshot)
          ? previousSnapshot
          : appendSnapshot(registry, name, rawSnapshot)
      }
      revisions[name] = stableSnapshot.releaseRevision
      previousRevisionBySkill.set(name, stableSnapshot.releaseRevision)
    }

    if (committedRelease) {
      encounteredCommittedVersions.add(release.appVersion)
      if (!isDeepStrictEqual(revisions, committedRelease.skills)) {
        throw new Error(`Released skill mapping changed for ${release.appVersion}.`)
      }
    }
    mapping.releases.push({ appVersion: release.appVersion, skills: revisions })
  }

  const missingVersions = [...committedByVersion.keys()].filter(
    (version) => !encounteredCommittedVersions.has(version)
  )
  if (missingVersions.length > 0) {
    throw new Error(
      `Released snapshot history is incomplete at ${missingVersions[0]}. ` +
        'Fetch all release tags before regenerating skill artifacts.'
    )
  }
  return { registry, mapping }
}

// Why: only revisions named by a committed release mapping have shipped;
// trailing working-tree snapshots may be replaced before their first release.
function assertReleasedHistoryPreserved(committedRegistry, committedMapping, artifacts) {
  if (!hasCompatibleHistory(committedRegistry, committedMapping)) {
    return
  }
  for (const [name, revisions] of releasedRevisions(committedMapping)) {
    for (const revision of revisions) {
      const committed = snapshotAt(committedRegistry, name, revision)
      const rebuilt = snapshotAt(artifacts.snapshotRegistry, name, revision)
      if (!committed || !rebuilt || !isDeepStrictEqual(rebuilt, committed)) {
        throw new Error(
          `Released snapshot history changed for ${name} at revision ${revision}. ` +
            'Released snapshots are append-only.'
        )
      }
    }
  }
}

export { assertReleasedHistoryPreserved, stabilizeReleasedHistory }
