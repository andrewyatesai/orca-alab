//! Pi-compatible agent-kind detection, ported from `src/shared/pi-agent-kind.ts`.
//!
//! Both Pi and OMP (omp.sh) share the `PI_CODING_AGENT_DIR` env contract and
//! extension API but default their on-disk config dir to a different
//! `~/.<kind>/agent` path. The per-PTY overlay must know which agent a launch
//! command targets so it mirrors that agent's source dir with **no cross-agent
//! fallback** (otherwise switching agents silently shadows the other's user
//! extensions).

use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiAgentKind {
    Pi,
    Omp,
}

/// OMP's launch command (`TUI_AGENT_CONFIG.omp.launchCmd`).
const OMP_LAUNCH_CMD: &str = "omp";

// Boundaries carved so `~/bin/omp` / `./omp` match but `comp` / `pomp` / `omp`
// inside a larger word do not: a leading boundary excluding alnum/`_`/`-`/`.`/
// `/`/`\` (start-of-string or a shell separator), a trailing boundary allowing
// whitespace, end-of-string, shell separators, or argv flags, and an optional
// path prefix that must end in a slash.
const BOUNDARY_BEFORE: &str = r#"(?:^|[\s;&|('"`])"#;
const BOUNDARY_AFTER: &str = r#"(?:$|[\s;&|)'"`])"#;
const PATH_PREFIX: &str = r#"(?:[^\s;&|('"`]*[\\/])?"#;

fn omp_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // launchCmd may carry args ("hermes --tui"); only the first token is the
        // binary name. Escape it so metacharacters match literally.
        let binary = OMP_LAUNCH_CMD.split_whitespace().next().unwrap_or(OMP_LAUNCH_CMD);
        let escaped = regex::escape(binary);
        Regex::new(&format!(
            "(?i){BOUNDARY_BEFORE}{PATH_PREFIX}{escaped}(?:\\.cmd|\\.exe|\\.sh)?{BOUNDARY_AFTER}"
        ))
        .unwrap()
    })
}

/// Identify the Pi-compatible agent kind a launch command targets.
///
/// Returns `Omp` when the command launches OMP (`omp` / `omp.sh`), otherwise
/// defaults to `Pi`. Defaulting to `Pi` preserves prior behaviour for the
/// non-launch case (bare shells that may later invoke `pi`). Never cross-falls
/// back between the two agents' config dirs.
pub fn detect_pi_agent_kind_from_command(command: Option<&str>) -> PiAgentKind {
    match command {
        Some(command) if omp_regex().is_match(command) => PiAgentKind::Omp,
        _ => PiAgentKind::Pi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PiAgentKind::{Omp, Pi};

    #[test]
    fn returns_pi_for_undefined_or_empty_commands() {
        assert_eq!(detect_pi_agent_kind_from_command(None), Pi);
        assert_eq!(detect_pi_agent_kind_from_command(Some("")), Pi);
    }

    #[test]
    fn returns_pi_for_a_bare_pi_launch() {
        assert_eq!(detect_pi_agent_kind_from_command(Some("pi")), Pi);
        assert_eq!(detect_pi_agent_kind_from_command(Some("pi --resume")), Pi);
    }

    #[test]
    fn returns_omp_for_a_bare_omp_launch() {
        assert_eq!(detect_pi_agent_kind_from_command(Some("omp")), Omp);
        assert_eq!(detect_pi_agent_kind_from_command(Some("omp -v")), Omp);
        assert_eq!(detect_pi_agent_kind_from_command(Some("omp.sh")), Omp);
    }

    #[test]
    fn returns_omp_for_omp_launched_via_an_absolute_path() {
        assert_eq!(detect_pi_agent_kind_from_command(Some("/usr/local/bin/omp")), Omp);
        assert_eq!(detect_pi_agent_kind_from_command(Some("~/bin/omp.sh")), Omp);
    }

    #[test]
    fn returns_pi_for_pi_launched_via_an_absolute_path() {
        assert_eq!(detect_pi_agent_kind_from_command(Some("/usr/local/bin/pi")), Pi);
    }

    #[test]
    fn does_not_confuse_pi_with_substrings_like_pip_mpi_python() {
        // Regression guard for the word-boundary regex — without boundary
        // protection any command containing "pi" would classify as a Pi launch.
        assert_eq!(detect_pi_agent_kind_from_command(Some("pip install foo")), Pi);
        assert_eq!(detect_pi_agent_kind_from_command(Some("mpirun -n 4 ./app")), Pi);
        assert_eq!(detect_pi_agent_kind_from_command(Some("python3 script.py")), Pi);
    }

    #[test]
    fn does_not_confuse_omp_with_substrings_like_comp_pomp() {
        // Regression guard for the OMP boundary; falls back to Pi on no match.
        assert_eq!(detect_pi_agent_kind_from_command(Some("compile this")), Pi);
        assert_eq!(detect_pi_agent_kind_from_command(Some("pomp.exe")), Pi);
    }

    #[test]
    fn matches_case_insensitively_on_windows_style_executables() {
        assert_eq!(detect_pi_agent_kind_from_command(Some("OMP.EXE")), Omp);
        assert_eq!(detect_pi_agent_kind_from_command(Some("PI.CMD")), Pi);
    }
}
