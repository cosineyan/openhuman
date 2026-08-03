//! Persistence for the Claude Code settings-profile registry.
//!
//! A small JSON file `claude_profiles.json` under the user's `workspace_dir`,
//! mirroring the `claude_code_settings.json` precedent. Only `{id, name, path}`
//! entries are stored — never any parsed model or secret. A missing/corrupt
//! file yields an empty registry (fail safe, never fail open).

use std::path::{Path, PathBuf};

use super::types::ProfileRegistry;

/// File name (under `workspace_dir`) holding the profile registry.
const REGISTRY_FILE: &str = "claude_profiles.json";

fn registry_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(REGISTRY_FILE)
}

/// Load the registry from `workspace_dir`. Missing or corrupt file → empty
/// registry (fail safe).
pub fn load(workspace_dir: &Path) -> ProfileRegistry {
    let path = registry_path(workspace_dir);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!(
                "[claude-profiles][store] corrupt {} ({e}); using empty registry",
                path.display()
            );
            ProfileRegistry::default()
        }),
        Err(e) => {
            log::debug!(
                "[claude-profiles][store] no registry at {} ({e}); using empty",
                path.display()
            );
            ProfileRegistry::default()
        }
    }
}

/// Persist `registry` to `workspace_dir`, creating the directory if needed.
pub fn save(workspace_dir: &Path, registry: &ProfileRegistry) -> std::io::Result<()> {
    let path = registry_path(workspace_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(registry).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    log::debug!(
        "[claude-profiles][store] saved {} profiles → {}",
        registry.profiles.len(),
        path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::claude_profiles::types::ClaudeProfile;

    #[test]
    fn load_missing_returns_empty() {
        let dir = std::env::temp_dir().join("oh_cp_store_missing_test");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).profiles.is_empty());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = std::env::temp_dir().join("oh_cp_store_roundtrip_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let reg = ProfileRegistry {
            profiles: vec![ClaudeProfile {
                id: "abc".into(),
                name: "Hyperspace".into(),
                path: "/tmp/settings.json.hyperspace".into(),
            }],
        };
        save(&dir, &reg).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].name, "Hyperspace");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_corrupt_returns_empty() {
        let dir = std::env::temp_dir().join("oh_cp_store_corrupt_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(registry_path(&dir), b"{not json").unwrap();
        assert!(load(&dir).profiles.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
