pub mod types;
pub mod helpers;
pub mod schema;
pub mod indexing;
pub mod lessons;
pub mod queries;
pub mod git_notes;
pub mod embeddings;
pub mod mcp_impl;
pub mod sqlite_backend;

pub use types::*;
pub use helpers::*;
pub use sqlite_backend::SqliteBackend;
