//! Quick Open (Cmd/Ctrl+P) file-listing filter policy, ported from
//! `src/shared/quick-open-filter.ts`.
//!
//! Pure policy shared by the local main process and the SSH relay: hidden-dir
//! blocklist, nested-worktree exclude prefixes, rg/git arg construction, and rg
//! stdout normalisation. No IO; callers own process execution and WSL
//! translation. Path comparisons are flavour-parameterised (POSIX vs Windows)
//! so a macOS app talking to a Linux/Windows relay computes them correctly.

/// Tool-generated cache/state dirs that are never hand-edited. A blocklist (not
/// an allowlist) keeps novel dotfiles discoverable. Order matches the TS Set's
/// insertion order (only relevant for glob-arg ordering, not membership).
pub const HIDDEN_DIR_BLOCKLIST: &[&str] = &[
    ".git",
    ".next",
    ".nuxt",
    ".cache",
    ".stably",
    ".vscode",
    ".idea",
    ".yarn",
    ".pnpm-store",
    ".terraform",
    ".docker",
    ".husky",
    ".npm",
    ".npm-global",
    ".gvfs",
];

/// `.local` itself can hold user files; only the generated runtime subtree is blocked.
const HIDDEN_PATH_BLOCKLIST: &[&str] = &[".local/share"];

/// Not a dotfile dir, but must still be pruned from every traversal.
const NON_DOTTED_PRUNE: &str = "node_modules";

pub fn hidden_dir_blocklist_contains(name: &str) -> bool {
    HIDDEN_DIR_BLOCKLIST.contains(&name)
}

fn contains_blocked_rel_path(path: &str, blocked: &str) -> bool {
    path == blocked
        || path.starts_with(&format!("{blocked}/"))
        || path.ends_with(&format!("/{blocked}"))
        || path.contains(&format!("/{blocked}/"))
}

/// True when `path` (a `/`-separated, root-relative path) does not traverse any
/// blocklisted directory segment.
pub fn should_include_quick_open_path(path: &str) -> bool {
    for blocked in HIDDEN_PATH_BLOCKLIST {
        if contains_blocked_rel_path(path, blocked) {
            return false;
        }
    }
    for segment in path.split('/') {
        if segment == NON_DOTTED_PRUNE || hidden_dir_blocklist_contains(segment) {
            return false;
        }
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathFlavor {
    Posix,
    Windows,
}

fn path_flavor(root_path: &str) -> PathFlavor {
    let b = root_path.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    if drive || root_path.starts_with("\\\\") {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    }
}

fn is_parent_relative_path(rel_path: &str) -> bool {
    // `..name` is a valid child path; only `..` and `../...` escape.
    rel_path == ".." || rel_path.starts_with("../")
}

fn normalize_segments(rest: &str) -> Vec<&str> {
    let mut segs: Vec<&str> = Vec::new();
    for s in rest.split('/') {
        if s.is_empty() || s == "." {
            continue;
        }
        if s == ".." {
            segs.pop();
            continue;
        }
        segs.push(s);
    }
    segs
}

fn posix_relative(from: &str, to: &str) -> String {
    let f = normalize_segments(from);
    let t = normalize_segments(to);
    let mut i = 0;
    while i < f.len() && i < t.len() && f[i] == t[i] {
        i += 1;
    }
    let mut parts: Vec<&str> = vec![".."; f.len() - i];
    parts.extend_from_slice(&t[i..]);
    parts.join("/")
}

fn split_drive(p: &str) -> (String, String) {
    let fp = p.replace('\\', "/");
    let b = fp.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        (fp[0..2].to_lowercase(), fp[2..].to_string())
    } else {
        (String::new(), fp)
    }
}

fn win32_relative(from: &str, to: &str) -> String {
    let (drive_from, rest_from) = split_drive(from);
    let (drive_to, rest_to) = split_drive(to);
    if drive_from != drive_to {
        // Different drives — Node returns the (resolved) `to` path.
        return to.replace('\\', "/");
    }
    let f = normalize_segments(&rest_from);
    let t = normalize_segments(&rest_to);
    let mut i = 0;
    while i < f.len() && i < t.len() && f[i].eq_ignore_ascii_case(t[i]) {
        i += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); f.len() - i];
    parts.extend(t[i..].iter().map(|s| s.to_string()));
    parts.join("/")
}

