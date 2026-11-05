//! Lamina project operations. Each public operation maps to one MCP tool
//! and one CLI subcommand.

pub mod lock;
pub mod ops;
pub mod project;
pub mod reuse;
pub mod validate;

pub use lamina_model as model;
pub use lamina_model::error::{Error, Result};
pub use ops::*;
pub use project::{OpResult, Project, Touched, next_id};
