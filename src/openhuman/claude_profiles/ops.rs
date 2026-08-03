//! Business logic for Claude Code settings profiles.
//!
//! Parsing (`parse_profile_models`) is the security-critical part: it reads a
//! settings.json and extracts ONLY the four whitelisted model-name keys from
//! the `env` block. It NEVER reads, returns, or logs `ANTHROPIC_AUTH_TOKEN`,
//! `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, or the `env` map wholesale.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use uuid::Uuid;

use crate::openhuman::config::Config;
use crate::openhuman::inference::provider::claude_code::workspace_dir_from_config;

use super::store;
use super::types::{
    ClaudeProfile, CreateProfileInput, GlobalFallback, LadderStep, LadderStepResolved,
    ProfileModels, ProfileRegistry, ProfileWithModels, ThrottleLimit,
};

/// Tiers in canonical order, used for auto-prefill and iteration.
const TIER_ORDER: [&str; 4] = ["opus", "sonnet", "haiku", "default"];

/// Pull the concrete model a tier maps to from parsed models.
fn tier_model(models: &ProfileModels, tier: &str) -> Option<String> {
    match tier {
        "opus" => models.opus.clone(),
        "sonnet" => models.sonnet.clone(),
        "haiku" => models.haiku.clone(),
        "default" => models.default.clone(),
        _ => None,
    }
}

/// Minimal deserialization shape: we only ever touch the `env` map, and within
/// it only four keys by name. Everything else in the settings.json is ignored.
#[derive(Debug, Deserialize)]
struct SettingsEnvShape {
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

/// Parse ONLY the model-name env keys from a settings.json at `path`.
///
/// SECURITY: reads the four whitelisted keys by name and nothing else. Never
/// logs the file contents or the `env` map — only which tiers were found
/// (booleans / the model-name values, which are not secret).
///
/// A missing/unreadable/corrupt file yields [`ProfileModels::default`] (all
/// `None`) — never errors.
pub fn parse_profile_models(path: &Path) -> ProfileModels {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::debug!(
                "[claude-profiles] settings unreadable at {} ({e}); no models",
                path.display()
            );
            return ProfileModels::default();
        }
    };
    let parsed: SettingsEnvShape = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "[claude-profiles] settings at {} not valid JSON ({e}); no models",
                path.display()
            );
            return ProfileModels::default();
        }
    };
    // Pull ONLY the four whitelisted keys. Do not iterate/log the env map.
    let models = ProfileModels {
        opus: parsed.env.get("ANTHROPIC_DEFAULT_OPUS_MODEL").cloned(),
        sonnet: parsed.env.get("ANTHROPIC_DEFAULT_SONNET_MODEL").cloned(),
        haiku: parsed.env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").cloned(),
        default: parsed.env.get("ANTHROPIC_MODEL").cloned(),
    };
    log::debug!(
        "[claude-profiles] parsed models from {} (opus={} sonnet={} haiku={} default={})",
        path.display(),
        models.opus.is_some(),
        models.sonnet.is_some(),
        models.haiku.is_some(),
        models.default.is_some(),
    );
    models
}

/// Build a [`ProfileWithModels`] view for `profile`: parse its models and set
/// `readable` from whether the path exists and yielded at least one tier.
fn enrich(profile: ClaudeProfile) -> ProfileWithModels {
    let path = PathBuf::from(&profile.path);
    let readable = path.is_file();
    let models = parse_profile_models(&path);
    ProfileWithModels {
        profile,
        models,
        readable,
    }
}

/// List all registered profiles with their parsed models.
pub fn list_profiles(config: &Config) -> Vec<ProfileWithModels> {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace)
        .profiles
        .into_iter()
        .map(enrich)
        .collect()
}

/// Parse the models at an arbitrary path WITHOUT registering it. Powers the
/// "live preview as you type the path" in the settings UI. Returns the parsed
/// models and whether the file was readable. Never returns secrets.
pub fn preview_models(path: &str) -> (ProfileModels, bool) {
    let p = PathBuf::from(path.trim());
    let readable = p.is_file();
    (parse_profile_models(&p), readable)
}