fn flavor_relative(flavor: PathFlavor, from: &str, to: &str) -> String {
    match flavor {
        PathFlavor::Posix => posix_relative(from, to),
        PathFlavor::Windows => win32_relative(from, to),
    }
}

/// Normalise renderer-supplied absolute exclude paths into `/`-separated,
/// root-relative prefixes. `None` ≙ a non-array input; element `None` ≙ a
/// non-string entry. Malformed, outside-root, and root-equal values are dropped.
pub fn build_exclude_path_prefixes(root_path: &str, exclude_paths: Option<&[Option<&str>]>) -> Vec<String> {
    let Some(paths) = exclude_paths else {
        return Vec::new();
    };
    let flavor = path_flavor(root_path);
    let trimmed_root = root_path.trim_end_matches(['\\', '/']);
    let normalized_root = format!("{}/", trimmed_root.replace('\\', "/"));
    let root_equal = &normalized_root[..normalized_root.len() - 1];

    let mut out: Vec<String> = Vec::new();
    for raw_opt in paths {
        let Some(raw) = raw_opt else { continue };
        if raw.is_empty() {
            continue;
        }
        let raw_fwd = raw.replace('\\', "/");
        if raw_fwd == root_equal {
            continue;
        }
        let rel = if raw_fwd.starts_with(&normalized_root) {
            raw_fwd[normalized_root.len()..].to_string()
        } else {
            flavor_relative(flavor, trimmed_root, raw).replace('\\', "/")
        };
        if rel.is_empty() || is_parent_relative_path(&rel) || rel.starts_with('/') {
            continue;
        }
        let rel = rel.trim_end_matches('/');
        if rel.is_empty() {
            continue;
        }
        out.push(rel.to_string());
    }
    out
}

/// Segment-boundary exclude check: `rel_path == prefix` or starts with `prefix/`.
pub fn should_exclude_quick_open_rel_path(rel_path: &str, exclude_path_prefixes: &[&str]) -> bool {
    for prefix in exclude_path_prefixes {
        if rel_path == *prefix {
            return true;
        }
        if rel_path.len() > prefix.len() && rel_path.starts_with(&format!("{prefix}/")) {
            return true;
        }
    }
    false
}

const GLOB_META: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];

fn escape_glob(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        if GLOB_META.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_glob_path(rel_path: &str) -> String {
    rel_path.split('/').map(escape_glob).collect::<Vec<_>>().join("/")
}

/// Hidden-dir traversal-pruning glob args for rg (directory-match form `!**/name`).
pub fn build_hidden_dir_exclude_globs() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("--glob".to_string());
    out.push(format!("!**/{}", escape_glob(NON_DOTTED_PRUNE)));
    for name in HIDDEN_DIR_BLOCKLIST {
        out.push("--glob".to_string());
        out.push(format!("!**/{}", escape_glob(name)));
    }
    for blocked in HIDDEN_PATH_BLOCKLIST {
        out.push("--glob".to_string());
        out.push(format!("!**/{}", escape_glob_path(blocked)));
    }
    out
}

pub struct RgArgsOptions<'a> {
    pub search_root: &'a str,
    pub exclude_path_prefixes: &'a [&'a str],
    pub force_slash_separator: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgArgs {
    pub primary: Vec<String>,
    pub ignored_pass: Vec<String>,
}

