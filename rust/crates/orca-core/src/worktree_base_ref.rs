//! Base-ref qualification for `git worktree add`, ported from
//! `src/shared/worktree-base-ref.ts`.
//!
//! `git worktree add` takes a revision, so a short name like `main` can collide
//! with a tag. This resolves the namespace Orca's base picker implies: prefer a
//! remote-tracking ref for remote-display names (`origin/main`), otherwise a
//! local branch. The original is async because `ref_exists` hits git; the pure
//! decision logic (candidate ordering, fully-qualified passthrough) lives here,
//! parameterised over a synchronous existence predicate so the IO boundary owns
//! the await.

pub fn resolve_worktree_add_base_ref<F>(base_ref: &str, ref_exists: F) -> String
where
    F: Fn(&str) -> bool,
{
    if base_ref.starts_with("refs/") {
        return base_ref.to_string();
    }

    let candidates: Vec<String> = if base_ref.contains('/') {
        vec![
            format!("refs/remotes/{base_ref}"),
            format!("refs/heads/{base_ref}"),
        ]
    } else {
        vec![format!("refs/heads/{base_ref}")]
    };

    for candidate in &candidates {
        if ref_exists(candidate) {
            return candidate.clone();
        }
    }

    base_ref.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn leaves_fully_qualified_refs_unchanged() {
        let called = RefCell::new(false);
        let result = resolve_worktree_add_base_ref("refs/heads/main", |_| {
            *called.borrow_mut() = true;
            true
        });
        assert_eq!(result, "refs/heads/main");
        assert!(!*called.borrow());
    }

    #[test]
    fn leaves_provider_review_refs_unchanged() {
        let called = RefCell::new(false);
        let mark = |_: &str| {
            *called.borrow_mut() = true;
            true
        };
        assert_eq!(
            resolve_worktree_add_base_ref("refs/pull/123/head", mark),
            "refs/pull/123/head"
        );
        assert_eq!(
            resolve_worktree_add_base_ref("refs/merge-requests/456/head", mark),
            "refs/merge-requests/456/head"
        );
        assert!(!*called.borrow());
    }

    #[test]
    fn qualifies_a_bare_local_branch_name() {
        let calls = RefCell::new(Vec::new());
        let result = resolve_worktree_add_base_ref("main", |ref_name| {
            calls.borrow_mut().push(ref_name.to_string());
            ref_name == "refs/heads/main"
        });
        assert_eq!(result, "refs/heads/main");
        assert_eq!(*calls.borrow(), vec!["refs/heads/main".to_string()]);
    }

    #[test]
    fn prefers_a_remote_tracking_ref_for_remote_display_names() {
        let calls = RefCell::new(Vec::new());
        let result = resolve_worktree_add_base_ref("origin/main", |ref_name| {
            calls.borrow_mut().push(ref_name.to_string());
            ref_name == "refs/remotes/origin/main"
        });
        assert_eq!(result, "refs/remotes/origin/main");
        assert_eq!(*calls.borrow(), vec!["refs/remotes/origin/main".to_string()]);
    }

    #[test]
    fn qualifies_slash_local_branch_when_no_matching_remote_ref_exists() {
        let calls = RefCell::new(Vec::new());
        let result = resolve_worktree_add_base_ref("release/main", |ref_name| {
            calls.borrow_mut().push(ref_name.to_string());
            ref_name == "refs/heads/release/main"
        });
        assert_eq!(result, "refs/heads/release/main");
        assert_eq!(
            *calls.borrow(),
            vec![
                "refs/remotes/release/main".to_string(),
                "refs/heads/release/main".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_unresolvable_revisions_untouched() {
        let result = resolve_worktree_add_base_ref("abc1234", |_| false);
        assert_eq!(result, "abc1234");
    }
}