/// Get one profile (with parsed models) by id.
pub fn get_profile(config: &Config, id: &str) -> Option<ProfileWithModels> {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace)
        .profiles
        .into_iter()
        .find(|p| p.id == id)
        .map(enrich)
}

/// Add a new profile. Stores `{id, name, path}` even if the file is not
/// currently readable (returns `readable:false` so the UI can warn); the source
/// file may be created later.
pub fn add_profile(
    config: &Config,
    input: CreateProfileInput,
) -> Result<ProfileWithModels, String> {
    let name = input.name.trim().to_string();
    let path = input.path.trim().to_string();
    if name.is_empty() {
        return Err("profile name is required".into());
    }
    if path.is_empty() {
        return Err("profile path is required".into());
    }
    let workspace = workspace_dir_from_config(config);
    let mut registry = store::load(&workspace);

    // Reject duplicate paths (same file registered twice is almost always a
    // mistake) — but allow re-adding under a different path.
    if registry.profiles.iter().any(|p| p.path == path) {
        return Err(format!("a profile already points at {path}"));
    }

    let profile = ClaudeProfile {
        id: Uuid::new_v4().to_string(),
        name,
        path,
    };
    registry.profiles.push(profile.clone());
    store::save(&workspace, &registry).map_err(|e| format!("failed to save registry: {e}"))?;
    Ok(enrich(profile))
}

/// Remove a profile by id. Returns whether a row was removed.
pub fn remove_profile(config: &Config, id: &str) -> Result<bool, String> {
    let workspace = workspace_dir_from_config(config);
    let mut registry = store::load(&workspace);
    let before = registry.profiles.len();
    registry.profiles.retain(|p| p.id != id);
    let removed = registry.profiles.len() != before;
    if removed {
        store::save(&workspace, &registry).map_err(|e| format!("failed to save registry: {e}"))?;
    }
    Ok(removed)
}

// --- Resolution helpers used by the projects pickup path (bus.rs) ----------

/// Resolve a profile id to its settings.json path. Returns `None` for an
/// unknown id (caller falls back to legacy behavior).
pub fn resolve_path(config: &Config, profile_id: &str) -> Option<PathBuf> {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace)
        .profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .map(|p| PathBuf::from(p.path))
}

/// Resolve a requested model against a profile's parsed tiers.
///
/// - A tier alias (`opus`/`sonnet`/`haiku`/`default`) maps to that tier's
///   concrete model string when present.
/// - Anything else is treated as an already-concrete model and passed through
///   unchanged.
/// - Falls back to the profile's `default` tier, then `None`.
pub fn resolve_model(models: &ProfileModels, requested: &str) -> Option<String> {
    match requested.trim().to_ascii_lowercase().as_str() {
        "opus" => models.opus.clone().or_else(|| models.default.clone()),
        "sonnet" => models.sonnet.clone().or_else(|| models.default.clone()),
        "haiku" => models.haiku.clone().or_else(|| models.default.clone()),
        "default" | "" => models.default.clone(),
        // Not an alias → treat as a concrete model id, pass through verbatim.
        _ => Some(requested.to_string()),
    }
}

// --- Fallback ladder --------------------------------------------------------

/// Auto-prefill a ladder from the registered profiles: every available tier of
/// every profile, in registration order then TIER_ORDER. Used when the stored
/// ladder is empty.
fn autofill_steps(profiles: &[ClaudeProfile]) -> Vec<LadderStep> {
    let mut steps = Vec::new();
    for p in profiles {
        let models = parse_profile_models(&PathBuf::from(&p.path));
        for tier in TIER_ORDER {
            if tier_model(&models, tier).is_some() {
                steps.push(LadderStep {
                    profile_id: p.id.clone(),
                    tier: tier.to_string(),
                });
            }
        }
    }
    steps
}

