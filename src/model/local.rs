//! Machine-local settings (`.circuit/local.toml`, gitignored). Holds the
//! worktree root override and the pure resolver for a session's worktree path (§7.2).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `.circuit/local.toml` — never committed (machine-specific paths). Absent file
/// deserializes to the all-`None` default via `Workspace::load_local`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    /// Root directory under which per-session worktrees are created. When unset,
    /// the default sibling `../<repo>-worktrees/` is used.
    #[serde(default)]
    pub worktrees_dir: Option<PathBuf>,
}

/// Resolve the worktree directory for a session. Precedence (§7.2):
/// 1. `env` (the `CIRCUIT_WORKTREES_DIR` value), 2. `local.worktrees_dir`,
/// 3. default sibling `<repo_root>/../<repo_name>-worktrees`.
///
/// In all cases the session id is appended as the final path component, so the
/// returned path is `<base>/<session_id>`.
pub fn resolve_worktree_dir(
    env: Option<&str>,
    local: &LocalConfig,
    repo_root: &Path,
    session_id: &str,
) -> PathBuf {
    let base: PathBuf = if let Some(e) = env.filter(|e| !e.is_empty()) {
        PathBuf::from(e)
    } else if let Some(d) = &local.worktrees_dir {
        d.clone()
    } else {
        let name = repo_root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_string());
        let parent = repo_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| repo_root.to_path_buf());
        parent.join(format!("{name}-worktrees"))
    };
    base.join(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_takes_precedence_over_everything() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(Some("/from/env"), &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/env/SID"));
    }

    #[test]
    fn empty_env_is_ignored() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(Some(""), &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/local/SID"));
    }

    #[test]
    fn local_config_used_when_no_env() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(None, &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/local/SID"));
    }

    #[test]
    fn default_is_sibling_worktrees_dir() {
        let got = resolve_worktree_dir(None, &LocalConfig::default(), Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/repos/circuit-worktrees/SID"));
    }

    #[test]
    fn local_config_round_trips_through_toml() {
        let c = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/tmp/wt")),
        };
        let parsed: LocalConfig = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn empty_toml_is_default() {
        let c: LocalConfig = toml::from_str("").unwrap();
        assert_eq!(c, LocalConfig::default());
    }
}
