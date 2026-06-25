//! M365 integration domain — bundled m365-cli Python tool wrapper.
//!
//! Provides RPC controllers for querying and refreshing Microsoft 365 tokens
//! (graph, rest, teams) extracted from Chrome's running Outlook/Teams pages.
//! Token file is stored at `<workspace_dir>/m365/tokens.json`.

pub mod ops;
mod schemas;

pub use schemas::{
    all_controller_schemas as all_m365_controller_schemas,
    all_registered_controllers as all_m365_registered_controllers,
};