/// Resolve a list of ladder steps into display/execution form (profile name +
/// concrete model + readability). Steps whose profile no longer exists are
/// dropped (ladder/profile drift).
fn resolve_steps(profiles: &[ClaudeProfile], steps: &[LadderStep]) -> Vec<LadderStepResolved> {
    steps
        .iter()
        .filter_map(|s| {
            let profile = profiles.iter().find(|p| p.id == s.profile_id)?;
            let path = PathBuf::from(&profile.path);
            let readable = path.is_file();
            let model = tier_model(&parse_profile_models(&path), &s.tier);
            Some(LadderStepResolved {
                profile_id: s.profile_id.clone(),
                profile_name: profile.name.clone(),
                tier: s.tier.clone(),
                model,
                readable,
            })
        })
        .collect()
}

/// Get the fallback ladder (resolved). Empty stored ladder → auto-prefill.
pub fn get_ladder(config: &Config) -> Vec<LadderStepResolved> {
    let workspace = workspace_dir_from_config(config);
    let registry = store::load(&workspace);
    let steps = if registry.ladder.is_empty() {
        autofill_steps(&registry.profiles)
    } else {
        registry.ladder.clone()
    };
    resolve_steps(&registry.profiles, &steps)
}

/// Persist a new ladder order (UI reorder / add / remove). Overwrites wholesale.
pub fn set_ladder(config: &Config, steps: Vec<LadderStep>) -> Result<(), String> {
    let workspace = workspace_dir_from_config(config);
    let mut registry = store::load(&workspace);
    registry.ladder = steps;
    store::save(&workspace, &registry).map_err(|e| format!("failed to save ladder: {e}"))
}

/// Get the global default fallback policy (for tasks without their own profile).
pub fn get_global_fallback(config: &Config) -> GlobalFallback {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace).global_fallback
}

/// Persist the global default fallback policy. Overwrites wholesale.
pub fn set_global_fallback(config: &Config, gf: GlobalFallback) -> Result<(), String> {
    let workspace = workspace_dir_from_config(config);
    let mut registry = store::load(&workspace);
    registry.global_fallback = gf;
    store::save(&workspace, &registry).map_err(|e| format!("failed to save global fallback: {e}"))
}

// --- Per-(profile,tier) concurrency throttles ------------------------------

/// Get all configured throttle limits.
pub fn get_throttles(config: &Config) -> Vec<ThrottleLimit> {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace).throttles
}

/// Persist throttle limits (overwrites wholesale). Rejects limit==0, dedupes
/// on (profile_id, tier) keeping the last occurrence.
pub fn set_throttles(config: &Config, limits: Vec<ThrottleLimit>) -> Result<(), String> {
    let mut deduped: Vec<ThrottleLimit> = Vec::new();
    for l in limits {
        if l.limit == 0 {
            return Err(format!(
                "throttle limit for {}:{} must be >= 1 (0 would freeze the bucket)",
                l.profile_id, l.tier
            ));
        }
        // last wins on duplicate (profile_id, tier)
        if let Some(existing) = deduped
            .iter_mut()
            .find(|e| e.profile_id == l.profile_id && e.tier == l.tier)
        {
            existing.limit = l.limit;
        } else {
            deduped.push(l);
        }
    }
    let workspace = workspace_dir_from_config(config);
    let mut registry = store::load(&workspace);
    registry.throttles = deduped;
    store::save(&workspace, &registry).map_err(|e| format!("failed to save throttles: {e}"))
}

