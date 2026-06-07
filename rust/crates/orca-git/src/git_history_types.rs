//! Shared git-history view types and constants, ported from
//! `src/shared/git-history-types.ts`. The lane colors, default/max limits, and
//! the color ids drive the swimlane graph (`git_history_graph`) and the loader
//! (`git_history`); the [`GitHistoryExecutor`] is the injected IO boundary that
//! lets the loader run against a real `git` or a mock.

/// Stable color identifier used by the graph view-model. Each variant maps to a
/// CSS token id (`as_str`); the renderer resolves the actual color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GitHistoryGraphColorId {
    Ref,
    RemoteRef,
    BaseRef,
    Lane1,
    Lane2,
    Lane3,
    Lane4,
    Lane5,
}

impl GitHistoryGraphColorId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ref => "git-graph-ref",
            Self::RemoteRef => "git-graph-remote-ref",
            Self::BaseRef => "git-graph-base-ref",
            Self::Lane1 => "git-graph-lane-1",
            Self::Lane2 => "git-graph-lane-2",
            Self::Lane3 => "git-graph-lane-3",
            Self::Lane4 => "git-graph-lane-4",
            Self::Lane5 => "git-graph-lane-5",
        }
    }
}

pub const GIT_HISTORY_REF_COLOR: GitHistoryGraphColorId = GitHistoryGraphColorId::Ref;
pub const GIT_HISTORY_REMOTE_REF_COLOR: GitHistoryGraphColorId = GitHistoryGraphColorId::RemoteRef;
pub const GIT_HISTORY_BASE_REF_COLOR: GitHistoryGraphColorId = GitHistoryGraphColorId::BaseRef;

/// Lanes rotate through these five colors (index wraps via `rotate`).
pub const GIT_HISTORY_LANE_COLORS: [GitHistoryGraphColorId; 5] = [
    GitHistoryGraphColorId::Lane1,
    GitHistoryGraphColorId::Lane2,
    GitHistoryGraphColorId::Lane3,
    GitHistoryGraphColorId::Lane4,
    GitHistoryGraphColorId::Lane5,
];

pub const GIT_HISTORY_DEFAULT_LIMIT: i64 = 50;
pub const GIT_HISTORY_MAX_LIMIT: i64 = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHistoryRefCategory {
    Branches,
    RemoteBranches,
    Tags,
    Commits,
}

impl GitHistoryRefCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Branches => "branches",
            Self::RemoteBranches => "remote branches",
            Self::Tags => "tags",
            Self::Commits => "commits",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryItemRef {
    pub id: String,
    pub name: String,
    pub revision: Option<String>,
    pub category: Option<GitHistoryRefCategory>,
    pub description: Option<String>,
    pub color: Option<GitHistoryGraphColorId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHistoryItemStatistics {
    pub files: i64,
    pub insertions: i64,
    pub deletions: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryItem {
    pub id: String,
    pub parent_ids: Vec<String>,
    pub subject: String,
    pub message: String,
    pub display_id: Option<String>,
    pub author: Option<String>,
    pub author_email: Option<String>,
    pub timestamp: Option<i64>,
    pub statistics: Option<GitHistoryItemStatistics>,
    pub references: Option<Vec<GitHistoryItemRef>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryOptions {
    pub limit: Option<i64>,
    pub base_ref: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryResult {
    pub items: Vec<GitHistoryItem>,
    pub current_ref: Option<GitHistoryItemRef>,
    pub remote_ref: Option<GitHistoryItemRef>,
    pub base_ref: Option<GitHistoryItemRef>,
    pub merge_base: Option<String>,
    pub has_incoming_changes: bool,
    pub has_outgoing_changes: bool,
    pub has_more: bool,
    pub limit: i64,
}

/// Captured stdout from a git invocation (stderr is unused by the loader, like
/// the TS `{ stdout: string; stderr?: string }`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryExecOutput {
    pub stdout: String,
}

/// A failed git invocation. The loader treats every error as a soft fallback
/// (the TS code wraps each call in `try/catch`), so the message is advisory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitHistoryExecError {
    pub message: String,
}

impl GitHistoryExecError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// The injected IO boundary: runs `git <args>` in `cwd`. Mirrors the TS
/// `GitHistoryExecutor = (args, cwd) => Promise<{ stdout }>`. Tests supply a
/// closure; production wraps the real `git` binary.
pub trait GitHistoryExecutor {
    fn run(&self, args: &[&str], cwd: &str) -> Result<GitHistoryExecOutput, GitHistoryExecError>;
}

/// Any `Fn(&[&str], &str) -> Result<..>` is an executor, so tests need no struct.
impl<F> GitHistoryExecutor for F
where
    F: Fn(&[&str], &str) -> Result<GitHistoryExecOutput, GitHistoryExecError>,
{
    fn run(&self, args: &[&str], cwd: &str) -> Result<GitHistoryExecOutput, GitHistoryExecError> {
        self(args, cwd)
    }
}
