//! Test harness for MCP tool integration tests.
//!
//! Builds a `ServerState` from fixture files in a temp dir, dispatches JSON-RPC
//! requests via `dispatch_jsonrpc()` directly (no subprocess, no HTTP).

pub mod fixtures;

use codescope_server::mcp::dispatch_jsonrpc;
use codescope_server::tokenizer::BytesEstimateTokenizer;
use codescope_server::types::{ServerState, SessionState};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use tempfile::TempDir;

pub struct TestHarness {
    pub state: Arc<RwLock<ServerState>>,
    pub session: Option<SessionState>,
    _temp_dir: TempDir,
}

impl TestHarness {
    /// Create a harness from a named fixture directory.
    /// Copies fixture files to a temp dir, runs `git init` + initial commit, then scans.
    pub fn from_fixture(name: &str) -> Self {
        let fixture_src =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        assert!(fixture_src.exists(), "Fixture '{name}' not found at {}", fixture_src.display());

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let root = temp_dir.path();

        // Copy fixture files to temp dir
        fixtures::copy_dir_recursive(&fixture_src, root);

        // Git init + initial commit so git operations work
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git init failed");
        assert!(status.success(), "git init failed");

        let status = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git add failed");
        assert!(status.success(), "git add failed");

        let status = std::process::Command::new("git")
            .args([
                "-c", "user.email=test@test.com",
                "-c", "user.name=Test",
                "commit", "-m", "Initial commit",
            ])
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("git commit failed");
        assert!(status.success(), "git commit failed");

        // Scan the fixture project
        let tok: Arc<dyn codescope_server::tokenizer::Tokenizer> =
            Arc::new(BytesEstimateTokenizer);
        let repo_state = codescope_server::scan_repo("test", root, &tok);

        let mut repos = BTreeMap::new();
        repos.insert("test".to_string(), repo_state);

        let server_state = ServerState {
            repos,
            default_repo: Some("test".to_string()),
            cross_repo_edges: vec![],
            tokenizer: tok,
            #[cfg(feature = "semantic")]
            semantic_enabled: false,
            #[cfg(feature = "semantic")]
            semantic_model: None,
        };

        TestHarness {
            state: Arc::new(RwLock::new(server_state)),
            session: Some(SessionState::new()),
            _temp_dir: temp_dir,
        }
    }

    /// Send a JSON-RPC request and return the response.
    pub fn dispatch(&mut self, msg: Value) -> Option<Value> {
        dispatch_jsonrpc(&self.state, &msg, &mut self.session)
    }

    /// Call an MCP tool by name with the given arguments. Returns (text, is_error).
    pub fn call_tool(&mut self, tool: &str, args: Value) -> (String, bool) {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": args
            }
        });
        let resp = self.dispatch(msg).expect("Expected response for tools/call");
        let result = &resp["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or("").to_string();
        let is_error = text.starts_with("\u{26a0} Error:");
        (text, is_error)
    }

    /// Send an initialize request and return the response.
    pub fn initialize(&mut self) -> Value {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0.1.0" }
            }
        });
        self.dispatch(msg).expect("Expected initialize response")
    }
}
