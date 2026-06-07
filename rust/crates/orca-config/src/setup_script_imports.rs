//! Setup-script import detection, ported from `src/shared/setup-script-imports.ts`.
//!
//! Inspects a project for setup/teardown commands declared by other agent
//! tools — Superset (`.superset/config.json` + `config.local.json`), Conductor
//! (`conductor.json`), Codex environment files, and cmux — plus a
//! package-manager fallback, normalizing each into a [`SetupScriptImportCandidate`].
//! Codex parsing and the package-manager suggestion are delegated to their own
//! modules. File reads / existence checks are injected (the IO boundary), so
//! this stays pure and testable.

use crate::setup_script_import_codex_environment::inspect_codex_environment_config;
use crate::setup_script_package_manager::{
    inspect_package_manager_setup_candidate, SetupScriptImportCandidate as PackageManagerCandidate,
};
use serde_json::{Map, Value};

const SUPERSET_CONFIG_PATH: &str = ".superset/config.json";
const SUPERSET_LOCAL_CONFIG_PATH: &str = ".superset/config.local.json";
const CONDUCTOR_CONFIG_PATH: &str = "conductor.json";
const CMUX_CONFIG_PATHS: [&str; 2] = [".cmux/cmux.json", "cmux.json"];

/// A normalized setup-script import. `unsupported_fields` is always present
/// (possibly empty); `archive` is `None` when no teardown command exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupScriptImportCandidate {
    pub provider: String,
    pub label: String,
    pub files: Vec<String>,
    pub setup: String,
    pub archive: Option<String>,
    pub unsupported_fields: Vec<String>,
}

/// `read_file(path) -> Some(contents)` / `None`; `file_exists` (optional) is
/// forwarded to the package-manager fallback. Every returned candidate carries
/// a non-empty `setup`.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<SetupScriptImportCandidate>|
    out.iter().all(|candidate| !candidate.setup.is_empty())))]
pub fn inspect_setup_script_import_candidates(
    read_file: &dyn Fn(&str) -> Option<String>,
    file_exists: Option<&dyn Fn(&str) -> bool>,
) -> Vec<SetupScriptImportCandidate> {
    let mut candidates: Vec<SetupScriptImportCandidate> = Vec::new();
    if let Some(candidate) = inspect_superset_config(read_file) {
        candidates.push(candidate);
    }
    if let Some(candidate) = inspect_conductor_config(read_file) {
        candidates.push(candidate);
    }
    if let Some(candidate) = inspect_codex_environment_config(read_file) {
        candidates.push(candidate);
    }
    if let Some(candidate) = inspect_cmux_config(read_file) {
        candidates.push(candidate);
    }
    if let Some(candidate) = inspect_package_manager_setup_candidate(read_file, file_exists) {
        candidates.push(from_package_manager(candidate));
    }
    candidates
}

/// The package-manager port omits `archive` and never reports it; widen its
/// candidate into the full shape.
fn from_package_manager(candidate: PackageManagerCandidate) -> SetupScriptImportCandidate {
    SetupScriptImportCandidate {
        provider: candidate.provider,
        label: candidate.label,
        files: candidate.files,
        setup: candidate.setup,
        archive: None,
        unsupported_fields: candidate.unsupported_fields,
    }
}

