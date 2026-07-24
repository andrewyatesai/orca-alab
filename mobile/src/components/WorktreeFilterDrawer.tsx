import { View, Text, Pressable, StyleSheet } from 'react-native'
import { Check } from 'lucide-react-native'
import { BottomDrawer } from './BottomDrawer'
import { colors, spacing, typography } from '../theme/mobile-theme'
import type { FilterState } from '../worktree/workspace-list-types'
import type { WorkspaceSectionRepo } from '../worktree/use-workspace-sections'

type WorktreeFilterDrawerProps = {
  visible: boolean
  onClose: () => void
  activeFilterCount: number
  onClearFilters: () => void
  filters: FilterState
  onToggleHideSleeping: () => void
  onToggleHideDefaultBranch: () => void
  uniqueRepos: WorkspaceSectionRepo[]
  onToggleRepoFilter: (repoId: string) => void
}

export function WorktreeFilterDrawer({
  visible,
  onClose,
  activeFilterCount,
  onClearFilters,
  filters,
  onToggleHideSleeping,
  onToggleHideDefaultBranch,
  uniqueRepos,
  onToggleRepoFilter
}: WorktreeFilterDrawerProps) {
  return (
    <BottomDrawer visible={visible} onClose={onClose}>
      <View style={styles.filterModalHeader}>
        <Text style={styles.filterModalTitle}>Filter</Text>
        {activeFilterCount > 0 && (
          <Pressable onPress={onClearFilters}>
            <Text style={styles.clearFiltersText}>Clear filters</Text>
          </Pressable>
        )}
      </View>

      <Text style={styles.filterSectionLabel}>Workspaces</Text>
      <View style={styles.filterGroup}>
        <Pressable style={styles.filterRow} onPress={onToggleHideSleeping}>
          <Text style={styles.filterRowText}>Hide sleeping</Text>
          {filters.hideSleeping && <Check size={14} color={colors.textPrimary} />}
        </Pressable>
        <View style={styles.filterSeparator} />
        <Pressable style={styles.filterRow} onPress={onToggleHideDefaultBranch}>
          <Text style={styles.filterRowText}>Hide default branch</Text>
          {filters.hideDefaultBranch && <Check size={14} color={colors.textPrimary} />}
        </Pressable>
      </View>

      {uniqueRepos.length > 1 && (
        <>
          <Text style={styles.filterSectionLabel}>Repositories</Text>
          <View style={styles.filterGroup}>
            {uniqueRepos.map((repo, i) => (
              <View key={repo.id}>
                {i > 0 && <View style={styles.filterSeparator} />}
                <Pressable style={styles.filterRow} onPress={() => onToggleRepoFilter(repo.id)}>
                  <View style={[styles.filterRepoDot, { backgroundColor: repo.color }]} />
                  <Text style={styles.filterRowText} numberOfLines={1}>
                    {repo.name}
                  </Text>
                  {filters.filterRepoIds.has(repo.id) && (
                    <Check size={14} color={colors.textPrimary} />
                  )}
                </Pressable>
              </View>
            ))}
          </View>
        </>
      )}
    </BottomDrawer>
  )
}

const styles = StyleSheet.create({
  filterModalHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: spacing.xs,
    marginBottom: spacing.md
  },
  filterModalTitle: {
    fontSize: 15,
    fontWeight: '600',
    color: colors.textPrimary
  },
  clearFiltersText: {
    fontSize: 13,
    color: colors.textSecondary
  },
  filterSectionLabel: {
    fontSize: 11,
    fontWeight: '600',
    color: colors.textMuted,
    textTransform: 'uppercase',
    letterSpacing: 0.5,
    marginBottom: spacing.xs,
    paddingHorizontal: spacing.xs
  },
  filterGroup: {
    backgroundColor: colors.bgPanel,
    borderRadius: 12,
    overflow: 'hidden',
    marginBottom: spacing.md
  },
  filterRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingVertical: spacing.md,
    paddingHorizontal: spacing.md + 2,
    gap: spacing.sm
  },
  filterRowText: {
    flex: 1,
    fontSize: typography.bodySize,
    color: colors.textPrimary
  },
  filterSeparator: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.borderSubtle,
    marginHorizontal: spacing.md
  },
  filterRepoDot: {
    width: 8,
    height: 8,
    borderRadius: 4
  }
})
