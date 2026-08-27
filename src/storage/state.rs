//! State manager for `.calman-state.json` (version, uid_counter, last_modified).
//!
//! Layout per DESIGN.md §2.3.

use super::atomic_write;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata file name within a source location.
pub const STATE_FILE: &str = ".calman-state.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub uid_counter: u64,
    pub last_modified: DateTime<Utc>,
}

impl Default for State {
    fn default() -> Self {
        State {
            version: 1,
            uid_counter: 0,
            last_modified: Utc::now(),
        }
    }
}

/// Reads/writes `.calman-state.json` for a source location.
pub struct StateManager {
    path: PathBuf,
    state: State,
}

impl StateManager {
    pub fn open(location: &Path) -> Result<Self> {
        fs::create_dir_all(location)
            .with_context(|| format!("create storage dir {}", location.display()))?;
        let path = location.join(STATE_FILE);
        let state = if path.exists() {
            let content =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?
        } else {
            State::default()
        };
        Ok(StateManager { path, state })
    }

    /// Record a storage modification and persist.
    pub fn touch(&mut self) -> Result<()> {
        self.state.uid_counter += 1;
        self.state.last_modified = Utc::now();
        let json = serde_json::to_string(&self.state)?;
        atomic_write(&self.path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_default_and_roundtrips() {
        let dir = tempdir().unwrap();
        let mut m = StateManager::open(dir.path()).unwrap();
        assert_eq!(m.state.version, 1);
        assert_eq!(m.state.uid_counter, 0);
        m.touch().unwrap();
        drop(m);

        let m2 = StateManager::open(dir.path()).unwrap();
        assert_eq!(m2.state.uid_counter, 1);
    }
}
