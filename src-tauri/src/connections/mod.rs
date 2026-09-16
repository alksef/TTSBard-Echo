pub mod client;
pub mod error;
pub mod manager;

/// Test-only SSE fixture server (roadmap 011, decision G).
#[cfg(test)]
pub(crate) mod sse_fixture;

pub use error::{ConnectionError, ConnectionErrorKind};
pub use manager::ConnectionManager;
