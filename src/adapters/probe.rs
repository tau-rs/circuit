//! `DeliveryProbe` implemented by probing the host: `gh --version` for CLI
//! availability and `git remote -v` for a github.com remote. Detection failures
//! degrade to `false` (never errors).

use std::path::PathBuf;
use std::process::Command;

use crate::ports::DeliveryProbe;

/// Probes the real host environment, rooted at a working tree.
pub struct SystemDeliveryProbe {
    root: PathBuf,
}

impl SystemDeliveryProbe {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl DeliveryProbe for SystemDeliveryProbe {
    fn gh_available(&self) -> bool {
        Command::new("gh")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    fn has_github_remote(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["remote", "-v"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("github.com"))
            .unwrap_or(false)
    }
}
