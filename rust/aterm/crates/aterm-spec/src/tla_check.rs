// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! TLA+ file parsing for action cross-referencing.
//!
//! Parses `.tla` files to extract module names and definition names,
//! enabling cross-referencing between Kani proofs and TLA+ properties.

use std::collections::BTreeSet;
use std::path::Path;

/// Error parsing a TLA+ file.
#[non_exhaustive]
#[derive(Debug, aterm_error::Error)]
pub enum TlaParseError {
    #[error("failed to read TLA+ file: {0}")]
    Io(#[from] std::io::Error),
    #[error("no MODULE declaration found in {path}")]
    NoModule { path: String },
}

/// A parsed TLA+ specification with module name and extracted definitions.
#[derive(Debug, Clone)]
pub struct TlaSpec {
    /// Module name from the `---- MODULE <name> ----` header.
    pub module_name: String,
    /// File path (for reporting).
    pub file_path: String,
    /// ALL named top-level definitions (actions, invariants, `Next`, `Spec`, named
    /// constants like `BeforeFork`, …). This is the resolution set for obligation 1
    /// ("an anchor names a real definition") — a `#[refines]`/`#[spec_invariant]`
    /// must name SOMETHING defined in the module.
    pub actions: BTreeSet<String>,
    /// The real ACTION names: the disjuncts of `Next == A \/ B \/ …`. This is the
    /// COVERAGE set (obligation 3) — the set every actively-bound external machine
    /// must have bound-or-waived. Excludes `Init`/`Spec`/`TypeOK`/invariants/named
    /// constants, which are NOT actions and must not demand a `#[refines]`.
    pub next_actions: BTreeSet<String>,
}

impl TlaSpec {
    /// Parse a TLA+ file from disk.
    pub fn parse_file(path: &Path) -> Result<Self, TlaParseError> {
        let content = std::fs::read_to_string(path)?;
        let file_path = path.display().to_string();
        Self::parse_str(&content, &file_path)
    }

