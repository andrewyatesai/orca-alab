//! Parity dispatch for `orca_text::workspace_name` vs
//! `src/shared/workspace-name.ts`.

use orca_text::workspace_name::{
    get_linear_issue_workspace_name, get_linked_work_item_suggested_name,
    get_linked_work_item_workspace_name, get_workspace_intent_name, slugify_for_workspace_name,
    WorkItemType, WorkspaceIntentArgs, WorkspaceIntentName, WorkspaceIntentWorkItem,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single string arg; returns the git-ref-safe workspace seed slug.
        "slugifyForWorkspaceName" => match input.as_str() {
            Some(text) => Value::String(slugify_for_workspace_name(text)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "slugifyForWorkspaceName expects a string input" }),
        },
        // `{title}` object in (the TS takes `{ title: string }`), slug out.
        "getLinkedWorkItemSuggestedName" => {
            let title = input.get("title").and_then(Value::as_str).unwrap_or_default();
            Value::String(get_linked_work_item_suggested_name(title))
        }
        "getWorkspaceIntentName" => {
            let args = WorkspaceIntentArgs {
                source_text: input.get("sourceText").and_then(Value::as_str).map(str::to_string),
                work_item: input.get("workItem").and_then(work_item_from_input),
                fallback_name: input
                    .get("fallbackName")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            intent_name_to_json(get_workspace_intent_name(&args))
        }
        "getLinkedWorkItemWorkspaceName" => match work_item_from_input(input) {
            Some(item) => intent_name_to_json(get_linked_work_item_workspace_name(&item)),
            None => json!({ "__parity_error__": "getLinkedWorkItemWorkspaceName expects an item" }),
        },
        // `{identifier, title}` in, dedup-aware combined seed slug out.
        "getLinearIssueWorkspaceName" => {
            let identifier = input.get("identifier").and_then(Value::as_str).unwrap_or_default();
            let title = input.get("title").and_then(Value::as_str).unwrap_or_default();
            Value::String(get_linear_issue_workspace_name(identifier, title))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `WorkspaceIntentName` (camelCase, or null).
fn intent_name_to_json(name: Option<WorkspaceIntentName>) -> Value {
    match name {
        Some(name) => json!({ "displayName": name.display_name, "seedName": name.seed_name }),
        None => Value::Null,
    }
}

fn work_item_from_input(input: &Value) -> Option<WorkspaceIntentWorkItem> {
    let object = input.as_object()?;
    let kind = match object.get("type").and_then(Value::as_str) {
        Some("pr") => Some(WorkItemType::Pr),
        Some("mr") => Some(WorkItemType::Mr),
        Some("issue") => Some(WorkItemType::Issue),
        _ => None,
    };
    Some(WorkspaceIntentWorkItem {
        kind,
        number: object.get("number").and_then(Value::as_u64).unwrap_or_default(),
        title: object.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
        linear_identifier: object
            .get("linearIdentifier")
            .and_then(Value::as_str)
            .map(str::to_string),
        jira_identifier: object.get("jiraIdentifier").and_then(Value::as_str).map(str::to_string),
    })
}