fn inspect_superset_config(
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Option<SetupScriptImportCandidate> {
    let config = parse_json_object(read_file(SUPERSET_CONFIG_PATH))?;

    let local_config = parse_json_object(read_file(SUPERSET_LOCAL_CONFIG_PATH));
    let mut unsupported_fields = collect_unsupported_fields(&config, &["run", "cwd"]);
    let files = if local_config.is_some() {
        vec![SUPERSET_CONFIG_PATH.to_string(), SUPERSET_LOCAL_CONFIG_PATH.to_string()]
    } else {
        vec![SUPERSET_CONFIG_PATH.to_string()]
    };
    if let Some(local) = &local_config {
        for field in collect_unsupported_fields(local, &["run", "cwd"]) {
            unsupported_fields.push(format!("config.local.{field}"));
        }
    }

    let setup = resolve_superset_script_value(
        config.get("setup"),
        local_config.as_ref().and_then(|local| local.get("setup")),
        "setup",
        &mut unsupported_fields,
    );
    if setup.is_empty() {
        return None;
    }

    collect_unsupported_script_object_fields(config.get("setup"), "setup", &mut unsupported_fields);
    collect_unsupported_script_object_fields(config.get("teardown"), "teardown", &mut unsupported_fields);

    let archive = resolve_superset_script_value(
        config.get("teardown"),
        local_config.as_ref().and_then(|local| local.get("teardown")),
        "teardown",
        &mut unsupported_fields,
    );

    Some(SetupScriptImportCandidate {
        provider: "superset".to_string(),
        label: "Superset".to_string(),
        files,
        setup,
        archive: (!archive.is_empty()).then_some(archive),
        unsupported_fields,
    })
}

fn inspect_conductor_config(
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Option<SetupScriptImportCandidate> {
    let config = parse_json_object(read_file(CONDUCTOR_CONFIG_PATH))?;
    let scripts = config.get("scripts").and_then(Value::as_object)?;

    let setup = normalize_command_value(scripts.get("setup"));
    if setup.is_empty() {
        return None;
    }

    let mut unsupported_fields =
        collect_unsupported_fields(&config, &["enterpriseDataPrivacy", "runScriptMode"]);
    for field in ["run", "teardown"] {
        if !normalize_command_value(scripts.get(field)).is_empty() {
            unsupported_fields.push(format!("scripts.{field}"));
        }
    }

    let archive = normalize_command_value(scripts.get("archive"));

    Some(SetupScriptImportCandidate {
        provider: "conductor".to_string(),
        label: "Conductor".to_string(),
        files: vec![CONDUCTOR_CONFIG_PATH.to_string()],
        setup,
        archive: (!archive.is_empty()).then_some(archive),
        unsupported_fields,
    })
}

fn inspect_cmux_config(
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Option<SetupScriptImportCandidate> {
    for config_path in CMUX_CONFIG_PATHS {
        if let Some(config) = parse_json_object(read_file(config_path)) {
            if let Some(candidate) = build_cmux_setup_candidate(config_path, &config) {
                return Some(candidate);
            }
        }
    }
    None
}

fn parse_json_object(content: Option<String>) -> Option<Map<String, Value>> {
    let content = content.filter(|text| !text.is_empty())?;
    let value: Value = serde_json::from_str(&content).ok()?;
    value.as_object().cloned()
}

fn normalize_command_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::trim).unwrap_or(""))
            .filter(|command| !command.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn resolve_superset_script_value(
    base_value: Option<&Value>,
    local_value: Option<&Value>,
    key: &str,
    unsupported_fields: &mut Vec<String>,
) -> String {
    let base_command = normalize_command_value(base_value);
    // `localValue === undefined` is an absent key (no local file or no override).
    let Some(local) = local_value else {
        return base_command;
    };
    if matches!(local, Value::String(_) | Value::Array(_)) {
        return normalize_command_value(Some(local));
    }
    let Some(local_record) = local.as_object() else {
        unsupported_fields.push(format!("config.local.{key}"));
        return base_command;
    };

    for field in local_record.keys() {
        if field != "before" && field != "after" {
            unsupported_fields.push(format!("config.local.{key}.{field}"));
        }
    }

    let before_command = normalize_command_value(local_record.get("before"));
    let after_command = normalize_command_value(local_record.get("after"));
    [before_command, base_command, after_command]
        .into_iter()
        .filter(|command| !command.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_cmux_setup_candidate(
    config_path: &str,
    config: &Map<String, Value>,
) -> Option<SetupScriptImportCandidate> {
    // A missing / non-array `commands` yields no iterations, hence no candidate.
    let commands = config.get("commands").and_then(Value::as_array)?;
    for (index, raw) in commands.iter().enumerate() {
        let Some(command) = raw.as_object() else {
            continue;
        };
        if !is_cmux_setup_command(command) {
            continue;
        }
        let setup = normalize_command_value(command.get("command"));
        if setup.is_empty() {
            continue;
        }
        return Some(SetupScriptImportCandidate {
            provider: "cmux".to_string(),
            label: "cmux".to_string(),
            files: vec![config_path.to_string()],
            setup,
            archive: None,
            unsupported_fields: collect_unsupported_cmux_command_fields(command, index),
        });
    }
    None
}

fn is_cmux_setup_command(command: &Map<String, Value>) -> bool {
    let command_text = match command.get("command").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => text,
        _ => return false,
    };

    let name = normalize_match_text(command.get("name"));
    let title = normalize_match_text(command.get("title"));
    let labels: Vec<String> = [name, title].into_iter().filter(|label| !label.is_empty()).collect();
    const SETUP_LABELS: [&str; 4] = ["setup", "project setup", "workspace setup", "repository setup"];
    if labels.iter().any(|label| SETUP_LABELS.contains(&label.as_str())) {
        return true;
    }

    let keywords: Vec<String> = get_string_array(command.get("keywords"))
        .into_iter()
        .map(normalize_match_str)
        .collect();
    const SETUP_KEYWORDS: [&str; 4] = ["setup", "init", "initialize", "install"];
    let has_setup_keyword = keywords.iter().any(|keyword| SETUP_KEYWORDS.contains(&keyword.as_str()));
    if !has_setup_keyword {
        return false;
    }

    let normalized_command = normalize_match_str(command_text);
    labels.iter().any(|label| label.contains("setup")) || contains_setup_word(&normalized_command)
}

fn normalize_match_text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).map(normalize_match_str).unwrap_or_default()
}

/// `value.trim().toLowerCase().replace(/\s+/g, ' ')`.
fn normalize_match_str(value: &str) -> String {
    collapse_whitespace(&value.trim().to_lowercase())
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_whitespace = false;
    for c in value.chars() {
        if c.is_whitespace() {
            if !in_whitespace {
                out.push(' ');
                in_whitespace = true;
            }
        } else {
            out.push(c);
            in_whitespace = false;
        }
    }
    out
}

/// `/\bsetup\b/` — "setup" bounded by non-word (`[A-Za-z0-9_]`) chars.
fn contains_setup_word(text: &str) -> bool {
    for (start, matched) in text.match_indices("setup") {
        let end = start + matched.len();
        let before_word = text[..start].chars().next_back().is_some_and(is_word_char);
        let after_word = text[end..].chars().next().is_some_and(is_word_char);
        if !before_word && !after_word {
            return true;
        }
    }
    false
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn get_string_array(value: Option<&Value>) -> Vec<&str> {
    match value.and_then(Value::as_array) {
        Some(items) => items.iter().filter_map(Value::as_str).collect(),
        None => Vec::new(),
    }
}

fn collect_unsupported_cmux_command_fields(
    command: &Map<String, Value>,
    command_index: usize,
) -> Vec<String> {
    const SUPPORTED_FIELDS: [&str; 5] = ["name", "title", "description", "keywords", "command"];
    command
        .keys()
        .filter(|field| !SUPPORTED_FIELDS.contains(&field.as_str()))
        .map(|field| format!("commands.{command_index}.{field}"))
        .collect()
}

fn collect_unsupported_fields(source: &Map<String, Value>, field_names: &[&str]) -> Vec<String> {
    field_names
        .iter()
        .filter(|field| source.contains_key(**field))
        .map(|field| field.to_string())
        .collect()
}

fn collect_unsupported_script_object_fields(
    value: Option<&Value>,
    prefix: &str,
    unsupported_fields: &mut Vec<String>,
) {
    let Some(record) = value.and_then(Value::as_object) else {
        return;
    };
    for field in ["before", "after"] {
        if record.contains_key(field) {
            unsupported_fields.push(format!("{prefix}.{field}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reader(files: Vec<(&'static str, String)>) -> impl Fn(&str) -> Option<String> {
        move |path| {
            files
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, content)| content.clone())
        }
    }

    fn inspect(files: Vec<(&'static str, String)>) -> Vec<SetupScriptImportCandidate> {
        let read = reader(files);
        inspect_setup_script_import_candidates(&read, None)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn imports_setup_and_teardown_commands_from_superset_config() {
        let candidates = inspect(vec![(
            ".superset/config.json",
            json!({
                "setup": ["./.superset/setup.sh", "bun install"],
                "teardown": ["./.superset/teardown.sh"],
                "run": ["bun dev"]
            })
            .to_string(),
        )]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "superset".to_string(),
                label: "Superset".to_string(),
                files: strings(&[".superset/config.json"]),
                setup: "./.superset/setup.sh\nbun install".to_string(),
                archive: Some("./.superset/teardown.sh".to_string()),
                unsupported_fields: strings(&["run"]),
            }]
        );
    }

    #[test]
    fn applies_superset_local_before_and_after_setup_overlays() {
        let candidates = inspect(vec![
            (
                ".superset/config.json",
                json!({
                    "setup": ["bun install"],
                    "teardown": ["docker compose down"],
                    "cwd": "packages/web"
                })
                .to_string(),
            ),
            (
                ".superset/config.local.json",
                json!({
                    "setup": { "before": ["corepack enable"], "after": ["bun run db:migrate"] },
                    "teardown": ["docker compose down --remove-orphans"],
                    "run": ["bun dev"]
                })
                .to_string(),
            ),
        ]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "superset".to_string(),
                label: "Superset".to_string(),
                files: strings(&[".superset/config.json", ".superset/config.local.json"]),
                setup: "corepack enable\nbun install\nbun run db:migrate".to_string(),
                archive: Some("docker compose down --remove-orphans".to_string()),
                unsupported_fields: strings(&["cwd", "config.local.run"]),
            }]
        );
    }

    #[test]
    fn reports_unsupported_superset_local_script_object_fields() {
        let candidates = inspect(vec![
            (
                ".superset/config.json",
                json!({ "setup": ["bun install"] }).to_string(),
            ),
            (
                ".superset/config.local.json",
                json!({
                    "setup": {
                        "before": ["corepack enable"],
                        "after": ["bun run db:migrate"],
                        "cwd": "packages/web"
                    }
                })
                .to_string(),
            ),
        ]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "superset".to_string(),
                label: "Superset".to_string(),
                files: strings(&[".superset/config.json", ".superset/config.local.json"]),
                setup: "corepack enable\nbun install\nbun run db:migrate".to_string(),
                archive: None,
                unsupported_fields: strings(&["config.local.setup.cwd"]),
            }]
        );
    }

    #[test]
    fn imports_setup_commands_from_cmux_project_config() {
        let candidates = inspect(vec![(
            ".cmux/cmux.json",
            json!({
                "commands": [
                    {
                        "name": "Run Unit Tests",
                        "keywords": ["test", "unit"],
                        "command": "./scripts/test-unit.sh"
                    },
                    {
                        "name": "Setup",
                        "description": "Initialize submodules and build dependencies",
                        "keywords": ["setup", "init", "install"],
                        "command": "./scripts/setup.sh",
                        "confirm": true,
                        "cwd": "packages/web"
                    }
                ]
            })
            .to_string(),
        )]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "cmux".to_string(),
                label: "cmux".to_string(),
                files: strings(&[".cmux/cmux.json"]),
                setup: "./scripts/setup.sh".to_string(),
                archive: None,
                unsupported_fields: strings(&["commands.1.confirm", "commands.1.cwd"]),
            }]
        );
    }

    #[test]
    fn imports_setup_commands_from_root_cmux_config_when_project_config_is_absent() {
        let candidates = inspect(vec![(
            "cmux.json",
            json!({
                "commands": [
                    { "title": "Workspace Setup", "keywords": ["setup"], "command": "pnpm install" }
                ]
            })
            .to_string(),
        )]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "cmux".to_string(),
                label: "cmux".to_string(),
                files: strings(&["cmux.json"]),
                setup: "pnpm install".to_string(),
                archive: None,
                unsupported_fields: Vec::new(),
            }]
        );
    }

    #[test]
    fn imports_setup_and_archive_commands_from_conductor_config() {
        let candidates = inspect(vec![(
            "conductor.json",
            json!({
                "scripts": { "setup": "pnpm install", "archive": "pnpm clean", "run": "pnpm dev" },
                "runScriptMode": "manual"
            })
            .to_string(),
        )]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "conductor".to_string(),
                label: "Conductor".to_string(),
                files: strings(&["conductor.json"]),
                setup: "pnpm install".to_string(),
                archive: Some("pnpm clean".to_string()),
                unsupported_fields: strings(&["runScriptMode", "scripts.run"]),
            }]
        );
    }

    #[test]
    fn imports_setup_and_cleanup_scripts_from_codex_environment_config() {
        let toml = "\n[setup]\nscript = \"\"\"\nnpm ci\npnpm build\n\"\"\"\n\n[cleanup]\nscript = \"pnpm clean\"\n\n[actions.test]\ncommand = \"pnpm test\"\n";
        let candidates = inspect(vec![(".codex/environments/environment.toml", toml.to_string())]);

        assert_eq!(
            candidates,
            vec![SetupScriptImportCandidate {
                provider: "codex".to_string(),
                label: "Codex environment".to_string(),
                files: strings(&[".codex/environments/environment.toml"]),
                setup: "npm ci\npnpm build".to_string(),
                archive: Some("pnpm clean".to_string()),
                unsupported_fields: strings(&["[actions.test]"]),
            }]
        );
    }

    #[test]
    fn ignores_malformed_or_setup_less_configs() {
        let candidates = inspect(vec![
            (".superset/config.json", "{".to_string()),
            ("conductor.json", json!({ "scripts": { "run": "pnpm dev" } }).to_string()),
            (".codex/environments/environment.toml", "[cleanup]\nscript = \"pnpm clean\"".to_string()),
            (
                ".cmux/cmux.json",
                json!({ "commands": [{ "name": "Build", "keywords": ["build"], "command": "pnpm build" }] })
                    .to_string(),
            ),
        ]);

        assert_eq!(candidates, Vec::new());
    }
}
