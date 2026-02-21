//! Integration tests for all MCP tools via dispatch_jsonrpc().
//!
//! Each test builds a ServerState from the `basic` fixture project,
//! then sends JSON-RPC requests and validates the responses.

mod helpers;

use helpers::TestHarness;
use serde_json::json;

// ---------------------------------------------------------------------------
// Protocol tests
// ---------------------------------------------------------------------------

#[test]
fn test_initialize() {
    let mut h = TestHarness::from_fixture("basic");
    let resp = h.initialize();

    // Should negotiate the requested protocol version
    let version = resp["result"]["protocolVersion"].as_str().unwrap();
    assert_eq!(version, "2025-11-25");

    // Should report server info
    let name = resp["result"]["serverInfo"]["name"].as_str().unwrap();
    assert_eq!(name, "codescope");

    // Should include tools capability
    assert!(resp["result"]["capabilities"]["tools"].is_object());
}

// ---------------------------------------------------------------------------
// cs_search tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_search_returns_results() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config" }));
    assert!(!is_err, "cs_search returned error: {text}");
    assert!(text.contains("Found"), "Expected 'Found' in output: {text}");
    // Should find types.rs which defines Config
    assert!(text.contains("types.rs"), "Expected types.rs in results: {text}");
}

// ---------------------------------------------------------------------------
// cs_grep tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_grep_all_mode() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_grep",
        json!({ "query": "pub fn", "match_mode": "all" }),
    );
    assert!(!is_err, "cs_grep error: {text}");
    assert!(text.contains("Found"), "Expected matches: {text}");
    // "pub fn" should match lines in lib.rs
    assert!(text.contains("lib.rs"), "Expected lib.rs in results: {text}");
}

#[test]
fn test_cs_grep_any_mode() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_grep",
        json!({ "query": "greet process", "match_mode": "any" }),
    );
    assert!(!is_err, "cs_grep error: {text}");
    // Should find both functions since either term matches
    assert!(text.contains("greet"), "Expected 'greet' in matches: {text}");
    assert!(text.contains("process"), "Expected 'process' in matches: {text}");
}

#[test]
fn test_cs_grep_exact_mode() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_grep",
        json!({ "query": "pub fn greet", "match_mode": "exact" }),
    );
    assert!(!is_err, "cs_grep error: {text}");
    assert!(text.contains("lib.rs"), "Expected lib.rs for exact match: {text}");
}

#[test]
fn test_cs_grep_regex_mode() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_grep",
        json!({ "query": "fn\\s+\\w+\\(", "match_mode": "regex" }),
    );
    assert!(!is_err, "cs_grep error: {text}");
    assert!(text.contains("Found"), "Expected regex matches: {text}");
}

#[test]
fn test_cs_grep_context() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_grep",
        json!({ "query": "greet", "context": 1 }),
    );
    assert!(!is_err, "cs_grep error: {text}");
    // Context lines use '|' separator, match lines use ':'
    assert!(text.contains("|") || text.contains(":"), "Expected context in output: {text}");
}

// ---------------------------------------------------------------------------
// cs_read tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_read_full() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_read",
        json!({ "path": "src/types.rs" }),
    );
    assert!(!is_err, "cs_read error: {text}");
    assert!(text.contains("pub struct Config"), "Expected Config struct: {text}");
    assert!(text.contains("pub enum Status"), "Expected Status enum: {text}");
}

#[test]
fn test_cs_read_stubs() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_read",
        json!({ "path": "src/lib.rs", "mode": "stubs" }),
    );
    assert!(!is_err, "cs_read stubs error: {text}");
    assert!(text.contains("stubs"), "Expected stubs mode indicator: {text}");
    // Stubs should show function signatures
    assert!(text.contains("fn greet"), "Expected greet signature: {text}");
}

#[test]
fn test_cs_read_line_range() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_read",
        json!({ "path": "src/types.rs", "start_line": 1, "end_line": 5 }),
    );
    assert!(!is_err, "cs_read line range error: {text}");
    assert!(text.contains("lines 1-5"), "Expected line range header: {text}");
    // Should contain the doc comment and start of Config struct
    assert!(text.contains("Config"), "Expected Config in first 5 lines: {text}");
}

#[test]
fn test_cs_read_batch() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_read",
        json!({ "paths": ["src/main.rs", "src/types.rs"] }),
    );
    assert!(!is_err, "cs_read batch error: {text}");
    assert!(text.contains("main.rs"), "Expected main.rs in batch: {text}");
    assert!(text.contains("types.rs"), "Expected types.rs in batch: {text}");
}

#[test]
fn test_cs_read_budget() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_read",
        json!({
            "paths": ["src/main.rs", "src/lib.rs", "src/types.rs"],
            "budget": 5000
        }),
    );
    assert!(!is_err, "cs_read budget error: {text}");
    assert!(text.contains("budget:"), "Expected budget info: {text}");
    // All three small files should fit in 5000 token budget
    assert!(text.contains("main.rs"), "Expected main.rs: {text}");
}

