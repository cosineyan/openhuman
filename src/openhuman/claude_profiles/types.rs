//! Domain types for Claude Code settings profiles.
//!
//! A "profile" registers a path to one of the user's Claude Code
//! `settings.json` files (e.g. `~/.claude/settings.json.hyperspace`). The
//! registry persists only `{id, name, path}`; the available models are parsed
//! on demand from the settings.json `env` block so the file on disk stays the
//! single source of truth (rotating the auth token or model mapping needs no
//! re-registration).
//!
//! SECURITY: parsed models NEVER include the auth token or base URL — see
//! [`super::ops::parse_profile_models`]. No type in this module carries a
//! secret field, so nothing secret can leak over RPC.

use serde::{Deserialize, Serialize};

/// A registered Claude Code settings profile. Only these three fields are
/// persisted to the registry JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeProfile {
    /// Stable UUID v4 identifier (referenced by tasks via `settings_profile`).
    pub id: String,
    /// User-facing label, e.g. "Hyperspace" / "Local Qwen".
    pub name: String,
    /// Absolute path to a Claude Code `settings.json.*` file.
    pub path: String,
}

/// The model tiers parsed from a settings.json `env` block. NEVER contains
/// auth tokens or base URLs — only the four model-name keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileModels {
    /// `env.ANTHROPIC_DEFAULT_OPUS_MODEL`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opus: Option<String>,
    /// `env.ANTHROPIC_DEFAULT_SONNET_MODEL`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sonnet: Option<String>,
    /// `env.ANTHROPIC_DEFAULT_HAIKU_MODEL`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub haiku: Option<String>,
    /// `env.ANTHROPIC_MODEL` (the profile's default model)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl ProfileModels {
    /// True when at least one tier was parsed from the file.
    pub fn any(&self) -> bool {
        self.opus.is_some()
            || self.sonnet.is_some()
            || self.haiku.is_some()
            || self.default.is_some()
    }
}

/// A profile plus its parsed models and a readability flag, returned by the
/// list/get/add RPCs. Contains NO secret fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileWithModels {
    pub profile: ClaudeProfile,
    pub models: ProfileModels,
    /// False when the settings.json at `profile.path` is missing/unreadable at
    /// read time (the UI surfaces this as a warning).
    pub readable: bool,
}

/// Input for `add_profile`.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProfileInput {
    pub name: String,
    pub path: String,
}

/// One step on the global fallback ladder: a (profile, tier) pair. Order in the
/// ladder `Vec` is the fallback order. `tier` ∈ opus/sonnet/haiku/default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderStep {
    pub profile_id: String,
    pub tier: String,
}

/// A ladder step resolved for display / execution: carries the profile name,
/// the concrete model the tier maps to, and readability. NO token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LadderStepResolved {
    pub profile_id: String,
    pub profile_name: String,
    pub tier: String,
    /// Concrete model the tier resolves to (may be None if unreadable/missing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub readable: bool,
}

/// Global default fallback policy for tasks that have NO profile of their own.
/// When `enabled`, such tasks start at `start_profile`/`start_tier` and fall
/// back along the ladder in `direction` toward `end`. Never carries a token.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalFallback {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_profile: Option<String>,
    /// opus/sonnet/haiku/default
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_tier: Option<String>,
    /// "up" / "down"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Terminus "<profile_id>:<tier>", or None = walk to ladder boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

/// The persisted registry container (`claude_profiles.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRegistry {
    #[serde(default)]
    pub profiles: Vec<ClaudeProfile>,
    /// Global fallback ladder (ordered). Empty = auto-prefill from `profiles`.
    #[serde(default)]
    pub ladder: Vec<LadderStep>,
    /// Global default fallback for tasks without their own profile.
    #[serde(default)]
    pub global_fallback: GlobalFallback,
}
