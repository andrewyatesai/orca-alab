//! Parity dispatch for `orca_core::native_file_drop` vs
//! `src/shared/native-file-drop.ts`.

use orca_core::native_file_drop::{
    has_native_file_drag_types, resolve_native_file_drop_path, NativeDropResolution,
    NativeFileDropPathEntry, TARGET_COMPOSER, TARGET_EDITOR, TARGET_FILE_EXPLORER,
    TARGET_PROJECT_SIDEBAR, TARGET_TERMINAL,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "hasNativeFileDragTypes" => {
            let types = type_strs(input);
            Value::Bool(has_native_file_drag_types(&types))
        }
        "resolveNativeFileDropPath" => {
            let path = parse_path(input);
            match resolve_native_file_drop_path(&path) {
                // TS returns `null` when no surface claims the drop.
                None => Value::Null,
                Some(resolution) => resolution_to_json(&resolution),
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Collect the `DataTransfer.types` strings; a non-array (e.g. `null`) input is
/// empty, matching the TS `getDataTransferTypes` null guard.
fn type_strs(input: &Value) -> Vec<&str> {
    input
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn parse_path(input: &Value) -> Vec<NativeFileDropPathEntry> {
    input
        .as_array()
        .map(|items| items.iter().map(parse_entry).collect())
        .unwrap_or_default()
}

fn parse_entry(value: &Value) -> NativeFileDropPathEntry {
    NativeFileDropPathEntry {
        native_file_drop_target: string_field(value, "nativeFileDropTarget"),
        native_file_drop_dir: string_field(value, "nativeFileDropDir"),
        terminal_tab_id: string_field(value, "terminalTabId"),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Match `JSON.stringify` of the TS `NativeDropResolution` union: a `target`
/// string id plus the variant's extra field, with `tabId` omitted (not `null`)
/// when the terminal drop carries no tab id.
fn resolution_to_json(resolution: &NativeDropResolution) -> Value {
    match resolution {
        NativeDropResolution::Editor => json!({ "target": TARGET_EDITOR }),
        NativeDropResolution::Composer => json!({ "target": TARGET_COMPOSER }),
        NativeDropResolution::ProjectSidebar => json!({ "target": TARGET_PROJECT_SIDEBAR }),
        NativeDropResolution::Rejected => json!({ "target": "rejected" }),
        NativeDropResolution::FileExplorer { destination_dir } => {
            json!({ "target": TARGET_FILE_EXPLORER, "destinationDir": destination_dir })
        }
        NativeDropResolution::Terminal { tab_id } => {
            let mut map = Map::new();
            map.insert("target".to_string(), Value::String(TARGET_TERMINAL.to_string()));
            if let Some(id) = tab_id {
                map.insert("tabId".to_string(), Value::String(id.clone()));
            }
            Value::Object(map)
        }
    }
}