    /// Parse TLA+ content from a string.
    pub fn parse_str(content: &str, file_path: &str) -> Result<Self, TlaParseError> {
        let module_name = extract_module_name(content).ok_or_else(|| TlaParseError::NoModule {
            path: file_path.to_string(),
        })?;
        let actions = extract_definitions(content);
        let next_actions = extract_next_disjuncts(content);
        Ok(TlaSpec {
            module_name,
            file_path: file_path.to_string(),
            actions,
            next_actions,
        })
    }
}

/// Extract the module name from `---- MODULE <name> ----` header.
fn extract_module_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("MODULE") {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if let Some(idx) = parts.iter().position(|&s| s == "MODULE")
            && idx + 1 < parts.len()
        {
            let name = parts[idx + 1].trim_matches('-').trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Scan a line for `(*` and `*)` delimiters and return the updated nesting depth.
///
/// TLA+ block comments nest: `(* (* inner *) outer *)` is a single comment.
/// This function increments depth on each `(*` and decrements on each `*)`,
/// returning the final depth after processing the entire line.
fn update_comment_depth(line: &str, mut depth: u32) -> u32 {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'(' && bytes[i + 1] == b'*' {
            depth = depth.saturating_add(1);
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b')' {
            depth = depth.saturating_sub(1);
            i += 2;
        } else {
            i += 1;
        }
    }
    depth
}

/// Extract all top-level definitions from TLA+ content.
///
/// Matches definitions of the form `Name ==` or `Name(params) ==` at column 0,
/// plus `THEOREM Name ==` patterns. Skips TLA+ keywords and comment lines.
fn extract_definitions(content: &str) -> BTreeSet<String> {
    let mut defs = BTreeSet::new();
    let mut comment_depth: u32 = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // Update block comment nesting depth by scanning for `(*` and `*)`.
        // TLA+ block comments nest: `(* (* inner *) outer *)` is one comment.
        if comment_depth > 0 || trimmed.contains("(*") {
            comment_depth = update_comment_depth(trimmed, comment_depth);
            // Skip any line that participates in block comment state.
            continue;
        }

        // Skip line comments
        if trimmed.starts_with("\\*") {
            continue;
        }

        // Skip indented lines (inside LET blocks, etc.)
        if !line.is_empty() && line.starts_with(|c: char| c.is_whitespace()) {
            let indent = line.len() - line.trim_start().len();
            if indent > 4 {
                continue;
            }
        }

        if let Some(name) = extract_definition_name(trimmed)
            && !is_tla_keyword(&name)
        {
            defs.insert(name);
        }
    }
    defs
}

/// Extract the ACTION names from the `Next == …` definition — the disjuncts of the
/// next-state relation. These are the real actions a machine offers, as opposed to
/// every top-level `==` def (which also includes `Init`/`Spec`/`TypeOK`/invariants/
/// named constants). Used for the COVERAGE obligation (every action bound-or-waived).
///
/// Handles the two shapes the ISOLATION specs use:
///   * a plain disjunction `Next == Fork \/ Setrlimit \/ … \/ Exec`, and
///   * quantified disjuncts `Next == \/ \E c \in … : WriteMain(c, v) \/ Enter \/ …`
///     — the called action identifier (`WriteMain`, before its `(`) is extracted.
/// The body may span multiple (indented) lines; we gather until the next top-level
/// definition (a non-indented `Name ==`) or the module end.
fn extract_next_disjuncts(content: &str) -> BTreeSet<String> {
    // Gather the full `Next == …` body (possibly multi-line).
    let mut body = String::new();
    let mut in_next = false;
    let mut comment_depth: u32 = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if comment_depth > 0 || trimmed.contains("(*") {
            comment_depth = update_comment_depth(trimmed, comment_depth);
            continue;
        }
        let code = match trimmed.split_once("\\*") {
            Some((before, _)) => before.trim_end(),
            None => trimmed,
        };
        if in_next {
            // A new top-level definition (non-indented `Name ==`) or module end stops it.
            let is_new_def = !line.starts_with(char::is_whitespace)
                && (code.contains("==") || code.starts_with("===") || code.is_empty());
            if is_new_def {
                break;
            }
            body.push(' ');
            body.push_str(code);
        } else if code.starts_with("Next") {
            if let Some((lhs, rhs)) = code.split_once("==") {
                if lhs.trim() == "Next" {
                    in_next = true;
                    body.push_str(rhs);
                }
            }
        }
    }

    // Tokenize the body: split on `\/`, then for each disjunct take the FIRST
    // identifier that is a callable action — i.e. skip the quantifier prelude
    // (`\E c \in 1..Cells, v \in 1..MaxVal :`) and grab the name after the `:` (or
    // the bare identifier when there is no quantifier).
    let mut actions = BTreeSet::new();
    for disj in body.split("\\/") {
        let segment = match disj.rsplit_once(':') {
            // After a quantifier colon, the action call follows.
            Some((_, after)) => after,
            None => disj,
        };
        if let Some(name) = leading_identifier(segment) {
            if !is_tla_keyword(&name) && name != "UNCHANGED" {
                actions.insert(name);
            }
        }
    }
    actions
}

/// The first TLA+ identifier in `s` (leading whitespace skipped), or `None`.
fn leading_identifier(s: &str) -> Option<String> {
    let s = s.trim_start();
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() && first != '_' {
        return None;
    }
    let mut name = String::new();
    name.push(first);
    for c in chars {
        if c.is_alphanumeric() || c == '_' {
            name.push(c);
        } else {
            break;
        }
    }
    Some(name)
}

/// Try to extract a definition name from a single line.
fn extract_definition_name(line: &str) -> Option<String> {
    // Handle `THEOREM Name ==` and `LOCAL Name ==` prefixes
    let effective = if line.starts_with("THEOREM ") {
        line.strip_prefix("THEOREM ")?.trim_start()
    } else if line.starts_with("LOCAL ") {
        line.strip_prefix("LOCAL ")?.trim_start()
    } else {
        line
    };

    // Find `==` that isn't part of `====` (end-of-module marker)
    let eq_pos = effective.find("==")?;

    // Skip if this is the `====` end-of-module marker
    if effective[eq_pos..].starts_with("====") {
        return None;
    }

    let before = effective[..eq_pos].trim();
    if before.is_empty() {
        return None;
    }

    // Strip parameters: `Name(params)` → `Name`
    let name = match before.find('(') {
        Some(p) => before[..p].trim(),
        None => before,
    };

    // Validate identifier
    if name.is_empty() {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() && first != '_' {
        return None;
    }
    if !chars.all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    Some(name.to_string())
}

fn is_tla_keyword(name: &str) -> bool {
    matches!(
        name,
        "CONSTANTS"
            | "CONSTANT"
            | "VARIABLES"
            | "VARIABLE"
            | "ASSUME"
            | "EXTENDS"
            | "INSTANCE"
            | "MODULE"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_module() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Modes ----
VARIABLES mode

SetMode == mode' = TRUE
ResetMode == mode' = FALSE

Next == SetMode \/ ResetMode
====
"#,
            "tla/Modes.tla",
        )
        .unwrap();

        assert_eq!(spec.module_name, "Modes");
        assert_eq!(spec.file_path, "tla/Modes.tla");
        assert!(spec.actions.contains("SetMode"));
        assert!(spec.actions.contains("ResetMode"));
        assert!(spec.actions.contains("Next"));
    }

    #[test]
    fn test_parse_parametric_definition() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Test ----
VARIABLES x

Inc(n) == x' = x + n
Dec == x' = x - 1

Next == Inc(1) \/ Dec
====
"#,
            "test.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Inc"));
        assert!(spec.actions.contains("Dec"));
        assert!(spec.actions.contains("Next"));
    }

    #[test]
    fn test_parse_theorem_definitions() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Thm ----
