//! Hosted-review queue classification, ported from `src/shared/hosted-review-queue.ts`.
//!
//! Provider-neutral (GitHub / GitLab): classifies a review's queue bucket
//! (mine / requested / agent / teammate), whether it needs a response, and
//! whether it is ready to merge. Pure; only the summary fields the classifier
//! reads are modelled. The `updatedAt > lastViewedAt` check uses a small
//! hand-rolled UTC ISO-8601 parser (no date crate).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedReviewIdentity {
    /// `"github"` | `"gitlab"`.
    pub provider: String,
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostedReviewUser {
    pub login: Option<String>,
    pub is_bot: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadSummary {
    pub unresolved_count: u64,
}

/// The summary fields the classifier reads (the full record carries more).
#[derive(Clone, Debug, Default)]
pub struct HostedReviewQueueSummary {
    pub identity: HostedReviewIdentity,
    /// `"open"` | `"draft"` | `"merged"` | `"closed"` | …
    pub state: String,
    pub author: Option<HostedReviewUser>,
    pub requested_reviewer_logins: Option<Vec<String>>,
    pub updated_at: String,
    /// Epoch-ms the viewer last looked, if ever.
    pub last_viewed_at: Option<i64>,
    /// `"MERGEABLE"` | `"CONFLICTING"` | `"UNKNOWN"` | …
    pub mergeable: Option<String>,
    /// `"success"` | `"failure"` | `"neutral"` | `"pending"` | …
    pub checks_status: Option<String>,
    pub thread_summary: Option<ThreadSummary>,
    pub draft: bool,
    /// GitHub-only: `"BEHIND"` | `"BLOCKED"` | …
    pub merge_state_status: Option<String>,
    /// `"review_required"` | `"changes_requested"` | `"approved"` | …
    pub review_decision: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedReviewQueueState {
    Mine,
    Requested,
    Agent,
    Teammate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostedReviewQueueClassification {
    pub state: HostedReviewQueueState,
    pub requested: bool,
    pub needs_response: bool,
    pub ready_to_merge: bool,
}

#[derive(Clone, Debug, Default)]
pub struct HostedReviewClassificationOptions {
    pub viewer: Option<HostedReviewUser>,
    pub agent_author_logins: Vec<String>,
}

/// Enterprise-safe identity key: provider + host disambiguate dotcom from a
/// self-hosted instance with the same owner/repo/number.
pub fn hosted_review_identity_key(identity: &HostedReviewIdentity) -> String {
    format!(
        "{}::{}::{}::{}::{}",
        identity.provider,
        identity.host.to_lowercase(),
        identity.owner.to_lowercase(),
        identity.repo.to_lowercase(),
        identity.number
    )
}

fn nonempty_lower(login: Option<&str>) -> Option<String> {
    login.filter(|value| !value.is_empty()).map(str::to_lowercase)
}

fn has_requested_reviewer_signal(summary: &HostedReviewQueueSummary, viewer: Option<&HostedReviewUser>) -> bool {
    let Some(viewer_login) = viewer.and_then(|user| nonempty_lower(user.login.as_deref())) else {
        return false;
    };
    let Some(requested) = summary.requested_reviewer_logins.as_ref().filter(|list| !list.is_empty()) else {
        return false;
    };
    requested.iter().any(|login| login.to_lowercase() == viewer_login)
}

fn is_agent_authored(summary: &HostedReviewQueueSummary, options: &HostedReviewClassificationOptions) -> bool {
    if summary.author.as_ref().is_some_and(|author| author.is_bot) {
        return true;
    }
    let Some(author) = summary.author.as_ref().and_then(|author| nonempty_lower(author.login.as_deref())) else {
        return false;
    };
    if options.agent_author_logins.iter().any(|login| login.to_lowercase() == author) {
        return true;
    }
    author.ends_with("[bot]") || author.contains("bot")
}

fn get_queue_state(summary: &HostedReviewQueueSummary, options: &HostedReviewClassificationOptions) -> HostedReviewQueueState {
    let viewer_login = options.viewer.as_ref().and_then(|user| nonempty_lower(user.login.as_deref()));
    let author_login = summary.author.as_ref().and_then(|author| nonempty_lower(author.login.as_deref()));
    if let (Some(viewer), Some(author)) = (&viewer_login, &author_login) {
        if viewer == author {
            return HostedReviewQueueState::Mine;
        }
    }
    if has_requested_reviewer_signal(summary, options.viewer.as_ref()) {
        return HostedReviewQueueState::Requested;
    }
    if is_agent_authored(summary, options) {
        return HostedReviewQueueState::Agent;
    }
    HostedReviewQueueState::Teammate
}

pub fn review_needs_response(summary: &HostedReviewQueueSummary) -> bool {
    if summary.state != "open" && summary.state != "draft" {
        return false;
    }
    if summary.thread_summary.map_or(0, |thread| thread.unresolved_count) > 0 {
        return true;
    }
    if summary.checks_status.as_deref() == Some("failure") {
        return true;
    }
    if summary.mergeable.as_deref() == Some("CONFLICTING") {
        return true;
    }
    let Some(last_viewed_at) = summary.last_viewed_at else {
        return false;
    };
    parse_iso8601_utc_ms(&summary.updated_at).is_some_and(|updated| updated > last_viewed_at)
}

pub fn review_ready_to_merge(summary: &HostedReviewQueueSummary) -> bool {
    if summary.state != "open" || summary.draft {
        return false;
    }
    if summary.mergeable.as_deref() != Some("MERGEABLE") {
        return false;
    }
    // Merge-state blockers are a GitHub concept; don't gate other providers on them.
    if summary.identity.provider == "github"
        && matches!(summary.merge_state_status.as_deref(), Some("BEHIND") | Some("BLOCKED"))
    {
        return false;
    }
    if matches!(summary.review_decision.as_deref(), Some("review_required") | Some("changes_requested")) {
        return false;
    }
    if !matches!(summary.checks_status.as_deref(), Some("success") | Some("neutral")) {
        return false;
    }
    summary.thread_summary.map(|thread| thread.unresolved_count) == Some(0)
}

pub fn classify_hosted_review(
    summary: &HostedReviewQueueSummary,
    options: &HostedReviewClassificationOptions,
) -> HostedReviewQueueClassification {
    HostedReviewQueueClassification {
        state: get_queue_state(summary, options),
        requested: has_requested_reviewer_signal(summary, options.viewer.as_ref()),
        needs_response: review_needs_response(summary),
        ready_to_merge: review_ready_to_merge(summary),
    }
}

/// Parse a `YYYY-MM-DDTHH:MM:SS(.sss)?Z` UTC timestamp to epoch milliseconds.
/// `None` on a malformed string (mirrors `Date.parse` → `NaN` → not-finite).
fn parse_iso8601_utc_ms(value: &str) -> Option<i64> {
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: i64 = date_parts.next()?.parse().ok()?;
    let day: i64 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let time = time.strip_suffix('Z').unwrap_or(time);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut hms_parts = hms.split(':');
    let hour: i64 = hms_parts.next()?.parse().ok()?;
    let minute: i64 = hms_parts.next()?.parse().ok()?;
    let second: i64 = hms_parts.next().unwrap_or("0").parse().ok()?;

    let mut millis: i64 = 0;
    for (index, digit) in frac.chars().take(3).enumerate() {
        millis += i64::from(digit.to_digit(10)?) * 10i64.pow(2 - index as u32);
    }

    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000 + millis)
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`; integer, panic-free).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use HostedReviewQueueState::{Agent, Mine, Requested, Teammate};

    fn user(login: &str) -> HostedReviewUser {
        HostedReviewUser { login: Some(login.to_string()), is_bot: false }
    }

    fn base_summary() -> HostedReviewQueueSummary {
        HostedReviewQueueSummary {
            identity: HostedReviewIdentity {
                provider: "github".to_string(),
                host: "github.com".to_string(),
                owner: "acme".to_string(),
                repo: "orca".to_string(),
                number: 42,
            },
            state: "open".to_string(),
            author: Some(user("teammate")),
            updated_at: "2026-05-10T00:00:00.000Z".to_string(),
            mergeable: Some("MERGEABLE".to_string()),
            checks_status: Some("success".to_string()),
            thread_summary: Some(ThreadSummary { unresolved_count: 0 }),
            ..Default::default()
        }
    }

    fn no_options() -> HostedReviewClassificationOptions {
        HostedReviewClassificationOptions::default()
    }

    #[test]
    fn parses_iso8601_to_epoch_ms() {
        assert_eq!(parse_iso8601_utc_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso8601_utc_ms("1970-01-02T00:00:00.000Z"), Some(86_400_000));
        assert!(
            parse_iso8601_utc_ms("2026-05-11T00:00:00.000Z") > parse_iso8601_utc_ms("2026-05-10T00:00:00.000Z")
        );
        assert_eq!(parse_iso8601_utc_ms("not-a-date"), None);
    }

    #[test]
    fn identity_key_includes_provider_and_host_for_enterprise_safe_keys() {
        let dotcom = hosted_review_identity_key(&HostedReviewIdentity {
            provider: "github".to_string(),
            host: "github.com".to_string(),
            owner: "acme".to_string(),
            repo: "orca".to_string(),
            number: 7,
        });
        let ghe = hosted_review_identity_key(&HostedReviewIdentity {
            provider: "github".to_string(),
            host: "github.acme.internal".to_string(),
            owner: "acme".to_string(),
            repo: "orca".to_string(),
            number: 7,
        });
        assert_ne!(dotcom, ghe);
    }

    #[test]
    fn classifies_mine_requested_agent_teammate() {
        let mut mine = base_summary();
        mine.author = Some(user("me"));
        let options = HostedReviewClassificationOptions { viewer: Some(user("me")), ..Default::default() };
        assert_eq!(classify_hosted_review(&mine, &options).state, Mine);

        let mut requested = base_summary();
        requested.requested_reviewer_logins = Some(vec!["me".to_string()]);
        let options = HostedReviewClassificationOptions { viewer: Some(user("me")), ..Default::default() };
        assert_eq!(classify_hosted_review(&requested, &options).state, Requested);

        let mut agent = base_summary();
        agent.author = Some(user("orca-ci"));
        let options =
            HostedReviewClassificationOptions { agent_author_logins: vec!["orca-ci".to_string()], ..Default::default() };
        assert_eq!(classify_hosted_review(&agent, &options).state, Agent);

        assert_eq!(classify_hosted_review(&base_summary(), &no_options()).state, Teammate);
    }

    #[test]
    fn needs_response_for_unresolved_failed_conflicting_and_newer_updates() {
        let mut unresolved = base_summary();
        unresolved.thread_summary = Some(ThreadSummary { unresolved_count: 1 });
        assert!(review_needs_response(&unresolved));

        let mut failed = base_summary();
        failed.checks_status = Some("failure".to_string());
        assert!(review_needs_response(&failed));

        let mut conflicting = base_summary();
        conflicting.mergeable = Some("CONFLICTING".to_string());
        assert!(review_needs_response(&conflicting));

        let mut newer = base_summary();
        newer.updated_at = "2026-05-11T00:00:00.000Z".to_string();
        newer.last_viewed_at = parse_iso8601_utc_ms("2026-05-10T00:00:00.000Z");
        assert!(review_needs_response(&newer));
    }

    #[test]
    fn no_needs_response_from_updated_at_alone_when_last_viewed_missing() {
        let mut summary = base_summary();
        summary.updated_at = "2026-05-11T00:00:00.000Z".to_string();
        assert!(!review_needs_response(&summary));
    }

    #[test]
    fn ready_to_merge_rejects_blocked_states() {
        let reject = |mutate: &dyn Fn(&mut HostedReviewQueueSummary)| {
            let mut summary = base_summary();
            mutate(&mut summary);
            assert!(!review_ready_to_merge(&summary));
        };
        reject(&|s| {
            s.state = "draft".to_string();
            s.draft = true;
        });
        reject(&|s| s.mergeable = Some("CONFLICTING".to_string()));
        reject(&|s| s.checks_status = Some("failure".to_string()));
        reject(&|s| s.checks_status = Some("pending".to_string()));
        reject(&|s| s.thread_summary = Some(ThreadSummary { unresolved_count: 2 }));
        reject(&|s| s.thread_summary = None);
        reject(&|s| s.mergeable = Some("UNKNOWN".to_string()));
        reject(&|s| s.review_decision = Some("review_required".to_string()));
        reject(&|s| s.review_decision = Some("changes_requested".to_string()));
        reject(&|s| s.merge_state_status = Some("BEHIND".to_string()));
        reject(&|s| s.merge_state_status = Some("BLOCKED".to_string()));
    }

    #[test]
    fn ready_to_merge_accepts_neutral_checks() {
        let mut summary = base_summary();
        summary.checks_status = Some("neutral".to_string());
        assert!(review_ready_to_merge(&summary));
    }

    #[test]
    fn ready_to_merge_scopes_github_merge_state_blockers_to_github() {
        let mut summary = base_summary();
        summary.identity = HostedReviewIdentity {
            provider: "gitlab".to_string(),
            host: "gitlab.com".to_string(),
            owner: "acme".to_string(),
            repo: "orca".to_string(),
            number: 42,
        };
        summary.merge_state_status = Some("BLOCKED".to_string());
        assert!(review_ready_to_merge(&summary));
    }
}
