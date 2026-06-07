//! `git check-ignore` chunking + output parsing, ported from
//! `src/main/git/check-ignored-paths.ts`. The runner is bound to the worktree
//! cwd, so this takes only the relative paths.

use crate::runner::{GitError, GitRunner};
use std::collections::HashSet;

const CHECK_IGNORE_CHUNK_SIZE: usize = 100;

fn parse_check_ignore_output(stdout: &str) -> Vec<String> {
    stdout
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn run_check_ignore_chunk<R: GitRunner>(runner: &R, relative_paths: &[&str]) -> Result<Vec<String>, GitError> {
    let mut args: Vec<&str> = vec!["-c", "core.quotePath=false", "check-ignore", "--"];
    args.extend_from_slice(relative_paths);
    match runner.run(&args) {
        Ok(output) => Ok(parse_check_ignore_output(&output.stdout)),
        // git check-ignore exits 1 when nothing matched; the matched paths (if
        // any) are still on stdout.
        Err(error) if error.code == Some(1) => Ok(parse_check_ignore_output(&error.stdout)),
        Err(error) => Err(error),
    }
}

pub fn check_ignored_paths<R: GitRunner>(runner: &R, relative_paths: &[&str]) -> Result<Vec<String>, GitError> {
    let mut ignored: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for chunk in relative_paths.chunks(CHECK_IGNORE_CHUNK_SIZE) {
        for path in run_check_ignore_chunk(runner, chunk)? {
            if seen.insert(path.clone()) {
                ignored.push(path);
            }
        }
    }
    Ok(ignored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::GitOutput;
    use std::cell::RefCell;

    #[test]
    fn returns_ignored_paths_from_check_ignore_output() {
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let runner = |args: &[&str]| {
            calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            Ok(GitOutput { stdout: "dist/bundle.js\n.env\n".to_string(), stderr: String::new() })
        };
        let result = check_ignored_paths(&runner, &["dist/bundle.js", "src/index.ts", ".env"]).unwrap();
        assert_eq!(result, vec!["dist/bundle.js".to_string(), ".env".to_string()]);
        assert_eq!(
            calls.borrow()[0],
            vec![
                "-c",
                "core.quotePath=false",
                "check-ignore",
                "--",
                "dist/bundle.js",
                "src/index.ts",
                ".env",
            ]
        );
    }

    #[test]
    fn treats_exit_code_1_as_no_ignored_paths() {
        let runner = |_: &[&str]| {
            Err(GitError { code: Some(1), stdout: String::new(), stderr: String::new(), message: "no matches".to_string() })
        };
        assert_eq!(check_ignored_paths(&runner, &["src/index.ts"]).unwrap(), Vec::<String>::new());
    }
}
