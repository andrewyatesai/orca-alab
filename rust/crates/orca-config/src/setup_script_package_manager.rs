//! Package-manager setup-script suggestion, ported from
//! `src/shared/setup-script-package-manager-suggestion.ts`.
//!
//! Inspects a project to suggest an install command: an explicit
//! `package.json#packageManager` wins; otherwise a single detected lockfile
//! family selects the command, and ambiguous (multi-family) or
//! `package.json`-less projects yield nothing. File reads / existence checks are
//! injected (the IO boundary), so this is pure and testable.

use serde_json::Value;

const PACKAGE_JSON_PATH: &str = "package.json";

struct Lockfile {
    path: &'static str,
    manager: &'static str,
    setup: &'static str,
}

const PACKAGE_MANAGER_LOCKFILES: [Lockfile; 6] = [
    Lockfile { path: "pnpm-lock.yaml", manager: "pnpm", setup: "pnpm install" },
    Lockfile { path: "bun.lock", manager: "bun", setup: "bun install" },
    Lockfile { path: "bun.lockb", manager: "bun", setup: "bun install" },
    Lockfile { path: "yarn.lock", manager: "yarn", setup: "yarn install" },
    Lockfile { path: "package-lock.json", manager: "npm", setup: "npm install" },
    Lockfile { path: "npm-shrinkwrap.json", manager: "npm", setup: "npm install" },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupScriptImportCandidate {
    pub provider: String,
    pub label: String,
    pub files: Vec<String>,
    pub setup: String,
    pub unsupported_fields: Vec<String>,
}

/// `read_file(path) -> Some(contents)` / `None`; `file_exists` (optional) checks
/// existence without reading — falling back to a non-`None` read.
pub fn inspect_package_manager_setup_candidate(
    read_file: &dyn Fn(&str) -> Option<String>,
    file_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<SetupScriptImportCandidate> {
    let package_json = parse_package_json(read_file(PACKAGE_JSON_PATH).as_deref())?;

    if let Some(manager) = package_manager_name(package_json.get("packageManager")) {
        return Some(candidate(vec![PACKAGE_JSON_PATH.to_string()], manager.setup()));
    }

    let existing: Vec<&Lockfile> = PACKAGE_MANAGER_LOCKFILES
        .iter()
        .filter(|entry| match file_exists {
            Some(check) => check(entry.path),
            None => read_file(entry.path).is_some(),
        })
        .collect();
    let families: std::collections::HashSet<&str> = existing.iter().map(|entry| entry.manager).collect();
    if families.len() > 1 {
        return None;
    }
    let selected = if families.len() == 1 { existing.first().copied() } else { None };
    let setup = selected.map_or("npm install", |entry| entry.setup);
    let files = vec![selected.map_or(PACKAGE_JSON_PATH, |entry| entry.path).to_string()];
    Some(candidate(files, setup))
}

fn candidate(files: Vec<String>, setup: &str) -> SetupScriptImportCandidate {
    SetupScriptImportCandidate {
        provider: "package-manager".to_string(),
        label: "package manager".to_string(),
        files,
        setup: setup.to_string(),
        unsupported_fields: Vec::new(),
    }
}

#[derive(Clone, Copy)]
enum PackageManager {
    Pnpm,
    Bun,
    Yarn,
    Npm,
}

impl PackageManager {
    fn setup(self) -> &'static str {
        match self {
            PackageManager::Pnpm => "pnpm install",
            PackageManager::Bun => "bun install",
            PackageManager::Yarn => "yarn install",
            PackageManager::Npm => "npm install",
        }
    }
}

fn parse_package_json(content: Option<&str>) -> Option<Value> {
    let content = content.filter(|text| !text.is_empty())?;
    let parsed: Value = serde_json::from_str(content).ok()?;
    // Must be a JSON object (not an array/primitive/null).
    parsed.is_object().then_some(parsed)
}

fn package_manager_name(value: Option<&Value>) -> Option<PackageManager> {
    let declared = value?.as_str()?.trim().to_lowercase();
    if declared.starts_with("pnpm@") {
        Some(PackageManager::Pnpm)
    } else if declared.starts_with("bun@") {
        Some(PackageManager::Bun)
    } else if declared.starts_with("yarn@") {
        Some(PackageManager::Yarn)
    } else if declared.starts_with("npm@") {
        Some(PackageManager::Npm)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn reader<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |path| files.iter().find(|(name, _)| *name == path).map(|(_, content)| content.to_string())
    }

    fn pm_candidate(files: &[&str], setup: &str) -> SetupScriptImportCandidate {
        SetupScriptImportCandidate {
            provider: "package-manager".to_string(),
            label: "package manager".to_string(),
            files: files.iter().map(|f| f.to_string()).collect(),
            setup: setup.to_string(),
            unsupported_fields: Vec::new(),
        }
    }

    #[test]
    fn suggests_setup_commands_from_package_manager_lockfiles() {
        let read = reader(&[("package.json", r#"{"scripts":{"dev":"vite"}}"#), ("pnpm-lock.yaml", "lockfileVersion: 9.0")]);
        assert_eq!(
            inspect_package_manager_setup_candidate(&read, None),
            Some(pm_candidate(&["pnpm-lock.yaml"], "pnpm install"))
        );
    }

    #[test]
    fn uses_package_manager_when_no_lockfile_is_present() {
        let read = reader(&[("package.json", r#"{"packageManager":"bun@1.2.0"}"#)]);
        assert_eq!(
            inspect_package_manager_setup_candidate(&read, None),
            Some(pm_candidate(&["package.json"], "bun install"))
        );
    }

    #[test]
    fn uses_explicit_package_manager_over_conflicting_lockfiles() {
        let read = reader(&[("package.json", r#"{"packageManager":"pnpm@9.15.0"}"#), ("package-lock.json", "{}")]);
        assert_eq!(
            inspect_package_manager_setup_candidate(&read, None),
            Some(pm_candidate(&["package.json"], "pnpm install"))
        );
    }

    #[test]
    fn does_not_check_lockfiles_when_package_manager_declares_the_setup_command() {
        let read = reader(&[("package.json", r#"{"packageManager":"pnpm@9.15.0"}"#)]);
        let calls: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let file_exists = |path: &str| {
            calls.borrow_mut().push(path.to_string());
            true
        };
        let result = inspect_package_manager_setup_candidate(&read, Some(&file_exists));
        assert!(calls.borrow().is_empty());
        assert_eq!(result, Some(pm_candidate(&["package.json"], "pnpm install")));
    }

    #[test]
    fn uses_file_existence_checks_instead_of_reading_lockfile_contents() {
        let read_calls: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let exists_calls: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let read = |path: &str| {
            read_calls.borrow_mut().push(path.to_string());
            (path == "package.json").then(|| r#"{"scripts":{"dev":"vite"}}"#.to_string())
        };
        let file_exists = |path: &str| {
            exists_calls.borrow_mut().push(path.to_string());
            path == "pnpm-lock.yaml"
        };
        let result = inspect_package_manager_setup_candidate(&read, Some(&file_exists));
        assert!(!read_calls.borrow().iter().any(|p| p == "pnpm-lock.yaml"));
        assert!(exists_calls.borrow().iter().any(|p| p == "pnpm-lock.yaml"));
        assert_eq!(result, Some(pm_candidate(&["pnpm-lock.yaml"], "pnpm install")));
    }

    #[test]
    fn does_not_suggest_without_a_valid_package_json() {
        let read = reader(&[("package.json", "{"), ("pnpm-lock.yaml", "lockfileVersion: 9.0")]);
        assert_eq!(inspect_package_manager_setup_candidate(&read, None), None);
    }

    #[test]
    fn does_not_guess_between_multiple_lockfiles_without_package_manager() {
        let read = reader(&[
            ("package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("pnpm-lock.yaml", "lockfileVersion: 9.0"),
            ("package-lock.json", "{}"),
        ]);
        assert_eq!(inspect_package_manager_setup_candidate(&read, None), None);
    }

    #[test]
    fn allows_multiple_lockfiles_for_the_same_package_manager_family() {
        let read = reader(&[
            ("package.json", r#"{"scripts":{"dev":"vite"}}"#),
            ("package-lock.json", "{}"),
            ("npm-shrinkwrap.json", "{}"),
        ]);
        assert_eq!(
            inspect_package_manager_setup_candidate(&read, None),
            Some(pm_candidate(&["package-lock.json"], "npm install"))
        );
    }
}
