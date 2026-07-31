//! User-defined topic threads: multi-dimensional subscriptions that
//! auto-aggregate matching chunks into a dedicated summary tree.
//!
//! A topic is defined by keywords (OR/AND), pinned source ids, and pinned
//! entity ids. During ingest, [`maybe_link_chunk_to_topics`] checks each
//! admitted chunk against every topic and enqueues an `AppendBuffer(Topic)`
//! job on a match — from there the normal seal → summary pipeline builds the
//! topic's timeline (highest-level summary = current state, lower levels =
//! history), ready to feed status reports.

pub mod ops;
mod schemas;
pub mod store;
mod types;

pub use ops::maybe_link_chunk_to_topics;
pub use schemas::{
    all_controller_schemas as all_topic_threads_controller_schemas,
    all_registered_controllers as all_topic_threads_registered_controllers,
};
