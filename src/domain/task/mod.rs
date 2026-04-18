//! Domain aggregate for work items.
//!
//! The `task` module is split into focused sub-modules:
//!
//! - [`types`] — `TaskStatus` state machine, `TaskPriority`, `MessageType`
//! - [`draft`] — `StructuredTaskDraft` with validation
//! - [`entity`] — `Task` aggregate and `TaskStats`
//!
//! All public symbols are re-exported here so call sites only need
//! `use crate::domain::task::Task` regardless of which sub-module owns it.

mod draft;
mod entity;
mod types;

pub use draft::StructuredTaskDraft;
pub use entity::{Task, TaskStats};
pub use types::{MessageType, TaskPriority, TaskStatus};