/// The concurrency limit for a (profile,tier) pair, or `None` if unlimited.
pub fn throttle_limit_for(config: &Config, profile_id: &str, tier: &str) -> Option<u32> {
    let workspace = workspace_dir_from_config(config);
    store::load(&workspace)
        .throttles
        .into_iter()
        .find(|t| t.profile_id == profile_id && t.tier == tier)
        .map(|t| t.limit)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackCandidate {
    pub profile_id: String,
    pub tier: String,
    pub settings_path: PathBuf,
    pub model: String,
}

/// Build the ordered candidate chain for a task, walking the ladder from the
/// `start` step toward `end` in `direction` ("up" = earlier steps, "down" =
/// later steps). Inclusive of both endpoints. Unreadable / unresolvable steps
/// are skipped (logged). Returns `[]` if `start` isn't on the ladder.
pub fn resolve_fallback_chain(
    config: &Config,
    start_profile: &str,
    start_tier: &str,
    direction: &str,
    end_profile: &str,
    end_tier: &str,
) -> Vec<FallbackCandidate> {
    let workspace = workspace_dir_from_config(config);
    let registry = store::load(&workspace);
    let steps = if registry.ladder.is_empty() {
        autofill_steps(&registry.profiles)
    } else {
        registry.ladder.clone()
    };

    let find = |prof: &str, tier: &str| -> Option<usize> {
        steps
            .iter()
            .position(|s| s.profile_id == prof && s.tier == tier)
    };
    let Some(start_idx) = find(start_profile, start_tier) else {
        log::warn!("[claude-profiles] fallback start ({start_profile}:{start_tier}) not on ladder");
        return Vec::new();
    };
    // End index: if not found, walk to the ladder boundary in the direction.
    let end_idx = find(end_profile, end_tier);

    let down = direction.eq_ignore_ascii_case("down");
    let ordered_indices = ladder_slice_indices(steps.len(), start_idx, end_idx, down);

    ordered_indices
        .into_iter()
        .filter_map(|i| {
            let step = &steps[i];
            let profile = registry.profiles.iter().find(|p| p.id == step.profile_id)?;
            let path = PathBuf::from(&profile.path);
            let model = tier_model(&parse_profile_models(&path), &step.tier)?;
            Some(FallbackCandidate {
                profile_id: step.profile_id.clone(),
                tier: step.tier.clone(),
                settings_path: path,
                model,
            })
        })
        .collect()
}

/// Pure ladder walk: given ladder length, a start index, an optional end index,
/// and direction, return the inclusive ordered index list from start toward end
/// (or to the boundary when end is None/unfound). Extracted for unit testing.
fn ladder_slice_indices(
    len: usize,
    start_idx: usize,
    end_idx: Option<usize>,
    down: bool,
) -> Vec<usize> {
    if len == 0 || start_idx >= len {
        return Vec::new();
    }
    if down {
        let stop = end_idx.unwrap_or(len - 1).clamp(start_idx, len - 1);
        (start_idx..=stop).collect()
    } else {
        let stop = end_idx.unwrap_or(0).min(start_idx);
        (stop..=start_idx).rev().collect()
    }
}

/// Registry helper for tests / callers that already hold a workspace dir.
#[cfg(test)]
pub fn list_from_registry(registry: ProfileRegistry) -> Vec<ProfileWithModels> {
    registry.profiles.into_iter().map(enrich).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_settings(dir: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn parse_extracts_only_whitelisted_keys_never_token() {
        let dir = std::env::temp_dir().join("oh_cp_parse_test");
        let _ = std::fs::remove_dir_all(&dir);
        let body = r#"{
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "sk-secret-should-never-appear",
                "ANTHROPIC_BASE_URL": "http://localhost:6655/anthropic/",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-latest",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-latest",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-latest",
                "ANTHROPIC_MODEL": "claude-opus-latest"
            }
        }"#;
        let path = write_settings(&dir, "settings.json.hyperspace", body);
        let m = parse_profile_models(&path);
        assert_eq!(m.opus.as_deref(), Some("claude-opus-latest"));
        assert_eq!(m.sonnet.as_deref(), Some("claude-sonnet-latest"));
        assert_eq!(m.haiku.as_deref(), Some("claude-haiku-latest"));
        assert_eq!(m.default.as_deref(), Some("claude-opus-latest"));

        // Serialized view must NOT contain the token or base url anywhere.
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("sk-secret"), "token leaked into models");
        assert!(!json.contains("localhost"), "base url leaked into models");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_missing_file_returns_default() {
        let m = parse_profile_models(Path::new("/no/such/settings.json"));
        assert!(!m.any());
    }

    #[test]
    fn parse_corrupt_returns_default() {
        let dir = std::env::temp_dir().join("oh_cp_parse_corrupt_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = write_settings(&dir, "settings.json", "{not json");
        assert!(!parse_profile_models(&path).any());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_model_alias_and_passthrough() {
        let models = ProfileModels {
            opus: Some("claude-opus-latest".into()),
            sonnet: Some("claude-sonnet-latest".into()),
            haiku: None,
            default: Some("claude-opus-latest".into()),
        };
        assert_eq!(
            resolve_model(&models, "sonnet").as_deref(),
            Some("claude-sonnet-latest")
        );
        // Missing tier falls back to default.
        assert_eq!(
            resolve_model(&models, "haiku").as_deref(),
            Some("claude-opus-latest")
        );
        // Concrete model passes through unchanged.
        assert_eq!(
            resolve_model(&models, "Qwen3.5-27B-OptiQ-4bit").as_deref(),
            Some("Qwen3.5-27B-OptiQ-4bit")
        );
        // default alias.
        assert_eq!(
            resolve_model(&models, "default").as_deref(),
            Some("claude-opus-latest")
        );
    }

    #[test]
    fn enrich_marks_unreadable() {
        let profile = ClaudeProfile {
            id: "x".into(),
            name: "Gone".into(),
            path: "/no/such/file.json".into(),
        };
        let view = enrich(profile);
        assert!(!view.readable);
        assert!(!view.models.any());
    }

    #[test]
    fn ladder_slice_down_from_start_to_end() {
        // len 5, start 1, end 3, down → [1,2,3]
        assert_eq!(ladder_slice_indices(5, 1, Some(3), true), vec![1, 2, 3]);
    }

    #[test]
    fn ladder_slice_down_to_boundary_when_end_none() {
        // start 2, no end, down → [2,3,4]
        assert_eq!(ladder_slice_indices(5, 2, None, true), vec![2, 3, 4]);
    }

    #[test]
    fn ladder_slice_up_from_start_to_end() {
        // start 3, end 1, up → [3,2,1]
        assert_eq!(ladder_slice_indices(5, 3, Some(1), false), vec![3, 2, 1]);
    }

    #[test]
    fn ladder_slice_up_to_boundary_when_end_none() {
        // start 2, no end, up → [2,1,0]
        assert_eq!(ladder_slice_indices(5, 2, None, false), vec![2, 1, 0]);
    }

    #[test]
    fn ladder_slice_single_when_start_equals_end() {
        assert_eq!(ladder_slice_indices(5, 2, Some(2), true), vec![2]);
        assert_eq!(ladder_slice_indices(5, 2, Some(2), false), vec![2]);
    }

    #[test]
    fn ladder_slice_end_wrong_side_clamps_to_start() {
        // down but end is before start → clamp to just [start]
        assert_eq!(ladder_slice_indices(5, 3, Some(1), true), vec![3]);
        // up but end is after start → clamp to just [start]
        assert_eq!(ladder_slice_indices(5, 1, Some(3), false), vec![1]);
    }

    #[test]
    fn ladder_slice_empty_or_oob() {
        assert!(ladder_slice_indices(0, 0, None, true).is_empty());
        assert!(ladder_slice_indices(3, 5, None, true).is_empty());
    }

    #[test]
    fn autofill_skips_unreadable_profiles() {
        // Profile pointing at a missing file yields no steps.
        let profiles = vec![ClaudeProfile {
            id: "p1".into(),
            name: "Gone".into(),
            path: "/no/such/settings.json".into(),
        }];
        assert!(autofill_steps(&profiles).is_empty());
    }

    #[test]
    fn autofill_orders_tiers_per_profile() {
        let dir = std::env::temp_dir().join("oh_cp_autofill_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = write_settings(
            &dir,
            "settings.json.x",
            r#"{"env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"o","ANTHROPIC_DEFAULT_HAIKU_MODEL":"h"}}"#,
        );
        let profiles = vec![ClaudeProfile {
            id: "p1".into(),
            name: "X".into(),
            path: path.to_string_lossy().to_string(),
        }];
        let steps = autofill_steps(&profiles);
        // opus present, sonnet absent, haiku present, default absent → [opus, haiku]
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].tier, "opus");
        assert_eq!(steps[1].tier, "haiku");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