VARIABLES x

Safety == x >= 0

THEOREM SafetyHolds == Spec => []Safety
====
"#,
            "thm.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Safety"));
        assert!(spec.actions.contains("SafetyHolds"));
    }

    #[test]
    fn test_no_module_returns_error() {
        let result = TlaSpec::parse_str("no module here", "bad.tla");
        assert!(result.is_err());
    }

    #[test]
    fn test_skips_keywords() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE KW ----
VARIABLES x
Init == x = 0
====
"#,
            "kw.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Init"));
        assert!(!spec.actions.contains("VARIABLES"));
    }

    #[test]
    fn test_end_marker_not_parsed_as_definition() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE End ----
VARIABLES x
Init == x = 0
=============================================================================
"#,
            "end.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Init"));
        // The ==== end marker should not produce a definition
        assert_eq!(spec.actions.len(), 1);
    }

    #[test]
    fn test_nested_block_comments_parse_correctly() {
        // Nested TLA+ block comments: the first `*)` closes the inner comment,
        // not the outer one. The parser must track nesting depth.
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Nested ----
VARIABLES x

(* outer comment
   (* inner comment *)
   still inside outer comment
*)

Init == x = 0
Next == x' = x + 1
====
"#,
            "nested.tla",
        )
        .unwrap();

        assert_eq!(spec.module_name, "Nested");
        assert!(spec.actions.contains("Init"));
        assert!(spec.actions.contains("Next"));
        // Only Init and Next should be extracted; nothing from inside the comment.
        assert_eq!(spec.actions.len(), 2);
    }

    #[test]
    fn test_deeply_nested_block_comments() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Deep ----
VARIABLES x

(* level 1
   (* level 2
      (* level 3 *)
   *)
*)

Visible == x = 0
====
"#,
            "deep.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Visible"));
        assert_eq!(spec.actions.len(), 1);
    }

    #[test]
    fn next_disjuncts_plain_and_quantified() {
        // Plain disjunction.
        let plain = extract_next_disjuncts(
            "Next == Fork \\/ Setrlimit \\/ Chdir \\/ CloseMaster \\/ UnsafeEnvOp \\/ Exec\n",
        );
        assert_eq!(
            plain,
            ["Fork", "Setrlimit", "Chdir", "CloseMaster", "UnsafeEnvOp", "Exec"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
        // Quantified, multi-line (the AltScreen shape): the called action after the
        // `\E … :` quantifier colon is what we extract.
        let quant = extract_next_disjuncts(
            "Next ==\n    \\/ \\E c \\in 1..Cells, v \\in 1..MaxVal : WriteMain(c, v)\n    \\/ Enter\n    \\/ \\E c \\in 1..Cells, v \\in 1..MaxVal : Scribble(c, v)\n    \\/ Leave\n\nSpec == Init\n",
        );
        assert_eq!(
            quant,
            ["WriteMain", "Enter", "Scribble", "Leave"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn next_disjuncts_single_action() {
        // A `Next == Apply` (single action) — Sandbox / PathConfine shape.
        assert_eq!(
            extract_next_disjuncts("Next == Apply\n"),
            ["Apply"].iter().map(|s| s.to_string()).collect()
        );
        assert_eq!(
            extract_next_disjuncts("Next == Confine\n"),
            ["Confine"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn test_single_line_block_comment() {
        let spec = TlaSpec::parse_str(
            r#"
---- MODULE Inline ----
VARIABLES x

(* this is a single-line block comment *)

Init == x = 0
====
"#,
            "inline.tla",
        )
        .unwrap();

        assert!(spec.actions.contains("Init"));
        assert_eq!(spec.actions.len(), 1);
    }
}
