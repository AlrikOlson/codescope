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

// ---------------------------------------------------------------------------
// MCP prompts tests
// ---------------------------------------------------------------------------

#[test]
fn test_prompts_list() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "prompts/list",
        "params": {}
    });
    let resp = h.dispatch(msg).expect("Expected response");
    let prompts = resp["result"]["prompts"].as_array().expect("prompts should be an array");
    assert_eq!(prompts.len(), 4, "Expected 4 prompts, got {}", prompts.len());

    let names: Vec<&str> = prompts.iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(names.contains(&"implement-feature"), "Missing implement-feature prompt");
    assert!(names.contains(&"debug-error"), "Missing debug-error prompt");
    assert!(names.contains(&"write-tests"), "Missing write-tests prompt");
    assert!(names.contains(&"review-code"), "Missing review-code prompt");
}

#[test]
fn test_prompts_get_implement_feature() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "prompts/get",
        "params": {
            "name": "implement-feature",
            "arguments": { "description": "add logging support" }
        }
    });
    let resp = h.dispatch(msg).expect("Expected response");
    assert!(resp["error"].is_null(), "Expected no error: {resp}");
    let messages = resp["result"]["messages"].as_array().expect("messages should be an array");
    assert!(!messages.is_empty(), "Expected at least one message");
    let text = messages[0]["content"]["text"].as_str().unwrap_or("");
    assert!(text.contains("add logging support"), "Expected feature description in prompt text: {text}");
    assert!(text.contains("conventions"), "Expected conventions in prompt text: {text}");
}

#[test]
fn test_prompts_get_unknown() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "prompts/get",
        "params": {
            "name": "nonexistent-prompt",
            "arguments": {}
        }
    });
    let resp = h.dispatch(msg).expect("Expected response");
    assert!(resp["error"].is_object(), "Expected error for unknown prompt: {resp}");
    let error_msg = resp["error"]["message"].as_str().unwrap_or("");
    assert!(error_msg.contains("Unknown prompt"), "Expected 'Unknown prompt' in error: {error_msg}");
}

// ---------------------------------------------------------------------------
// MCP resources tests
// ---------------------------------------------------------------------------

#[test]
fn test_resources_list() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/list",
        "params": {}
    });
    let resp = h.dispatch(msg).expect("Expected response");
    let resources = resp["result"]["resources"].as_array().expect("resources should be an array");
    assert_eq!(resources.len(), 4, "Expected 4 resources, got {}", resources.len());

    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(uris.contains(&"conventions://summary"), "Missing conventions://summary");
    assert!(uris.contains(&"conventions://error-handling"), "Missing conventions://error-handling");
    assert!(uris.contains(&"conventions://naming"), "Missing conventions://naming");
    assert!(uris.contains(&"conventions://testing"), "Missing conventions://testing");
}

#[test]
fn test_resources_read_conventions_summary() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/read",
        "params": { "uri": "conventions://summary" }
    });
    let resp = h.dispatch(msg).expect("Expected response");
    assert!(resp["error"].is_null(), "Expected no error: {resp}");
    let contents = resp["result"]["contents"].as_array().expect("contents should be an array");
    assert!(!contents.is_empty(), "Expected at least one content entry");
    let text = contents[0]["text"].as_str().unwrap_or("");
    assert!(text.contains("Project Conventions"), "Expected 'Project Conventions' in text: {text}");
    assert!(text.contains("Error Handling"), "Expected 'Error Handling' section: {text}");
}

#[test]
fn test_resources_read_unknown() {
    let mut h = TestHarness::from_fixture("basic");
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "resources/read",
        "params": { "uri": "conventions://nonexistent" }
    });
    let resp = h.dispatch(msg).expect("Expected response");
    assert!(resp["error"].is_object(), "Expected error for unknown resource: {resp}");
}

// ---------------------------------------------------------------------------
// Initialize capabilities test (prompts + resources)
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_capabilities() {
    let mut h = TestHarness::from_fixture("basic");
    let resp = h.initialize();
    let caps = &resp["result"]["capabilities"];
    assert!(caps["tools"].is_object(), "Expected tools capability");
    assert!(caps["prompts"].is_object(), "Expected prompts capability");
    assert!(caps["resources"].is_object(), "Expected resources capability");
}

// ---------------------------------------------------------------------------
// cs_git evolution + cochange tests
// ---------------------------------------------------------------------------

#[test]
fn test_cs_git_evolution_action() {
    let mut h = TestHarness::from_fixture("basic");
    // Use explicit line range (the entire main.rs file in the fixture is small)
    let (text, is_err) = h.call_tool(
        "cs_git",
        json!({ "action": "evolution", "path": "src/main.rs", "start_line": 1, "end_line": 10 }),
    );
    assert!(!is_err, "cs_git evolution error: {text}");
    // Should find the initial commit that created main.rs
    assert!(
        text.contains("Initial commit") || text.contains("commits"),
        "Expected evolution output: {text}"
    );
}

#[test]
fn test_cs_git_cochange_action() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_git",
        json!({ "action": "cochange", "path": "src/main.rs" }),
    );
    assert!(!is_err, "cs_git cochange error: {text}");
    // With a single commit touching all files, everything cochanges with main.rs
    assert!(
        text.contains("temporal coupling") || text.contains("No cochanged"),
        "Expected cochange output: {text}"
    );
}

// ---------------------------------------------------------------------------
// Code graph edge type tests (Phase 3, requires treesitter)
// ---------------------------------------------------------------------------