pub fn build_rg_args_for_quick_open(opts: &RgArgsOptions<'_>) -> RgArgs {
    let sep_args: Vec<String> = if opts.force_slash_separator {
        vec!["--path-separator".to_string(), "/".to_string()]
    } else {
        Vec::new()
    };
    let hidden_dir_globs = build_hidden_dir_exclude_globs();
    let mut exclude_globs: Vec<String> = Vec::new();
    for prefix in opts.exclude_path_prefixes {
        exclude_globs.push("--glob".to_string());
        exclude_globs.push(format!("!{}", escape_glob_path(prefix)));
        exclude_globs.push("--glob".to_string());
        exclude_globs.push(format!("!{}/**", escape_glob_path(prefix)));
    }

    let build = |extra: &[&str]| {
        let mut args: Vec<String> = vec!["--files".to_string(), "--hidden".to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        args.extend(sep_args.iter().cloned());
        args.extend(hidden_dir_globs.iter().cloned());
        args.extend(exclude_globs.iter().cloned());
        args.push(opts.search_root.to_string());
        args
    };
    RgArgs {
        primary: build(&[]),
        ignored_pass: build(&["--no-ignore-vcs"]),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RgOutputMode {
    Absolute { root_path: String },
    CwdRelative,
}

/// Convert one rg `--files` stdout line into a root-relative, `/`-separated
/// path, or `None` if it escapes the root or cannot be normalised.
pub fn normalize_quick_open_rg_line(raw_line: &str, output_mode: &RgOutputMode) -> Option<String> {
    let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
    if line.is_empty() {
        return None;
    }
    let normalized = line.replace('\\', "/");
    match output_mode {
        RgOutputMode::CwdRelative => {
            let rel = if let Some(stripped) = normalized.strip_prefix("./") {
                stripped
            } else if normalized == "." {
                return None;
            } else {
                &normalized
            };
            if rel.is_empty() || rel.starts_with('/') || is_parent_relative_path(rel) {
                return None;
            }
            Some(rel.to_string())
        }
        RgOutputMode::Absolute { root_path } => {
            // Replace only backslashes (do NOT collapse repeated slashes — that
            // would break Windows UNC roots).
            let normalized_root =
                format!("{}/", root_path.replace('\\', "/").trim_end_matches('/'));
            let rel = normalized.strip_prefix(&normalized_root)?;
            if rel.is_empty() || is_parent_relative_path(rel) || rel.starts_with('/') {
                return None;
            }
            Some(rel.to_string())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitLsFilesArgs {
    pub primary: Vec<String>,
    pub ignored_pass: Vec<String>,
}

pub fn build_git_ls_files_args_for_quick_open(exclude_path_prefixes: &[&str]) -> GitLsFilesArgs {
    let mut trailing: Vec<String> = Vec::new();
    if !exclude_path_prefixes.is_empty() {
        trailing.push("--".to_string());
        trailing.push(".".to_string());
        for prefix in exclude_path_prefixes {
            trailing.push(format!(":(exclude,glob){}", escape_glob_path(prefix)));
            trailing.push(format!(":(exclude,glob){}/**", escape_glob_path(prefix)));
        }
    }
    let with = |head: &[&str]| {
        let mut args: Vec<String> = head.iter().map(|s| s.to_string()).collect();
        args.extend(trailing.iter().cloned());
        args
    };
    GitLsFilesArgs {
        primary: with(&["-z", "--cached", "--others", "--exclude-standard"]),
        ignored_pass: with(&["-z", "--others", "--ignored", "--exclude-standard"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(items: &[&'static str]) -> Vec<Option<&'static str>> {
        items.iter().map(|s| Some(*s)).collect()
    }

    #[test]
    fn includes_normal_source_paths() {
        assert!(should_include_quick_open_path("src/index.ts"));
        assert!(should_include_quick_open_path(".github/workflows/ci.yml"));
        assert!(should_include_quick_open_path(".env"));
    }

    #[test]
    fn excludes_node_modules_and_blocklisted_dirs_at_any_depth() {
        assert!(!should_include_quick_open_path("node_modules/a/b.js"));
        assert!(!should_include_quick_open_path("packages/x/node_modules/a.js"));
        assert!(!should_include_quick_open_path(".git/config"));
        assert!(!should_include_quick_open_path("foo/.cache/bar"));
    }

    #[test]
    fn hides_home_dir_cache_and_runtime_state() {
        assert!(!should_include_quick_open_path(".npm/pkg/index.js"));
        assert!(!should_include_quick_open_path(".npm-global/bin/foo"));
        assert!(!should_include_quick_open_path(".gvfs/mount/file"));
        assert!(!should_include_quick_open_path(".local/share/app/state.db"));
        assert!(!should_include_quick_open_path("nested/.local/share/app/state.db"));
        assert!(should_include_quick_open_path(".local/bin/tool"));
    }

    #[test]
    fn does_not_blocklist_user_authored_dirs() {
        for name in [".config", ".ssh", ".github", ".devcontainer", ".local"] {
            assert!(!hidden_dir_blocklist_contains(name), "{name}");
        }
    }

    #[test]
    fn exclude_prefixes_root_relative_posix() {
        assert_eq!(
            build_exclude_path_prefixes(
                "/home/u/repo",
                Some(&some(&["/home/u/repo/packages/app", "/home/u/repo/worktrees/b"]))
            ),
            vec!["packages/app".to_string(), "worktrees/b".to_string()]
        );
    }

    #[test]
    fn exclude_prefixes_ignore_malformed_input() {
        assert_eq!(build_exclude_path_prefixes("/home/u/repo", None), Vec::<String>::new());
        assert_eq!(
            build_exclude_path_prefixes("/home/u/repo", Some(&[None, None, Some(""), Some("/outside")])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn exclude_prefixes_ignore_root_equal_and_outside() {
        assert_eq!(
            build_exclude_path_prefixes("/home/u/repo", Some(&some(&["/home/u/repo"]))),
            Vec::<String>::new()
        );
        assert_eq!(
            build_exclude_path_prefixes("/home/u/repo", Some(&some(&["/home/u/other"]))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn exclude_prefixes_keep_dotdot_children_reject_escapes() {
        assert_eq!(
            build_exclude_path_prefixes(
                "/home/u/repo",
                Some(&some(&[
                    "/home/u/repo/..env",
                    "/home/u/repo/..workspace/app",
                    "/home/u/repo/../outside",
                ]))
            ),
            vec!["..env".to_string(), "..workspace/app".to_string()]
        );
    }

    #[test]
    fn exclude_prefixes_handle_windows_roots() {
        assert_eq!(
            build_exclude_path_prefixes("C:\\repo", Some(&some(&["C:\\repo\\packages\\app"]))),
            vec!["packages/app".to_string()]
        );
    }

    #[test]
    fn exclude_prefixes_strip_trailing_slashes() {
        assert_eq!(
            build_exclude_path_prefixes("/r", Some(&some(&["/r/a/", "/r/b///"]))),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn exclude_rel_path_matches_exact_and_boundary_only() {
        assert!(should_exclude_quick_open_rel_path("packages/app", &["packages/app"]));
        assert!(should_exclude_quick_open_rel_path("packages/app/x.ts", &["packages/app"]));
        assert!(!should_exclude_quick_open_rel_path("packages/app2/x.ts", &["packages/app"]));
        assert!(!should_exclude_quick_open_rel_path("packages/application", &["packages/app"]));
    }

    #[test]
    fn hidden_dir_globs_use_directory_match_form() {
        let globs = build_hidden_dir_exclude_globs();
        let has = |g: &str| globs.iter().any(|x| x == g);
        assert!(has("!**/node_modules"));
        assert!(has("!**/.git"));
        assert!(has("!**/.cache"));
        assert!(has("!**/.local/share"));
        assert!(!has("!**/node_modules/**"));
    }

    #[test]
    fn rg_args_primary_and_ignored() {
        let RgArgs { primary, ignored_pass } = build_rg_args_for_quick_open(&RgArgsOptions {
            search_root: "/root",
            exclude_path_prefixes: &[],
            force_slash_separator: false,
        });
        assert!(primary.iter().any(|x| x == "--files"));
        assert!(primary.iter().any(|x| x == "--hidden"));
        assert!(primary.iter().any(|x| x == "!**/node_modules"));
        assert!(!primary.iter().any(|x| x == "--follow"));
        assert!(ignored_pass.iter().any(|x| x == "--no-ignore-vcs"));
        assert!(!ignored_pass.iter().any(|x| x == "--follow"));
    }

    #[test]
    fn rg_args_force_slash_separator() {
        let RgArgs { primary, .. } = build_rg_args_for_quick_open(&RgArgsOptions {
            search_root: "/r",
            exclude_path_prefixes: &[],
            force_slash_separator: true,
        });
        let idx = primary.iter().position(|x| x == "--path-separator").expect("flag");
        assert_eq!(primary[idx + 1], "/");
    }

    #[test]
    fn rg_args_escape_exclude_prefixes_as_directory_globs() {
        let RgArgs { primary, .. } = build_rg_args_for_quick_open(&RgArgsOptions {
            search_root: "/r",
            exclude_path_prefixes: &["packages/app", "feature[1]"],
            force_slash_separator: false,
        });
        assert!(primary.iter().any(|x| x == "!packages/app"));
        assert!(primary.iter().any(|x| x == "!packages/app/**"));
        assert!(primary.iter().any(|x| x == "!feature\\[1\\]"));
    }

    #[test]
    fn normalize_rg_line_absolute_and_windows() {
        assert_eq!(
            normalize_quick_open_rg_line("/root/src/a.ts", &RgOutputMode::Absolute { root_path: "/root".into() }),
            Some("src/a.ts".to_string())
        );
        assert_eq!(
            normalize_quick_open_rg_line("C:\\repo\\src\\a.ts", &RgOutputMode::Absolute { root_path: "C:\\repo".into() }),
            Some("src/a.ts".to_string())
        );
        assert_eq!(
            normalize_quick_open_rg_line(
                "\\\\server\\share\\repo\\src\\a.ts",
                &RgOutputMode::Absolute { root_path: "\\\\server\\share\\repo".into() }
            ),
            Some("src/a.ts".to_string())
        );
    }

    #[test]
    fn normalize_rg_line_cwd_relative() {
        assert_eq!(
            normalize_quick_open_rg_line("./src/a.ts", &RgOutputMode::CwdRelative),
            Some("src/a.ts".to_string())
        );
        assert_eq!(
            normalize_quick_open_rg_line("./..fixtures/a.ts", &RgOutputMode::CwdRelative),
            Some("..fixtures/a.ts".to_string())
        );
        assert_eq!(
            normalize_quick_open_rg_line("..env", &RgOutputMode::CwdRelative),
            Some("..env".to_string())
        );
        assert_eq!(normalize_quick_open_rg_line("../outside.ts", &RgOutputMode::CwdRelative), None);
        assert_eq!(normalize_quick_open_rg_line("..", &RgOutputMode::CwdRelative), None);
        assert_eq!(normalize_quick_open_rg_line("./../outside/a.ts", &RgOutputMode::CwdRelative), None);
    }

    #[test]
    fn normalize_rg_line_strips_crlf_and_rejects_outside() {
        assert_eq!(
            normalize_quick_open_rg_line("/root/a.ts\r", &RgOutputMode::Absolute { root_path: "/root".into() }),
            Some("a.ts".to_string())
        );
        assert_eq!(
            normalize_quick_open_rg_line("/other/a.ts", &RgOutputMode::Absolute { root_path: "/root".into() }),
            None
        );
        assert_eq!(normalize_quick_open_rg_line("", &RgOutputMode::CwdRelative), None);
        assert_eq!(normalize_quick_open_rg_line(".", &RgOutputMode::CwdRelative), None);
    }

    #[test]
    fn git_ls_files_args() {
        let GitLsFilesArgs { primary, ignored_pass } = build_git_ls_files_args_for_quick_open(&[]);
        assert_eq!(primary, vec!["-z", "--cached", "--others", "--exclude-standard"]);
        assert_eq!(ignored_pass, vec!["-z", "--others", "--ignored", "--exclude-standard"]);
    }
}
