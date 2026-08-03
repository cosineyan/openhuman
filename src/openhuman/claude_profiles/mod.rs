//! Claude Code settings-profile registry.
//!
//! Lets users register paths to their Claude Code `settings.json.*` files, then
//! parses each file's `env` block to expose its model tiers (opus/sonnet/haiku
//! + default). Tasks reference a profile by id; at pickup the projects runner
//! launches `claude --settings <path> --model <model>`.
//!
//! SECURITY: only the four model-name env keys are ever read; auth tokens and
//! base URLs are never parsed, returned, or logged. See [`ops::parse_profile_models`].

pub mod ops;
pub mod schemas;
pub mod store;
pub mod types;

pub use ops::{parse_profile_models, resolve_model, resolve_path};
pub use schemas::{
    all_controller_schemas as all_claude_profiles_controller_schemas,
    all_registered_controllers as all_claude_profiles_registered_controllers,
};
pub use types::{ClaudeProfile, ProfileModels, ProfileWithModels};
