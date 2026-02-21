//! CodeScope Server — unified facade over core, MCP, and HTTP crates.
//!
//! Re-exports all functionality so existing consumers (src-tauri, main.rs) keep working
//! with unchanged import paths.

// Re-export everything from core
pub use codescope_core::*;

// Re-export transport crates under their original module names
pub use codescope_mcp as mcp;
pub use codescope_mcp::http as mcp_http;
pub use codescope_mcp::auth;
pub use codescope_http::api;