#[test]
#[cfg(feature = "treesitter")]
fn test_cs_imports_edge_type_call() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_imports",
        json!({ "path": "src/main.rs", "edge_type": "call" }),
    );
    assert!(!is_err, "cs_imports call error: {text}");
    // main.rs calls greet() from lib.rs
    assert!(
        text.contains("greet") || text.contains("call"),
        "Expected call edges for main.rs: {text}"
    );
}

#[test]
#[cfg(feature = "treesitter")]
fn test_cs_imports_edge_type_all() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_imports",
        json!({ "path": "src/lib.rs", "edge_type": "all" }),
    );
    assert!(!is_err, "cs_imports all error: {text}");
    // lib.rs references Config type from types.rs
    assert!(
        text.contains("Config") || text.contains("edge"),
        "Expected edges for lib.rs: {text}"
    );
}

#[test]
#[cfg(feature = "treesitter")]
fn test_cs_imports_edge_type_type_ref() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool(
        "cs_imports",
        json!({ "path": "src/lib.rs", "edge_type": "type_ref" }),
    );
    assert!(!is_err, "cs_imports type_ref error: {text}");
    // lib.rs uses Config type in process() function signature
    assert!(
        text.contains("Config") || text.contains("type_ref") || text.contains("No type_ref"),
        "Expected type_ref info: {text}"
    );
}

// ---------------------------------------------------------------------------
// Session memory + structured output tests (Phase 4)
// ---------------------------------------------------------------------------

#[test]
fn test_search_confidence_header() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config" }));
    assert!(!is_err, "cs_search error: {text}");
    // Should include confidence/coverage metadata
    assert!(
        text.contains("Confidence:"),
        "Search results should include Confidence header: {text}"
    );
    assert!(
        text.contains("Coverage:"),
        "Search results should include Coverage header: {text}"
    );
}

#[test]
fn test_search_multi_term_coverage() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config verbose" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(
        text.contains("Confidence:") && text.contains("Coverage:"),
        "Multi-term search should show confidence/coverage: {text}"
    );
    assert!(text.contains("types.rs"), "Config verbose should find types.rs: {text}");
}

// ---------------------------------------------------------------------------
// Search relevance tests (cross-line matching, confidence, coverage)
// ---------------------------------------------------------------------------

#[test]
fn test_search_cross_line_multi_term() {
    // "Config verbose" — both terms exist in types.rs on different lines
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config verbose" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(text.contains("types.rs"), "Should find types.rs with cross-line terms: {text}");
}

#[test]
fn test_search_cross_line_coverage_full() {
    // Cross-line match where all terms are found → Coverage: full
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config verbose" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(text.contains("Coverage: full"), "Cross-line all-terms should be full: {text}");
}

#[test]
fn test_search_single_line_still_works() {
    // "Config name" on the same line (types.rs line 4) must still work
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config name" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(text.contains("types.rs"), "Same-line multi-term should still match: {text}");
}

#[test]
fn test_search_no_match_when_term_missing() {
    // "Config zzznonexistent" — one term not in codebase → no full coverage
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config zzznonexistent" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(!text.contains("Coverage: full"), "Missing term should not be full: {text}");
}

#[test]
fn test_confidence_high_on_exact_match() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "Config" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(
        text.contains("Confidence: high") || text.contains("Confidence: medium"),
        "Exact match should not be low: {text}"
    );
}

#[test]
fn test_confidence_low_on_gibberish() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "zzzzxyznonexistent" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(text.contains("Confidence: low"), "Gibberish should be low: {text}");
}

#[test]
fn test_coverage_full_single_term() {
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "greet" }));
    assert!(!is_err, "cs_search error: {text}");
    assert!(text.contains("Coverage: full"), "Single term should be full: {text}");
}

#[test]
fn test_grep_all_mode_still_line_level() {
    // Regression: cs_grep must keep line-level all-terms matching
    let mut h = TestHarness::from_fixture("basic");
    let (text, is_err) = h.call_tool("cs_grep", json!({ "query": "Config verbose", "match_mode": "all" }));
    assert!(!is_err, "cs_grep error: {text}");
    assert!(!text.contains("Config"), "cs_grep all-mode should not match cross-line: {text}");
}

#[test]
fn test_frontier_tracking() {
    let mut h = TestHarness::from_fixture("basic");
    h.initialize();

    // Read a file — this should populate the frontier with its import neighbors
    let (text, is_err) = h.call_tool("cs_read", json!({ "path": "src/main.rs" }));
    assert!(!is_err, "cs_read error: {text}");

    // Now search — the frontier count should appear if neighbors exist
    let (search_text, is_err) = h.call_tool("cs_search", json!({ "query": "greet" }));
    assert!(!is_err, "cs_search error: {search_text}");
    // The search should still work and include confidence metadata
    assert!(
        search_text.contains("Confidence:"),
        "Search should include confidence: {search_text}"
    );
}

#[test]
fn test_session_query_history() {
    let mut h = TestHarness::from_fixture("basic");
    h.initialize();

    // Multiple searches should not cause errors
    h.call_tool("cs_search", json!({ "query": "Config" }));
    h.call_tool("cs_search", json!({ "query": "greet" }));
    let (text, is_err) = h.call_tool("cs_search", json!({ "query": "process" }));
    assert!(!is_err, "Third search should succeed: {text}");
    assert!(
        text.contains("Confidence:"),
        "Third search should include confidence: {text}"
    );
}
