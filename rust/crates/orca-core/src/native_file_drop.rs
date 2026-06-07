//! Native OS file-drop routing, ported from `src/shared/native-file-drop.ts`.
//!
//! Decides where an OS file drag/drop lands by walking the event target path
//! (innermost first), distinguishing genuine OS drags from Orca's own internal
//! file moves. Pure: the DOM `DataTransfer` types and `composedPath` are modelled
//! as plain slices.

pub const ORCA_INTERNAL_FILE_DRAG_TYPE: &str = "text/x-orca-file-path";

pub const TARGET_EDITOR: &str = "editor";
pub const TARGET_TERMINAL: &str = "terminal";
pub const TARGET_COMPOSER: &str = "composer";
pub const TARGET_FILE_EXPLORER: &str = "file-explorer";
pub const TARGET_PROJECT_SIDEBAR: &str = "project-sidebar";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeDropResolution {
    Editor,
    Terminal { tab_id: Option<String> },
    Composer,
    FileExplorer { destination_dir: String },
    ProjectSidebar,
    Rejected,
}

/// One node on the drop event's target path.
#[derive(Clone, Debug, Default)]
pub struct NativeFileDropPathEntry {
    pub native_file_drop_target: Option<String>,
    pub native_file_drop_dir: Option<String>,
    pub terminal_tab_id: Option<String>,
}

/// True for a genuine OS file drag: carries `Files` and is **not** one of Orca's
/// own internal file moves (which would also set the internal drag type).
pub fn has_native_file_drag_types(types: &[&str]) -> bool {
    types.contains(&"Files") && !types.contains(&ORCA_INTERNAL_FILE_DRAG_TYPE)
}

/// Resolve where a native file drop should go by walking the target path
/// innermost-first. Returns `None` when no surface claims the drop.
pub fn resolve_native_file_drop_path(path: &[NativeFileDropPathEntry]) -> Option<NativeDropResolution> {
    let mut found_explorer = false;
    let mut destination_dir: Option<String> = None;

    for entry in path {
        match entry.native_file_drop_target.as_deref() {
            Some(TARGET_TERMINAL) => {
                return Some(NativeDropResolution::Terminal { tab_id: entry.terminal_tab_id.clone() });
            }
            Some(TARGET_EDITOR) => return Some(NativeDropResolution::Editor),
            Some(TARGET_COMPOSER) => return Some(NativeDropResolution::Composer),
            Some(TARGET_PROJECT_SIDEBAR) => return Some(NativeDropResolution::ProjectSidebar),
            Some(TARGET_FILE_EXPLORER) => found_explorer = true,
            _ => {}
        }
        // Pick the nearest (innermost) destination directory marker.
        if destination_dir.is_none() {
            if let Some(dir) = entry.native_file_drop_dir.as_ref().filter(|dir| !dir.is_empty()) {
                destination_dir = Some(dir.clone());
            }
        }
    }

    if found_explorer {
        // Fail closed: a file-explorer drop with no resolved directory is rejected.
        return Some(match destination_dir {
            Some(dir) => NativeDropResolution::FileExplorer { destination_dir: dir },
            None => NativeDropResolution::Rejected,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_entry(target: &str) -> NativeFileDropPathEntry {
        NativeFileDropPathEntry { native_file_drop_target: Some(target.to_string()), ..Default::default() }
    }

    #[test]
    fn accepts_native_os_file_drags() {
        assert!(has_native_file_drag_types(&["Files"]));
    }

    #[test]
    fn rejects_internal_orca_file_moves_and_url_text_drags() {
        assert!(!has_native_file_drag_types(&["Files", ORCA_INTERNAL_FILE_DRAG_TYPE]));
        assert!(!has_native_file_drag_types(&["text/uri-list"]));
        assert!(!has_native_file_drag_types(&["text/plain"]));
    }

    #[test]
    fn routes_drops_on_the_project_sidebar_to_the_add_project_surface() {
        assert_eq!(
            resolve_native_file_drop_path(&[target_entry(TARGET_PROJECT_SIDEBAR)]),
            Some(NativeDropResolution::ProjectSidebar)
        );
    }

    #[test]
    fn preserves_terminal_tab_routing_for_native_file_drops() {
        let entry = NativeFileDropPathEntry {
            native_file_drop_target: Some(TARGET_TERMINAL.to_string()),
            terminal_tab_id: Some("tab-1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            resolve_native_file_drop_path(&[entry]),
            Some(NativeDropResolution::Terminal { tab_id: Some("tab-1".to_string()) })
        );
    }

    #[test]
    fn uses_the_nearest_file_explorer_destination_and_fails_closed_without_one() {
        let path = [
            NativeFileDropPathEntry {
                native_file_drop_dir: Some("/repo/src".to_string()),
                ..Default::default()
            },
            NativeFileDropPathEntry {
                native_file_drop_target: Some(TARGET_FILE_EXPLORER.to_string()),
                native_file_drop_dir: Some("/repo".to_string()),
                ..Default::default()
            },
        ];
        assert_eq!(
            resolve_native_file_drop_path(&path),
            Some(NativeDropResolution::FileExplorer { destination_dir: "/repo/src".to_string() })
        );

        assert_eq!(
            resolve_native_file_drop_path(&[target_entry(TARGET_FILE_EXPLORER)]),
            Some(NativeDropResolution::Rejected)
        );
    }
}
