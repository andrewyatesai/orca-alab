//! GitLab "recent projects" list logic, ported from `src/shared/gitlab-projects.ts`.
//!
//! Pure recents computation kept out of the IPC handler so it is testable
//! without the Store. The clock is injected as an ISO timestamp string (the TS
//! `now.toISOString()`), keeping this side IO-free.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitLabRecentEntry {
    pub host: String,
    pub path: String,
    pub last_opened_at: String,
}

/// Default max recents kept before older entries fall off.
pub const GITLAB_RECENTS_MAX: usize = 10;

/// Next `recent` list after opening the project at (`host`, `path`):
/// most-recent-first, deduped by host+path, capped at `max`. Returns a fresh
/// vector (the input is borrowed, never mutated).
pub fn compute_next_gitlab_recents(
    existing: &[GitLabRecentEntry],
    host: &str,
    path: &str,
    now_iso: &str,
    max: usize,
) -> Vec<GitLabRecentEntry> {
    let mut result = Vec::with_capacity(existing.len() + 1);
    result.push(GitLabRecentEntry {
        host: host.to_string(),
        path: path.to_string(),
        last_opened_at: now_iso.to_string(),
    });
    // Filter before prepend so re-opening an already-recent project moves it to
    // the front rather than duplicating.
    for entry in existing {
        if !(entry.host == host && entry.path == path) {
            result.push(entry.clone());
        }
    }
    result.truncate(max);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-05-08T10:00:00.000Z";

    fn entry(host: &str, path: &str, last_opened_at: &str) -> GitLabRecentEntry {
        GitLabRecentEntry {
            host: host.to_string(),
            path: path.to_string(),
            last_opened_at: last_opened_at.to_string(),
        }
    }

    #[test]
    fn prepends_a_fresh_entry_to_an_empty_list() {
        assert_eq!(
            compute_next_gitlab_recents(&[], "gitlab.com", "g/p", NOW, GITLAB_RECENTS_MAX),
            vec![entry("gitlab.com", "g/p", NOW)]
        );
    }

    #[test]
    fn moves_an_existing_entry_to_the_front_dedupes_by_host_and_path() {
        let existing = vec![
            entry("gitlab.com", "a/b", "2026-05-07"),
            entry("gitlab.com", "g/p", "2026-05-06"),
            entry("gitlab.com", "c/d", "2026-05-05"),
        ];
        let result = compute_next_gitlab_recents(&existing, "gitlab.com", "g/p", NOW, GITLAB_RECENTS_MAX);
        assert_eq!(result.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(), ["g/p", "a/b", "c/d"]);
        assert_eq!(result[0].last_opened_at, NOW);
    }

    #[test]
    fn treats_different_hosts_at_the_same_path_as_distinct_entries() {
        let existing = vec![entry("gitlab.example.com", "g/p", "2026-05-07")];
        let result = compute_next_gitlab_recents(&existing, "gitlab.com", "g/p", NOW, GITLAB_RECENTS_MAX);
        assert_eq!(result.len(), 2);
        assert_eq!((result[0].host.as_str(), result[0].path.as_str()), ("gitlab.com", "g/p"));
        assert_eq!((result[1].host.as_str(), result[1].path.as_str()), ("gitlab.example.com", "g/p"));
    }

    #[test]
    fn caps_the_list_at_gitlab_recents_max_entries() {
        let existing: Vec<GitLabRecentEntry> =
            (0..GITLAB_RECENTS_MAX).map(|i| entry("gitlab.com", &format!("g/p{i}"), &format!("2026-05-0{i}"))).collect();
        let result = compute_next_gitlab_recents(&existing, "gitlab.com", "g/new", NOW, GITLAB_RECENTS_MAX);
        assert_eq!(result.len(), GITLAB_RECENTS_MAX);
        assert_eq!(result[0].path, "g/new");
        let dropped = format!("g/p{}", GITLAB_RECENTS_MAX - 1);
        assert!(!result.iter().any(|r| r.path == dropped));
    }

    #[test]
    fn does_not_mutate_the_input_array() {
        let existing = vec![entry("gitlab.com", "a/b", "2026-05-07")];
        let snapshot = existing.clone();
        compute_next_gitlab_recents(&existing, "gitlab.com", "g/p", NOW, GITLAB_RECENTS_MAX);
        assert_eq!(existing, snapshot);
    }
}
