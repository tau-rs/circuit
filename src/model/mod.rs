pub mod config;
pub mod glossary;
pub mod local;
pub mod node;
pub mod spec;

use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Errors at the `.circuit/` persistence boundary.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Read and deserialize a TOML file.
pub(crate) fn load_toml<T: DeserializeOwned>(path: &Path) -> Result<T, ModelError> {
    let text = std::fs::read_to_string(path).map_err(|source| ModelError::Io {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| ModelError::Parse {
        path: path.display().to_string(),
        source,
    })
}

/// Serialize and write a TOML file, creating parent directories as needed.
pub(crate) fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), ModelError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ModelError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let text = toml::to_string_pretty(value)?;
    std::fs::write(path, text).map_err(|source| ModelError::Io {
        path: path.display().to_string(),
        source,
    })
}