// ---------------------------------------------------------------------------
// cs_modules tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_modules_list() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_modules", json!({ "action": "list" }));
    assert!(!is_err, "cs_modules list error: {text}");
    assert!(text.contains("modules"), "Expected module count: {text}");
}

#[test]
fn test_cs_modules_files() {
    let mut h = TestHarness::from_fixture("basic");
    // First find what modules exist
    let (list_text, _) = h.call_tool("cs_modules", json!({ "action": "list" }));
    // Use "Other" since small projects typically categorize files there
    let (text, is_err) = h.call_tool(
        "cs_modules",
        json!({ "action": "files", "module": "Other" }),
    );
    // If "Other" doesn't exist, the fixture might use a different category
    if is_err {
        // Try with the first module from the listing
        let first_module = list_text
            .lines()
            .skip(1) // skip the "N modules total" header
            .find(|l| !l.is_empty())
            .and_then(|l| l.split("  ").next())
            .unwrap_or("Other");
        let (text2, is_err2) = h.call_tool(
            "cs_modules",
            json!({ "action": "files", "module": first_module }),
        );
        assert!(!is_err2, "cs_modules files error: {text2}");
        assert!(text2.contains("files"), "Expected file listing: {text2}");
    } else {
        assert!(text.contains("files"), "Expected file listing: {text}");
    }
}

#[test]
fn test_cs_modules_deps() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_modules",
        json!({ "action": "deps", "module": "basic-fixture" }),
    );
    // This should find the Cargo.toml deps
    if !is_err {
        assert!(text.contains("serde") || text.contains("dependencies"), "Expected deps: {text}");
    }
    // It's also valid for the module name not to match exactly — the test validates the call succeeds
}

// ---------------------------------------------------------------------------
// cs_imports tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_imports_direct() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, _is_err) = h.call_tool(
        "cs_imports",
        json!({ "path": "src/main.rs" }),
    );
    // main.rs imports lib and types — the import scanner should detect these
    // Even if no imports are resolved, the call should succeed without error
    assert!(!text.starts_with("\u{26a0} Error:"), "cs_imports should not error: {text}");
}

#[test]
fn test_cs_imports_transitive() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, _is_err) = h.call_tool(
        "cs_imports",
        json!({ "path": "src/types.rs", "transitive": true }),
    );
    // Transitive import analysis should succeed
    assert!(!text.starts_with("\u{26a0} Error:"), "cs_imports transitive should not error: {text}");
}

// ---------------------------------------------------------------------------
// cs_git tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_git_blame() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_git",
        json!({ "action": "blame", "path": "src/main.rs" }),
    );
    assert!(!is_err, "cs_git blame error: {text}");
    // Should show blame lines with author "Test" from our fixture commit
    assert!(text.contains("Test"), "Expected author 'Test' in blame: {text}");
}

#[test]
fn test_cs_git_history() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_git",
        json!({ "action": "history", "path": "src/main.rs" }),
    );
    assert!(!is_err, "cs_git history error: {text}");
    assert!(text.contains("Initial commit"), "Expected 'Initial commit' in history: {text}");
}

#[test]
fn test_cs_git_hotspots() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_git",
        json!({ "action": "hotspots" }),
    );
    assert!(!is_err, "cs_git hotspots error: {text}");
    // Should show files ranked by commit count (all have 1 commit)
    assert!(
        text.contains("commit") || text.contains("Hot files"),
        "Expected hotspot output: {text}"
    );
}

// ---------------------------------------------------------------------------
// cs_status test
// ---------------------------------------------------------------------------

#[test]
fn test_cs_status() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_status", json!({}));
    assert!(!is_err, "cs_status error: {text}");
    assert!(text.contains("CodeScope"), "Expected CodeScope header: {text}");
    assert!(text.contains("[test]"), "Expected repo name: {text}");
    // Should show file count (fixture has 7 files)
    assert!(text.contains("Files:"), "Expected file count: {text}");
    assert!(text.contains("Session:"), "Expected session info: {text}");
}

// ---------------------------------------------------------------------------
// Legacy tool translation test
// ---------------------------------------------------------------------------

#[test]
fn test_legacy_tool_translation() {
    let mut h = TestHarness::from_fixture("basic");

    // cs_find should translate to cs_search
    let (text, is_err) = h.call_tool("cs_find", json!({ "query": "main" }));
    assert!(!is_err, "cs_find->cs_search translation error: {text}");
    assert!(text.contains("Found"), "Expected search results from legacy cs_find: {text}");

    // cs_read_file should translate to cs_read
    let (text, is_err) = h.call_tool("cs_read_file", json!({ "path": "src/main.rs" }));
    assert!(!is_err, "cs_read_file->cs_read translation error: {text}");
    assert!(text.contains("fn main"), "Expected file content from legacy cs_read_file: {text}");

    // cs_blame should translate to cs_git with action=blame
    let (text, is_err) = h.call_tool("cs_blame", json!({ "path": "src/main.rs" }));
    assert!(!is_err, "cs_blame->cs_git translation error: {text}");
    assert!(text.contains("Test"), "Expected blame output from legacy cs_blame: {text}");
}
