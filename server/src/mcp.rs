//! MCP JSON-RPC server implementing the Model Context Protocol.
//!
//! Handles tool dispatch for 9 consolidated tools (`cs_search`, `cs_grep`, `cs_read`,
//! `cs_modules`, `cs_imports`, `cs_git`, `cs_status`, `cs_rescan`, `cs_add_repo`),
//! protocol version negotiation, and legacy tool name translation for backward compatibility.

use crate::budget::{allocate_budget, BudgetUnit, DEFAULT_TOKEN_BUDGET};
use crate::fuzzy::run_search;
use crate::scan::get_category_path;
use crate::stubs::extract_stubs;
use crate::types::*;
use regex::RegexBuilder;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, Write as IoWrite};
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// Repo resolution helper
// ---------------------------------------------------------------------------

fn resolve_repo<'a>(
    state: &'a ServerState,
    args: &serde_json::Value,
) -> Result<&'a RepoState, String> {
    match args.get("repo").and_then(|v| v.as_str()) {
        Some(name) => state.repos.get(name).ok_or_else(|| {
            let available: Vec<&str> = state.repos.keys().map(|k| k.as_str()).collect();
            format!("Unknown repo '{name}'. Available: {}", available.join(", "))
        }),
        None if state.repos.len() == 1 => Ok(state.repos.values().next().unwrap()),
        None if state.default_repo.is_some() => {
            let name = state.default_repo.as_ref().unwrap();
            Ok(state.repos.get(name).unwrap())
        }
        None => {
            let available: Vec<&str> = state.repos.keys().map(|k| k.as_str()).collect();
            Err(format!(
                "Multiple repos indexed. Specify 'repo' parameter. Available: {}",
                available.join(", ")
            ))
        }
    }
}

/// For search tools: collect all repos when no specific repo is requested.
fn resolve_repos_for_search<'a>(
    state: &'a ServerState,
    args: &serde_json::Value,
) -> Vec<&'a RepoState> {
    match args.get("repo").and_then(|v| v.as_str()) {
        Some(name) => match state.repos.get(name) {
            Some(repo) => vec![repo],
            None => vec![],
        },
        None if state.repos.len() == 1 => vec![state.repos.values().next().unwrap()],
        None => state.repos.values().collect(),
    }
}

/// Format a path with repo prefix when multiple repos exist.
fn repo_path(repo: &RepoState, path: &str, multi: bool) -> String {
    if multi {
        format!("[{}] {}", repo.name, path)
    } else {
        path.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tool definitions (consolidated: 9 tools)
// ---------------------------------------------------------------------------

fn tool_definitions() -> serde_json::Value {
    // Shared annotation sets (MCP spec 2025-11-25)
    let ro = serde_json::json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    let mutating = serde_json::json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    let additive = serde_json::json!({
        "readOnlyHint": false,
        "destructiveHint": false,
        "idempotentHint": false,
        "openWorldHint": false
    });

    serde_json::json!([
        {
            "name": "cs_search",
            "annotations": ro,
            "description": "YOUR PRIMARY DISCOVERY TOOL. Combined search: fuzzy filename + content grep + semantic search (when available) in one call. Returns a unified ranked list. Use this first for discovering files and modules.\n\nReturns files ranked by combined relevance. When semantic search is available, results are automatically fused with keyword matches for better accuracy. Use fileLimit/moduleLimit to control result counts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (e.g. 'VolumetricCloud', 'config parser', 'resource cleanup')" },
                    "match_mode": { "type": "string", "enum": ["all", "any", "exact", "regex"], "description": "How to match multi-word queries. 'all' (default): line must contain ALL terms. 'any': line contains ANY term (OR). 'exact': treat query as literal phrase. 'regex': raw regex pattern." },
                    "ext": { "type": "string", "description": "Comma-separated extensions to filter (e.g. 'h,cpp' or 'rs,ts')" },
                    "path": { "type": "string", "description": "Path prefix to filter files (e.g. 'server/src' or 'src/components')" },
                    "category": { "type": "string", "description": "Module category prefix to filter" },
                    "limit": { "type": "integer", "description": "Max file results (default: 20)" },
                    "fileLimit": { "type": "integer", "description": "Max file results (default: 30, alias for limit)" },
                    "moduleLimit": { "type": "integer", "description": "Max module results (default: 5)" },
                    "repo": { "type": "string", "description": "Repository name (searches all repos if omitted)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "cs_grep",
            "annotations": ro,
            "description": "Search source file contents (case-insensitive). Default match_mode='all' requires ALL terms present in a line. Use 'any' for OR, 'exact' for literal phrases, 'regex' for patterns.\n\nTips: Filter with ext='rs,go', path='server/src' prefix, or category. Follow up with cs_read for full context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (min 1 char)." },
                    "match_mode": { "type": "string", "enum": ["all", "any", "exact", "regex"], "description": "How to match multi-word queries. 'all' (default): line must contain ALL terms. 'any': line contains ANY term (OR). 'exact': treat query as literal phrase. 'regex': raw regex pattern." },
                    "ext": { "type": "string", "description": "Comma-separated extensions to filter (e.g. 'h,cpp' or 'rs,go')" },
                    "path": { "type": "string", "description": "Path prefix to filter files (e.g. 'server/src' or 'src/components')" },
                    "category": { "type": "string", "description": "Module category prefix to filter" },
                    "limit": { "type": "integer", "description": "Max files to return. Default: 50" },
                    "max_per_file": { "type": "integer", "description": "Max matching lines shown per file. Default: 8, max: 50" },
                    "context": { "type": "integer", "description": "Lines of context before/after each match (0-10). Default: 2" },
                    "output": { "type": "string", "enum": ["full", "files_only"], "description": "Output mode. 'full' (default): matching lines with context. 'files_only': just filenames and match counts." },
                    "repo": { "type": "string", "description": "Repository name (searches all repos if omitted)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "cs_read",
            "annotations": ro,
            "description": "Read source files. Use 'path' for a single file, 'paths' for batch reads.\n\nModes:\n- stubs (recommended first): structural outline with class/function signatures, no bodies.\n- full: complete content. For large files, use start_line/end_line.\n\nWith 'paths' + 'budget': budget-aware batch read with importance-weighted allocation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root (single file)" },
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of relative paths (batch read, max 50)"
                    },
                    "mode": { "type": "string", "enum": ["full", "stubs"], "description": "full = complete file, stubs = structural outline only. Default: full" },
                    "start_line": { "type": "integer", "description": "First line to return (1-based). Single file + mode='full' only." },
                    "end_line": { "type": "integer", "description": "Last line to return (1-based, inclusive). Single file + mode='full' only." },
                    "budget": { "type": "integer", "description": "Max token budget for batch reads. Triggers smart compression. Default: 50000" },
                    "ordering": { "type": "string", "enum": ["importance", "attention"], "description": "Output ordering for budget mode. 'importance' (default): descending by relevance. 'attention': primacy/recency optimized." },
                    "include_seen": { "type": "boolean", "description": "If true, don't deprioritize previously-read files in budget mode. Default: false" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                }
            }
        },
        {
            "name": "cs_modules",
            "annotations": ro,
            "description": "Explore module/category structure. Actions:\n- list (default): list modules with file counts\n- files: get all files in a specific module\n- deps: get package-level dependencies from manifests (Cargo.toml, package.json, go.mod). For file-level import relationships, use cs_imports instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "files", "deps"], "description": "What to do. Default: list" },
                    "module": { "type": "string", "description": "Module name (required for 'files' and 'deps' actions)" },
                    "prefix": { "type": "string", "description": "Filter modules by prefix (for 'list' action)" },
                    "limit": { "type": "integer", "description": "Max modules to return (for 'list' action). Default: 100" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                }
            }
        },
        {
            "name": "cs_imports",
            "annotations": ro,
            "description": "Find import/include relationships for a file. Shows what a file imports and/or what imports it.\n\nSet transitive=true for impact analysis: finds everything that depends on the file (directly or transitively) via BFS over the import graph.\n\nUse edge_type to query structural code edges (call, type_ref, extends, implements) when treesitter feature is enabled.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "direction": { "type": "string", "enum": ["imports", "imported_by", "both"], "description": "Which direction to query. Default: both" },
                    "transitive": { "type": "boolean", "description": "If true, perform full impact analysis (BFS traversal). Default: false" },
                    "edge_type": { "type": "string", "enum": ["import", "call", "type_ref", "extends", "all"], "description": "Type of edges to query. Default: import. Requires treesitter feature for non-import types." },
                    "max_depth": { "type": "integer", "description": "Max traversal depth for impact analysis (default: 5)" },
                    "limit": { "type": "integer", "description": "Max files to show in impact analysis (default: 50)" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "cs_git",
            "annotations": ro,
            "description": "Git history analysis. Actions:\n- blame: who last modified each line of a file\n- history: recent commits that touched a file\n- changed: files changed since a commit/branch/tag\n- hotspots: most frequently changed files (churn ranking)\n- evolution: commits that modified a specific symbol's line range\n- cochange: files that frequently change together with a given file (temporal coupling)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["blame", "history", "changed", "hotspots", "evolution", "cochange"], "description": "What to do (required)" },
                    "path": { "type": "string", "description": "File path (required for blame/history/evolution/cochange)" },
                    "symbol": { "type": "string", "description": "Symbol name for evolution action (used to look up line range from AST index)" },
                    "since": { "type": "string", "description": "Commit/branch/tag to diff against (required for 'changed')" },
                    "start_line": { "type": "integer", "description": "First line for blame/evolution (1-based, optional)" },
                    "end_line": { "type": "integer", "description": "Last line for blame/evolution (1-based, optional)" },
                    "limit": { "type": "integer", "description": "Max results (default: 10 for history, 20 for hotspots/cochange)" },
                    "days": { "type": "integer", "description": "Look back N days for hotspots/cochange (default: 90)" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["action"]
            }
        },
        {
            "name": "cs_status",
            "annotations": ro,
            "description": "Show indexed repositories, file counts, language breakdown, scan time, and session info (files read, tokens served).",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "cs_rescan",
            "annotations": mutating,
            "description": "Re-index one or all repositories without restarting the server. Use after significant file changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Specific repo to rescan (default: all)" }
                }
            }
        },
        {
            "name": "cs_add_repo",
            "annotations": additive,
            "description": "Dynamically add a new repository to the index at runtime.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name/alias for the repository" },
                    "root": { "type": "string", "description": "Absolute path to the repository root" }
                },
                "required": ["name", "root"]
            }
        }
    ])
}

// ---------------------------------------------------------------------------
// Legacy tool name translation (backward compatibility)
// ---------------------------------------------------------------------------

fn translate_legacy_tool<'a>(
    name: &'a str,
    args: &serde_json::Value,
) -> (&'a str, serde_json::Value) {
    match name {
        "cs_find" | "cs_semantic_search" => ("cs_search", args.clone()),
        "cs_read_file" => ("cs_read", args.clone()),
        "cs_read_files" => ("cs_read", args.clone()),
        "cs_read_context" => ("cs_read", args.clone()),
        "cs_list_modules" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("list")));
            ("cs_modules", a)
        }
        "cs_get_module_files" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("files")));
            ("cs_modules", a)
        }
        "cs_get_deps" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("deps")));
            ("cs_modules", a)
        }
        "cs_find_imports" => ("cs_imports", args.clone()),
        "cs_impact" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("transitive".to_string(), serde_json::json!(true)));
            ("cs_imports", a)
        }
        "cs_blame" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("blame")));
            ("cs_git", a)
        }
        "cs_file_history" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("history")));
            ("cs_git", a)
        }
        "cs_changed_since" => {
            let mut a = args.clone();
            a.as_object_mut().map(|m| m.insert("action".to_string(), serde_json::json!("changed")));
            ("cs_git", a)
        }
        "cs_hot_files" => {
            let mut a = args.clone();
            a.as_object_mut()
                .map(|m| m.insert("action".to_string(), serde_json::json!("hotspots")));
            ("cs_git", a)
        }
        "cs_session_info" => ("cs_status", args.clone()),
        _ => (name, args.clone()),
    }
}

// ---------------------------------------------------------------------------
// Tool call handler (read-only, takes &ServerState)
// ---------------------------------------------------------------------------

fn handle_tool_call(
    state: &ServerState,
    original_name: &str,
    original_args: &serde_json::Value,
    session: &mut Option<SessionState>,
) -> (String, bool) {
    let (name, args) = translate_legacy_tool(original_name, original_args);
    match name {
        // =================================================================
        // cs_read — unified file reading
        // =================================================================
        "cs_read" => {
            // Dispatch based on params:
            // - path (string) → single file read
            // - paths (array) + budget → budget-aware batch read
            // - paths (array) without budget → simple batch read
            if let Some(path_val) = args.get("path").and_then(|v| v.as_str()) {
                // Single file read (was cs_read_file)
                let repo = match resolve_repo(state, &args) {
                    Ok(r) => r,
                    Err(e) => return (format!("Error: {e}"), true),
                };
                let path = path_val;
                let mode = args["mode"].as_str().unwrap_or("full");
                let start_line = args["start_line"].as_u64().map(|n| n.max(1) as usize);
                let end_line = args["end_line"].as_u64().map(|n| n as usize);
                match validate_path(&repo.root, path) {
                    Err(e) => (format!("Error: {e}"), true),
                    Ok(full_path) => match fs::read_to_string(&full_path) {
                        Err(_) => ("Error: Could not read file".to_string(), true),
                        Ok(raw) => {
                            if let Some(ref mut s) = session {
                                let approx_tokens = raw.len() / 4;
                                s.record_read(path, approx_tokens);
                                // Update exploration frontier with import neighbors
                                let neighbors: Vec<String> = repo
                                    .import_graph
                                    .imports
                                    .get(path)
                                    .cloned()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .chain(
                                        repo.import_graph
                                            .imported_by
                                            .get(path)
                                            .cloned()
                                            .unwrap_or_default(),
                                    )
                                    .collect();
                                s.update_frontier(path, &neighbors);
                            }
                            if mode == "stubs" {
                                let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
                                let content = extract_stubs(&raw, ext);
                                let lines = content.lines().count();
                                (format!("# {path}\n({lines} lines, stubs)\n\n{content}"), false)
                            } else if start_line.is_some() || end_line.is_some() {
                                let all_lines: Vec<&str> = raw.lines().collect();
                                let total = all_lines.len();
                                let s = start_line.unwrap_or(1).min(total).max(1);
                                let e = end_line.unwrap_or(total).min(total);
                                if s > e {
                                    return (
                                        format!("Error: start_line ({s}) > end_line ({e})"),
                                        true,
                                    );
                                }
                                let width = format!("{}", e).len();
                                let mut content = String::new();
                                for i in s..=e {
                                    content.push_str(&format!(
                                        "{:>w$}: {}\n",
                                        i,
                                        all_lines[i - 1],
                                        w = width
                                    ));
                                }
                                (format!("# {path} (lines {s}-{e} of {total})\n\n{content}"), false)
                            } else {
                                let content = if raw.len() > MAX_FILE_READ {
                                    let mut end = MAX_FILE_READ;
                                    while !raw.is_char_boundary(end) && end > 0 {
                                        end -= 1;
                                    }
                                    format!("{}\n\n[truncated at 512KB]", &raw[..end])
                                } else {
                                    raw
                                };
                                let lines = content.lines().count();
                                (format!("# {path}\n({lines} lines)\n\n{content}"), false)
                            }
                        }
                    },
                }
            } else if let Some(paths_arr) = args.get("paths").and_then(|v| v.as_array()) {
                // Batch read
                let has_budget = args.get("budget").is_some();
                if has_budget {
                    // Budget-aware batch read (was cs_read_context)
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let paths: Vec<String> = paths_arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    let budget =
                        args["budget"].as_u64().unwrap_or(DEFAULT_TOKEN_BUDGET as u64) as usize;
                    let unit = match args["unit"].as_str() {
                        Some("chars") => BudgetUnit::Chars,
                        _ => BudgetUnit::Tokens,
                    };

                    if paths.is_empty() {
                        return ("Error: paths array is empty".to_string(), true);
                    }

                    let query = args["query"].as_str();
                    let ordering = args["ordering"].as_str();
                    let include_seen = args["include_seen"].as_bool().unwrap_or(false);
                    let seen =
                        if include_seen { None } else { session.as_ref().map(|s| s.seen_paths()) };
                    let resp = allocate_budget(
                        &repo.root,
                        &paths,
                        &repo.all_files,
                        budget,
                        &unit,
                        query,
                        ordering,
                        seen.as_ref(),
                        &repo.deps,
                        &repo.stub_cache,
                        &*state.tokenizer,
                        &repo.config,
                    );

                    if let Some(ref mut s) = session {
                        for (path, entry) in &resp.files {
                            if !path.starts_with('_') {
                                s.record_read(path, entry.tokens);
                            }
                        }
                    }

                    let tier_names = |t: u8| match t {
                        1 => "full stubs",
                        2 => "pruned",
                        3 => "TOC",
                        4 => "manifest",
                        _ => "error",
                    };

                    let mut out = format!(
                        "Context: {} files, ~{} tokens (budget: {})\n",
                        resp.summary.total_files, resp.summary.total_tokens, resp.summary.budget
                    );

                    let mut tier_parts: Vec<String> = Vec::new();
                    for tier in 1..=4u8 {
                        let key = tier.to_string();
                        if let Some(&count) = resp.summary.tier_counts.get(&key) {
                            if count > 0 {
                                tier_parts.push(format!("{} {}", count, tier_names(tier)));
                            }
                        }
                    }
                    if !tier_parts.is_empty() {
                        out.push_str(&format!("Tiers: {}\n\n", tier_parts.join(", ")));
                    }

                    let mut sorted_paths: Vec<&String> = resp.files.keys().collect();
                    sorted_paths.sort();

                    for path in sorted_paths {
                        if let Some(entry) = resp.files.get(path) {
                            if entry.tier == 0 {
                                out.push_str(&format!("# {path}\n{}\n\n", entry.content));
                            } else {
                                let tier_label = if entry.tier > 1 {
                                    format!(" [{}]", tier_names(entry.tier))
                                } else {
                                    String::new()
                                };
                                out.push_str(&format!("# {path}{tier_label}\n{}\n", entry.content));
                            }
                        }
                    }

                    (out, false)
                } else {
                    // Simple batch read (was cs_read_files)
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let paths: Vec<&str> = paths_arr.iter().filter_map(|v| v.as_str()).collect();
                    let mode = args["mode"].as_str().unwrap_or("full");

                    if paths.len() > 50 {
                        return ("Error: Max 50 files per call".to_string(), true);
                    }

                    let mut out = String::new();
                    for p in &paths {
                        match validate_path(&repo.root, p) {
                            Err(e) => {
                                out.push_str(&format!("# {p}\nError: {e}\n\n"));
                            }
                            Ok(full_path) => match fs::read_to_string(&full_path) {
                                Err(_) => {
                                    out.push_str(&format!("# {p}\nError: Could not read file\n\n"));
                                }
                                Ok(raw) => {
                                    if let Some(ref mut s) = session {
                                        let approx_tokens = raw.len() / 4;
                                        s.record_read(p, approx_tokens);
                                    }
                                    let content = if mode == "stubs" {
                                        let ext = p.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
                                        extract_stubs(&raw, ext)
                                    } else {
                                        raw
                                    };
                                    out.push_str(&format!("# {p}\n{content}\n\n"));
                                }
                            },
                        }
                    }
                    (out, false)
                }
            } else {
                ("Error: Either 'path' (string) or 'paths' (array) is required".to_string(), true)
            }
        }

        // =================================================================
        // cs_grep — exact pattern matching (unchanged)
        // =================================================================
        "cs_grep" => {
            let repos = resolve_repos_for_search(state, &args);
            if repos.is_empty() {
                return ("Error: No matching repos found".to_string(), true);
            }
            let multi = repos.len() > 1;

            let query = args["query"].as_str().unwrap_or("");
            if query.is_empty() {
                return ("Error: Query must not be empty".to_string(), true);
            }

            let limit = args["limit"].as_u64().unwrap_or(50).min(200) as usize;
            let max_per_file = args["max_per_file"].as_u64().unwrap_or(8).min(50) as usize;
            let context_lines = args["context"].as_u64().unwrap_or(2).min(10) as usize;
            let ext_filter: Option<HashSet<String>> = args["ext"].as_str().map(|exts| {
                exts.split(',').map(|e| e.trim().trim_start_matches('.').to_string()).collect()
            });
            let cat_filter = args["category"].as_str();
            let path_filter = args["path"].as_str();
            let match_mode = args["match_mode"].as_str().unwrap_or("all");
            let output_mode = args["output"].as_str().unwrap_or("full");

            let terms: Vec<&str> = query.split_whitespace().collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
            let require_all_terms = match_mode == "all" && terms.len() > 1;

            let pattern = match match_mode {
                "exact" => RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build(),
                "regex" => RegexBuilder::new(query).case_insensitive(true).build(),
                _ => {
                    let pattern_str =
                        terms.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");
                    RegexBuilder::new(&pattern_str).case_insensitive(true).build()
                }
            };
            let pattern = match pattern {
                Ok(p) => p,
                Err(e) => return (format!("Error: Invalid pattern: {e}"), true),
            };

            let start = std::time::Instant::now();

            struct GrepFileHit {
                display_path: String,
                desc: String,
                match_indices: Vec<usize>,
                total_match_count: usize,
                lines: Vec<String>,
                score: f64,
                terms_matched: usize,
                total_terms: usize,
            }

            let mut file_hits: Vec<GrepFileHit> = Vec::new();

            for repo in &repos {
                let config = &repo.config;
                let idf_weights: Vec<f64> =
                    terms_lower.iter().map(|t| repo.term_doc_freq.idf(t)).collect();
                let candidates: Vec<&ScannedFile> = repo
                    .all_files
                    .iter()
                    .filter(|f| {
                        if let Some(prefix) = path_filter {
                            if !f.rel_path.starts_with(prefix) {
                                return false;
                            }
                        }
                        if let Some(ref exts) = ext_filter {
                            if !exts.contains(&f.ext) {
                                return false;
                            }
                        }
                        if let Some(cat) = cat_filter {
                            let file_cat = get_category_path(&f.rel_path, config).join(" > ");
                            if !file_cat.starts_with(cat) {
                                return false;
                            }
                        }
                        true
                    })
                    .collect();

                use rayon::prelude::*;
                let mut par_hits: Vec<GrepFileHit> = candidates
                    .par_iter()
                    .filter_map(|file| {
                        let content = fs::read_to_string(&file.abs_path).ok()?;
                        let lines: Vec<&str> = content.lines().collect();
                        let total_lines = lines.len().max(1);

                        let mut match_indices: Vec<usize> = Vec::new();
                        let mut total_match_count = 0usize;
                        let mut first_match_line_idx = usize::MAX;
                        let mut terms_seen = std::collections::HashSet::new();
                        for (i, line) in lines.iter().enumerate() {
                            if !pattern.is_match(line) {
                                continue;
                            }
                            if require_all_terms {
                                let line_lower = line.to_lowercase();
                                if !terms_lower.iter().all(|t| line_lower.contains(t.as_str())) {
                                    continue;
                                }
                            }
                            total_match_count += 1;
                            if first_match_line_idx == usize::MAX {
                                first_match_line_idx = i;
                            }
                            let line_lower = line.to_lowercase();
                            for (ti, term) in terms_lower.iter().enumerate() {
                                if line_lower.contains(term.as_str()) {
                                    terms_seen.insert(ti);
                                }
                            }
                            if match_indices.len() < max_per_file {
                                match_indices.push(i);
                            }
                        }

                        if match_indices.is_empty() {
                            return None;
                        }

                        let filename = file
                            .rel_path
                            .rsplit('/')
                            .next()
                            .unwrap_or(&file.rel_path)
                            .to_lowercase();
                        let score = grep_relevance_score(
                            total_match_count,
                            total_lines,
                            &filename,
                            &file.ext,
                            &terms_lower,
                            terms_seen.len(),
                            if first_match_line_idx == usize::MAX {
                                0
                            } else {
                                first_match_line_idx
                            },
                            &idf_weights,
                        );

                        Some(GrepFileHit {
                            display_path: repo_path(repo, &file.rel_path, multi),
                            desc: file.desc.clone(),
                            match_indices,
                            total_match_count,
                            lines: lines.iter().map(|l| l.to_string()).collect(),
                            score,
                            terms_matched: terms_seen.len(),
                            total_terms: terms_lower.len(),
                        })
                    })
                    .collect();
                file_hits.append(&mut par_hits);
            }

            file_hits
                .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

            let mut results = Vec::new();
            let mut total_matches: usize = 0;

            let truncate = |line: &str| -> String {
                if line.len() > 200 {
                    format!("{}...", &line[..line.floor_char_boundary(200)])
                } else {
                    line.to_string()
                }
            };

            for hit in &file_hits {
                if results.len() >= limit {
                    break;
                }
                total_matches += hit.total_match_count;

                let term_info = if hit.total_terms > 1 {
                    format!(", {}/{} terms", hit.terms_matched, hit.total_terms)
                } else {
                    String::new()
                };

                if output_mode == "files_only" {
                    results.push(format!(
                        "{}  ({}, score {:.0}{}, {} matches)",
                        hit.display_path, hit.desc, hit.score, term_info, hit.total_match_count
                    ));
                } else if context_lines == 0 {
                    let file_lines: Vec<String> = hit
                        .match_indices
                        .iter()
                        .map(|&i| format!("  L{}: {}", i + 1, truncate(&hit.lines[i])))
                        .collect();
                    results.push(format!(
                        "{}  ({}, score {:.0}{})\n{}",
                        hit.display_path,
                        hit.desc,
                        hit.score,
                        term_info,
                        file_lines.join("\n")
                    ));
                } else {
                    let match_set: HashSet<usize> = hit.match_indices.iter().copied().collect();
                    let mut ranges: Vec<(usize, usize)> = Vec::new();
                    for &idx in &hit.match_indices {
                        let s = idx.saturating_sub(context_lines);
                        let e = (idx + context_lines).min(hit.lines.len() - 1);
                        if let Some(last) = ranges.last_mut() {
                            if s <= last.1 + 1 {
                                last.1 = e;
                            } else {
                                ranges.push((s, e));
                            }
                        } else {
                            ranges.push((s, e));
                        }
                    }

                    let mut file_output: Vec<String> = Vec::new();
                    for (ri, &(s, e)) in ranges.iter().enumerate() {
                        if ri > 0 {
                            file_output.push("  ---".to_string());
                        }
                        for i in s..=e {
                            let sep = if match_set.contains(&i) { ':' } else { '|' };
                            file_output.push(format!(
                                "  L{}{} {}",
                                i + 1,
                                sep,
                                truncate(&hit.lines[i])
                            ));
                        }
                    }
                    results.push(format!(
                        "{}  ({}, score {:.0}{})\n{}",
                        hit.display_path,
                        hit.desc,
                        hit.score,
                        term_info,
                        file_output.join("\n")
                    ));
                }
            }

            let query_time = start.elapsed().as_millis();
            let header = format!(
                "Found {} matches in {} files ({query_time}ms, ranked by relevance)\n\n",
                total_matches,
                results.len()
            );
            (format!("{header}{}", results.join("\n\n")), false)
        }

        // =================================================================
        // cs_modules — list/files/deps
        // =================================================================
        "cs_modules" => {
            let action = args["action"].as_str().unwrap_or("list");
            match action {
                "files" => {
                    // Was cs_get_module_files
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let module = args["module"].as_str().unwrap_or("");
                    let prefix_dot = format!("{module} > ");
                    let mut out = String::new();
                    let mut count = 0;
                    for (cat, files) in &repo.manifest {
                        if cat != module && !cat.starts_with(&prefix_dot) {
                            continue;
                        }
                        for f in files {
                            out.push_str(&format!("{}  ({}, {} bytes)\n", f.path, f.desc, f.size));
                            count += 1;
                        }
                    }
                    if count == 0 {
                        (format!("No files found for module '{module}'"), true)
                    } else {
                        (format!("{count} files in {module}\n\n{out}"), false)
                    }
                }
                "deps" => {
                    // Was cs_get_deps
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let module = args["module"].as_str().unwrap_or("");
                    match repo.deps.get(module) {
                        None => (format!("No dependency info found for '{module}'"), true),
                        Some(dep) => {
                            let mut out =
                                format!("Module: {module}\nCategory: {}\n\n", dep.category_path);
                            if !dep.public.is_empty() {
                                out.push_str("Public dependencies:\n");
                                for d in &dep.public {
                                    out.push_str(&format!("  - {d}\n"));
                                }
                            }
                            if !dep.private.is_empty() {
                                out.push_str("Private dependencies:\n");
                                for d in &dep.private {
                                    out.push_str(&format!("  - {d}\n"));
                                }
                            }
                            (out, false)
                        }
                    }
                }
                _ => {
                    // "list" (default) — was cs_list_modules
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let limit = args["limit"].as_u64().unwrap_or(100).min(1000) as usize;
                    let prefix = args["prefix"].as_str();

                    let mut out = String::new();
                    let mut shown = 0usize;
                    let mut total = 0usize;
                    for (cat, files) in &repo.manifest {
                        if let Some(pfx) = prefix {
                            if !cat.starts_with(pfx) {
                                continue;
                            }
                        }
                        total += 1;
                        if shown < limit {
                            out.push_str(&format!("{cat}  ({} files)\n", files.len()));
                            shown += 1;
                        }
                    }
                    let truncated = if total > shown {
                        format!("\n... and {} more (use prefix filter to narrow)", total - shown)
                    } else {
                        String::new()
                    };
                    (
                        format!(
                            "{total} modules{}\n\n{out}{truncated}",
                            if prefix.is_some() { " matching" } else { " total" }
                        ),
                        false,
                    )
                }
            }
        }

        // =================================================================
        // cs_imports — imports + impact analysis
        // =================================================================
        "cs_imports" => {
            let edge_type_str = args["edge_type"].as_str().unwrap_or("import");

            // Handle non-import edge types via code graph (requires treesitter)
            #[cfg(feature = "treesitter")]
            if edge_type_str != "import" {
                let repo = match resolve_repo(state, &args) {
                    Ok(r) => r,
                    Err(e) => return (format!("Error: {e}"), true),
                };
                let path = args["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    return ("Error: 'path' is required".to_string(), true);
                }
                let direction = args["direction"].as_str().unwrap_or("both");
                let limit = args["limit"].as_u64().unwrap_or(50).min(500) as usize;

                let edge_kind = if edge_type_str == "all" {
                    None
                } else {
                    crate::graph::EdgeKind::parse(edge_type_str)
                };

                let graph_guard = repo.code_graph.read().unwrap();
                let mut out = format!("# {path} — {} edges\n\n", edge_type_str);

                if direction == "both" || direction == "imports" {
                    let outgoing = graph_guard.edges_from(path, edge_kind);
                    if !outgoing.is_empty() {
                        out.push_str(&format!("Outgoing ({} edges):\n", outgoing.len()));
                        for (i, e) in outgoing.iter().enumerate() {
                            if i >= limit { break; }
                            out.push_str(&format!(
                                "  {} -> {}::{} [{}]\n",
                                e.from_symbol, e.to_file, e.to_symbol, e.kind.label()
                            ));
                        }
                        out.push('\n');
                    }
                }

                if direction == "both" || direction == "imported_by" {
                    let incoming = graph_guard.edges_to(path, edge_kind);
                    if !incoming.is_empty() {
                        out.push_str(&format!("Incoming ({} edges):\n", incoming.len()));
                        for (i, e) in incoming.iter().enumerate() {
                            if i >= limit { break; }
                            out.push_str(&format!(
                                "  {}::{} -> {} [{}]\n",
                                e.from_file, e.from_symbol, e.to_symbol, e.kind.label()
                            ));
                        }
                        out.push('\n');
                    }
                }

                if out.lines().count() <= 2 {
                    return (format!("No {edge_type_str} edges found for '{path}'"), false);
                }

                return (out, false);
            }
            #[cfg(not(feature = "treesitter"))]
            if edge_type_str != "import" {
                return (
                    format!("Error: edge_type '{edge_type_str}' requires the treesitter feature. Build with --features treesitter to enable structural edges."),
                    true,
                );
            }

            let transitive = args["transitive"].as_bool().unwrap_or(false);
            if transitive {
                // Impact analysis (was cs_impact)
                let repo = match resolve_repo(state, &args) {
                    Ok(r) => r,
                    Err(e) => return (format!("Error: {e}"), true),
                };
                let path = args["path"].as_str().unwrap_or("");
                let max_depth = args["max_depth"].as_u64().unwrap_or(5).min(20) as usize;
                let file_limit = args["limit"].as_u64().unwrap_or(50).min(500) as usize;

                if path.is_empty() {
                    return ("Error: path is required".to_string(), true);
                }

                let mut visited: HashSet<String> = HashSet::new();
                let mut queue: VecDeque<(String, usize)> = VecDeque::new();
                let mut by_depth: BTreeMap<usize, Vec<String>> = BTreeMap::new();

                visited.insert(path.to_string());
                queue.push_back((path.to_string(), 0));

                while let Some((current, depth)) = queue.pop_front() {
                    if depth > 0 {
                        by_depth.entry(depth).or_default().push(current.clone());
                    }
                    if depth >= max_depth {
                        continue;
                    }
                    if let Some(dependents) = repo.import_graph.imported_by.get(&current) {
                        for dep in dependents {
                            if visited.insert(dep.clone()) {
                                queue.push_back((dep.clone(), depth + 1));
                            }
                        }
                    }
                    for edge in &state.cross_repo_edges {
                        if edge.to_repo == repo.name && edge.to_file == current {
                            let key = format!("[{}] {}", edge.from_repo, edge.from_file);
                            if visited.insert(key.clone()) {
                                by_depth.entry(depth + 1).or_default().push(key);
                            }
                        }
                    }
                }

                let total: usize = by_depth.values().map(|v| v.len()).sum();
                if total == 0 {
                    return (
                        format!("No dependents found for '{path}'. This file is not imported by any other file."),
                        false,
                    );
                }

                let mut out = format!("Impact analysis for {path}\n\n");
                let max_depth_found = *by_depth.keys().max().unwrap_or(&0);
                let mut shown = 0usize;
                for depth in 1..=max_depth_found {
                    if let Some(files) = by_depth.get(&depth) {
                        let label = if depth == 1 { "direct dependents" } else { "" };
                        out.push_str(&format!(
                            "Depth {}{}: {} file{}\n",
                            depth,
                            if label.is_empty() { String::new() } else { format!(" ({label})") },
                            files.len(),
                            if files.len() == 1 { "" } else { "s" }
                        ));
                        for f in files {
                            if shown < file_limit {
                                out.push_str(&format!("  {f}\n"));
                                shown += 1;
                            }
                        }
                        if shown >= file_limit && depth < max_depth_found {
                            out.push_str(&format!("\n  ... output capped at {file_limit} files (use limit param to increase)\n"));
                            break;
                        }
                        out.push('\n');
                    }
                }
                out.push_str(&format!(
                    "Total: {} file{} affected across {} depth level{}",
                    total,
                    if total == 1 { "" } else { "s" },
                    max_depth_found,
                    if max_depth_found == 1 { "" } else { "s" }
                ));
                (out, false)
            } else {
                // Direct imports (was cs_find_imports)
                let repo = match resolve_repo(state, &args) {
                    Ok(r) => r,
                    Err(e) => return (format!("Error: {e}"), true),
                };
                let path = args["path"].as_str().unwrap_or("");
                let direction = args["direction"].as_str().unwrap_or("both");

                let imports: Vec<String> = if direction == "both" || direction == "imports" {
                    repo.import_graph.imports.get(path).cloned().unwrap_or_default()
                } else {
                    vec![]
                };
                let imported_by: Vec<String> = if direction == "both" || direction == "imported_by"
                {
                    repo.import_graph.imported_by.get(path).cloned().unwrap_or_default()
                } else {
                    vec![]
                };

                let mut cross_imports = Vec::new();
                let mut cross_imported_by = Vec::new();
                for edge in &state.cross_repo_edges {
                    if edge.from_repo == repo.name
                        && edge.from_file == path
                        && (direction == "both" || direction == "imports")
                    {
                        cross_imports.push(format!("[{}] {}", edge.to_repo, edge.to_file));
                    }
                    if edge.to_repo == repo.name
                        && edge.to_file == path
                        && (direction == "both" || direction == "imported_by")
                    {
                        cross_imported_by.push(format!("[{}] {}", edge.from_repo, edge.from_file));
                    }
                }

                if imports.is_empty()
                    && imported_by.is_empty()
                    && cross_imports.is_empty()
                    && cross_imported_by.is_empty()
                {
                    return (format!("No import relationships found for '{path}'"), false);
                }

                let mut out = format!("# {path}\n\n");
                if !imports.is_empty() {
                    out.push_str(&format!("Imports ({} files):\n", imports.len()));
                    for inc in &imports {
                        let desc = repo
                            .all_files
                            .iter()
                            .find(|f| f.rel_path == *inc)
                            .map(|f| f.desc.as_str())
                            .unwrap_or("");
                        out.push_str(&format!("  {inc}  ({desc})\n"));
                    }
                    out.push('\n');
                }
                if !cross_imports.is_empty() {
                    out.push_str(&format!("Cross-repo imports ({} files):\n", cross_imports.len()));
                    for inc in &cross_imports {
                        out.push_str(&format!("  {inc}\n"));
                    }
                    out.push('\n');
                }
                if !imported_by.is_empty() {
                    out.push_str(&format!("Imported by ({} files):\n", imported_by.len()));
                    for inc in &imported_by {
                        let desc = repo
                            .all_files
                            .iter()
                            .find(|f| f.rel_path == *inc)
                            .map(|f| f.desc.as_str())
                            .unwrap_or("");
                        out.push_str(&format!("  {inc}  ({desc})\n"));
                    }
                }
                if !cross_imported_by.is_empty() {
                    out.push_str(&format!(
                        "Cross-repo imported by ({} files):\n",
                        cross_imported_by.len()
                    ));
                    for inc in &cross_imported_by {
                        out.push_str(&format!("  {inc}\n"));
                    }
                }
                (out, false)
            }
        }

        // =================================================================
        // cs_search — unified search with semantic fusion
        // =================================================================
        "cs_search" => {
            let repos = resolve_repos_for_search(state, &args);
            if repos.is_empty() {
                return ("Error: No matching repos found".to_string(), true);
            }
            let multi = repos.len() > 1;

            let raw_query = args["query"].as_str().unwrap_or("");
            if raw_query.is_empty() {
                return ("Error: Query must not be empty".to_string(), true);
            }
            let file_limit =
                args["fileLimit"].as_u64().unwrap_or(args["limit"].as_u64().unwrap_or(30)).min(100)
                    as usize;
            let module_limit = args["moduleLimit"].as_u64().unwrap_or(5).min(50) as usize;
            let ext_filter: Option<HashSet<String>> = args["ext"].as_str().map(|exts| {
                exts.split(',').map(|e| e.trim().trim_start_matches('.').to_string()).collect()
            });
            let cat_filter = args["category"].as_str().map(|s| s.to_string());
            let path_filter = args["path"].as_str();
            let match_mode = args["match_mode"].as_str().unwrap_or("all");

            let start = std::time::Instant::now();

            // Content grep pattern
            let terms: Vec<&str> = raw_query.split_whitespace().collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
            let require_all_terms = match_mode == "all" && terms.len() > 1;

            let pattern = match match_mode {
                "exact" => {
                    RegexBuilder::new(&regex::escape(raw_query)).case_insensitive(true).build()
                }
                "regex" => RegexBuilder::new(raw_query).case_insensitive(true).build(),
                _ => {
                    let pattern_str =
                        terms.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");
                    RegexBuilder::new(&pattern_str).case_insensitive(true).build()
                }
            };

            struct FindResult {
                display_path: String,
                desc: String,
                name_score: f64,
                grep_score: f64,
                grep_count: usize,
                top_match: Option<String>,
                terms_matched: usize,
                total_terms: usize,
            }

            let mut merged: std::collections::HashMap<String, FindResult> =
                std::collections::HashMap::new();
            let mut all_modules: Vec<(&RepoState, crate::fuzzy::SearchModuleResult)> = Vec::new();

            for repo in &repos {
                let config = &repo.config;

                // 1. Fuzzy filename search
                let query = crate::fuzzy::preprocess_search_query(raw_query);
                let search_resp = run_search(
                    &repo.search_files,
                    &repo.search_modules,
                    &query,
                    file_limit,
                    module_limit,
                );

                for m in search_resp.modules {
                    all_modules.push((repo, m));
                }

                for f in &search_resp.files {
                    if let Some(prefix) = path_filter {
                        if !f.path.starts_with(prefix) {
                            continue;
                        }
                    }
                    if let Some(ref exts) = ext_filter {
                        let ext = f.ext.trim_start_matches('.');
                        if !exts.contains(ext) {
                            continue;
                        }
                    }
                    if let Some(ref cat) = cat_filter {
                        if !f.category.starts_with(cat.as_str()) {
                            continue;
                        }
                    }
                    let key = repo_path(repo, &f.path, multi);
                    merged.insert(
                        key.clone(),
                        FindResult {
                            display_path: key,
                            desc: f.desc.clone(),
                            name_score: f.score,
                            grep_score: 0.0,
                            grep_count: 0,
                            top_match: None,
                            terms_matched: 0,
                            total_terms: terms_lower.len(),
                        },
                    );
                }

                // 2. Content grep
                if let Ok(ref pattern) = pattern {
                    let idf_weights: Vec<f64> =
                        terms_lower.iter().map(|t| repo.term_doc_freq.idf(t)).collect();
                    let candidates: Vec<&ScannedFile> = repo
                        .all_files
                        .iter()
                        .filter(|f| {
                            if let Some(prefix) = path_filter {
                                if !f.rel_path.starts_with(prefix) {
                                    return false;
                                }
                            }
                            if let Some(ref exts) = ext_filter {
                                if !exts.contains(&f.ext) {
                                    return false;
                                }
                            }
                            if let Some(ref cat) = cat_filter {
                                let file_cat = get_category_path(&f.rel_path, config).join(" > ");
                                if !file_cat.starts_with(cat.as_str()) {
                                    return false;
                                }
                            }
                            true
                        })
                        .collect();

                    use rayon::prelude::*;
                    let grep_results: Vec<_> = candidates
                        .par_iter()
                        .filter_map(|file| {
                            let content = fs::read_to_string(&file.abs_path).ok()?;
                            let lines: Vec<&str> = content.lines().collect();
                            let total_lines = lines.len().max(1);
                            let mut match_count = 0usize;
                            let mut best_snippet: Option<String> = None;
                            let mut best_snippet_term_count: usize = 0;
                            let mut first_match_line_idx = usize::MAX;
                            let mut terms_seen = std::collections::HashSet::new();
                            for (i, line) in lines.iter().enumerate() {
                                if !pattern.is_match(line) {
                                    continue;
                                }
                                let line_lower = line.to_lowercase();
                                if require_all_terms
                                    && !terms_lower.iter().all(|t| line_lower.contains(t.as_str()))
                                {
                                    continue;
                                }
                                match_count += 1;
                                if first_match_line_idx == usize::MAX {
                                    first_match_line_idx = i;
                                }
                                let line_term_count = terms_lower
                                    .iter()
                                    .filter(|t| line_lower.contains(t.as_str()))
                                    .count();
                                for (ti, term) in terms_lower.iter().enumerate() {
                                    if line_lower.contains(term.as_str()) {
                                        terms_seen.insert(ti);
                                    }
                                }
                                if line_term_count > best_snippet_term_count {
                                    best_snippet_term_count = line_term_count;
                                    let trimmed = if line.len() > 120 {
                                        format!("{}...", &line[..line.floor_char_boundary(120)])
                                    } else {
                                        line.to_string()
                                    };
                                    best_snippet = Some(trimmed);
                                }
                            }
                            if match_count == 0 {
                                return None;
                            }

                            let filename = file
                                .rel_path
                                .rsplit('/')
                                .next()
                                .unwrap_or(&file.rel_path)
                                .to_lowercase();
                            let grep_score = grep_relevance_score(
                                match_count,
                                total_lines,
                                &filename,
                                &file.ext,
                                &terms_lower,
                                terms_seen.len(),
                                if first_match_line_idx == usize::MAX {
                                    0
                                } else {
                                    first_match_line_idx
                                },
                                &idf_weights,
                            );

                            let key = repo_path(repo, &file.rel_path, multi);
                            Some((
                                key,
                                file.desc.clone(),
                                grep_score,
                                match_count,
                                best_snippet,
                                terms_seen.len(),
                            ))
                        })
                        .collect();

                    for (key, desc, grep_score, match_count, best_snippet, terms_matched) in
                        grep_results
                    {
                        let entry = merged.entry(key.clone()).or_insert_with(|| FindResult {
                            display_path: key,
                            desc,
                            name_score: 0.0,
                            grep_score: 0.0,
                            grep_count: 0,
                            top_match: None,
                            terms_matched: 0,
                            total_terms: terms_lower.len(),
                        });
                        entry.grep_score = grep_score;
                        entry.grep_count = match_count;
                        entry.top_match = best_snippet;
                        entry.terms_matched = terms_matched;
                    }
                }
            }

            // Record search query in session
            if let Some(ref mut s) = session {
                s.record_query(raw_query);
            }

            // Unified scoring — adaptive weights with score normalization
            let (name_w, grep_w) = if terms.len() > 1 { (0.4, 0.6) } else { (0.6, 0.4) };
            let mut ranked: Vec<FindResult> = merged.into_values().collect();

            let max_name = ranked.iter().map(|r| r.name_score).fold(0.0f64, f64::max).max(1.0);
            let max_grep = ranked.iter().map(|r| r.grep_score).fold(0.0f64, f64::max).max(1.0);

            // Build frontier set for boosting
            let frontier_set: HashSet<String> = session
                .as_ref()
                .map(|s| s.exploration_frontier.clone())
                .unwrap_or_default();

            ranked.sort_by(|a, b| {
                let norm_a =
                    (a.name_score / max_name) * name_w + (a.grep_score / max_grep) * grep_w;
                let norm_b =
                    (b.name_score / max_name) * name_w + (b.grep_score / max_grep) * grep_w;
                let boost_a = if a.name_score > 0.0 && a.grep_count > 0 { 1.25 } else { 1.0 };
                let boost_b = if b.name_score > 0.0 && b.grep_count > 0 { 1.25 } else { 1.0 };
                // Frontier boost: files adjacent to already-read files get 1.1x
                let frontier_a = if frontier_set.contains(&a.display_path) { 1.1 } else { 1.0 };
                let frontier_b = if frontier_set.contains(&b.display_path) { 1.1 } else { 1.0 };
                (norm_b * boost_b * frontier_b)
                    .partial_cmp(&(norm_a * boost_a * frontier_a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ranked.truncate(file_limit);

            // Semantic fusion via Reciprocal Rank Fusion (RRF).
            // RRF is rank-based, sidestepping the score normalization problem between
            // keyword scores and cosine similarity. Formula: score = Σ 1/(k + rank).
            #[cfg(feature = "semantic")]
            let has_semantic = {
                let mut fused = false;
                for repo in &repos {
                    let sem_guard = repo.semantic_index.read().unwrap();
                    if let Some(ref index) = *sem_guard {
                        let sem_limit = file_limit * 2;
                        if let Ok(sem_results) =
                            crate::semantic::semantic_search(index, raw_query, sem_limit)
                        {
                            if !sem_results.is_empty() {
                                fused = true;
                                const RRF_K: f64 = 60.0;

                                // Build keyword rank map (ranked is already sorted by score)
                                let keyword_map: std::collections::HashMap<
                                    String,
                                    (usize, &FindResult),
                                > = ranked
                                    .iter()
                                    .enumerate()
                                    .map(|(i, r)| (r.display_path.clone(), (i + 1, r)))
                                    .collect();

                                // Build semantic rank map + metadata
                                use crate::semantic::SemanticSearchResult;
                                let sem_map: std::collections::HashMap<
                                    String,
                                    (usize, &SemanticSearchResult),
                                > = sem_results
                                    .iter()
                                    .enumerate()
                                    .map(|(i, sr)| {
                                        (repo_path(repo, &sr.file_path, multi), (i + 1, sr))
                                    })
                                    .collect();

                                // Collect all unique paths from both rankings
                                let all_paths: HashSet<String> =
                                    keyword_map.keys().chain(sem_map.keys()).cloned().collect();

                                // Compute RRF scores and rebuild ranked list
                                let mut rrf_ranked: Vec<(f64, FindResult)> = all_paths
                                    .into_iter()
                                    .map(|path| {
                                        let kw_rrf = keyword_map
                                            .get(&path)
                                            .map(|(rank, _)| 1.0 / (RRF_K + *rank as f64))
                                            .unwrap_or(0.0);
                                        let sem_rrf = sem_map
                                            .get(&path)
                                            .map(|(rank, _)| 1.0 / (RRF_K + *rank as f64))
                                            .unwrap_or(0.0);
                                        let rrf_score = kw_rrf + sem_rrf;

                                        // Build result entry from best available source
                                        let entry = if let Some((_, kw_result)) =
                                            keyword_map.get(&path)
                                        {
                                            // Keyword result exists — clone it
                                            FindResult {
                                                display_path: path,
                                                desc: kw_result.desc.clone(),
                                                name_score: kw_result.name_score,
                                                grep_score: kw_result.grep_score,
                                                grep_count: kw_result.grep_count,
                                                top_match: kw_result.top_match.clone(),
                                                terms_matched: kw_result.terms_matched,
                                                total_terms: kw_result.total_terms,
                                            }
                                        } else if let Some((_, sr)) = sem_map.get(&path) {
                                            // Semantic-only result
                                            let file_desc = repo
                                                .all_files
                                                .iter()
                                                .find(|f| f.rel_path == sr.file_path)
                                                .map(|f| f.desc.as_str())
                                                .unwrap_or("");
                                            let desc = if file_desc.is_empty() {
                                                format!("line ~{}", sr.start_line)
                                            } else {
                                                format!("{} (line ~{})", file_desc, sr.start_line)
                                            };
                                            let preview = sr
                                                .snippet
                                                .lines()
                                                .find(|l| {
                                                    let t = l.trim();
                                                    !t.is_empty() && !t.starts_with("// File:")
                                                })
                                                .unwrap_or("")
                                                .to_string();
                                            FindResult {
                                                display_path: path,
                                                desc,
                                                name_score: 0.0,
                                                grep_score: 0.0,
                                                grep_count: 0,
                                                top_match: Some(preview),
                                                terms_matched: 0,
                                                total_terms: terms_lower.len(),
                                            }
                                        } else {
                                            unreachable!()
                                        };
                                        (rrf_score, entry)
                                    })
                                    .collect();

                                rrf_ranked.sort_by(|a, b| {
                                    b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
                                });
                                rrf_ranked.truncate(file_limit);
                                ranked = rrf_ranked.into_iter().map(|(_, r)| r).collect();
                            }
                        }
                    }
                }
                fused
            };
            #[cfg(not(feature = "semantic"))]
            let has_semantic = false;

            let query_time = start.elapsed().as_millis();

            // Compute confidence and coverage for structured output
            let top_score = ranked
                .first()
                .map(|r| r.name_score.max(r.grep_score))
                .unwrap_or(0.0);
            let confidence = if ranked.len() >= 3 && top_score > 10.0 {
                "high"
            } else if !ranked.is_empty() && top_score > 3.0 {
                "medium"
            } else {
                "low"
            };
            let coverage = if terms_lower.len() <= 1 {
                "full"
            } else {
                let best_match = ranked
                    .first()
                    .map(|r| r.terms_matched)
                    .unwrap_or(0);
                if best_match == terms_lower.len() {
                    "full"
                } else if best_match > 0 {
                    "partial"
                } else {
                    "none"
                }
            };
            let frontier_count = frontier_set.len();

            let mut out = format!(
                "Found {} results for \"{}\" ({query_time}ms{})\nConfidence: {confidence} | Coverage: {coverage}{}",
                ranked.len(),
                raw_query,
                if has_semantic { ", semantic+keyword" } else { "" },
                if frontier_count > 0 {
                    format!(" | Frontier: {} unexplored neighbor{}", frontier_count, if frontier_count == 1 { "" } else { "s" })
                } else {
                    String::new()
                },
            );
            if confidence == "low" && !ranked.is_empty() {
                out.push_str("\nTip: Try more specific terms or use cs_grep for exact pattern matching.");
            }
            out.push_str("\n\n");

            // Module results
            if !all_modules.is_empty() {
                all_modules.sort_by(|a, b| {
                    b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal)
                });
                all_modules.truncate(module_limit);
                out.push_str("Modules:\n");
                for (repo, m) in &all_modules {
                    let prefix = if multi { format!("[{}] ", repo.name) } else { String::new() };
                    out.push_str(&format!(
                        "  {prefix}{} ({} files, score {:.1})\n",
                        m.id, m.file_count, m.score
                    ));
                }
                out.push('\n');
            }

            // File results
            for r in &ranked {
                let has_name = r.name_score > 0.0;
                let has_content = r.grep_count > 0;

                // Determine source tag
                // When semantic fusion is active, tag results by source:
                // - [semantic]: only found via semantic search (no keyword match)
                // - [both]: found by both semantic and keyword search
                // - [keyword]: found by keyword/filename only
                let source = if has_semantic {
                    if !has_name && !has_content {
                        "[semantic]"
                    } else {
                        "[both]"
                    }
                } else {
                    match (has_name, has_content) {
                        (true, true) => "name+content",
                        (true, false) => "name",
                        (false, true) => "content",
                        (false, false) => "",
                    }
                };

                let tag_str = if source.is_empty() {
                    String::new()
                } else if r.total_terms > 1 && has_content {
                    format!(
                        " [{}, {}/{} terms, {} lines]",
                        source, r.terms_matched, r.total_terms, r.grep_count
                    )
                } else if has_content {
                    format!(" [{}, {} lines]", source, r.grep_count)
                } else {
                    format!(" [{}]", source)
                };
                out.push_str(&format!("  {} — {}{tag_str}\n", r.display_path, r.desc));
                if let Some(ref line) = r.top_match {
                    out.push_str(&format!("    > {}\n", line.trim()));
                }
            }

            (out, false)
        }

        // =================================================================
        // cs_git — blame/history/changed/hotspots
        // =================================================================
        "cs_git" => {
            let action = args["action"].as_str().unwrap_or("");
            match action {
                "blame" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let path = args["path"].as_str().unwrap_or("");
                    if path.is_empty() {
                        return ("Error: 'path' is required".to_string(), true);
                    }
                    let start_line = args["start_line"].as_u64().map(|n| n as usize);
                    let end_line = args["end_line"].as_u64().map(|n| n as usize);

                    match crate::git::blame(&repo.root, path, start_line, end_line) {
                        Ok(lines) => {
                            if lines.is_empty() {
                                return (format!("No blame data for '{path}'"), false);
                            }
                            let range_str = match (start_line, end_line) {
                                (Some(s), Some(e)) => format!(" (lines {s}-{e})"),
                                (Some(s), None) => format!(" (from line {s})"),
                                (None, Some(e)) => format!(" (to line {e})"),
                                _ => String::new(),
                            };
                            let mut out = format!("# {path}{range_str}\n\n");
                            let width = lines.last().map(|l| format!("{}", l.line).len()).unwrap_or(1);
                            for bl in &lines {
                                out.push_str(&format!(
                                    "{:>w$}: {} | {} | {} | {}\n",
                                    bl.line,
                                    bl.commit,
                                    bl.author,
                                    bl.date,
                                    bl.content,
                                    w = width
                                ));
                            }
                            out.push_str(&format!("\n{} lines", lines.len()));
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                "history" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let path = args["path"].as_str().unwrap_or("");
                    if path.is_empty() {
                        return ("Error: 'path' is required".to_string(), true);
                    }
                    let limit = args["limit"].as_u64().unwrap_or(10).min(100) as usize;

                    match crate::git::file_history(&repo.root, path, limit) {
                        Ok(commits) => {
                            if commits.is_empty() {
                                return (format!("No commit history found for '{path}'"), false);
                            }
                            let mut out = format!("# {path} — {} recent commits\n\n", commits.len());
                            for c in &commits {
                                out.push_str(&format!(
                                    "{} | {} | {} | {}\n",
                                    c.hash, c.author, c.date, c.message
                                ));
                                if c.files_changed.len() > 1 {
                                    let others: Vec<&str> = c
                                        .files_changed
                                        .iter()
                                        .filter(|f| f.as_str() != path)
                                        .map(|f| f.as_str())
                                        .take(10)
                                        .collect();
                                    if !others.is_empty() {
                                        out.push_str(&format!("  also: {}\n", others.join(", ")));
                                    }
                                }
                            }
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                "changed" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let since = args["since"].as_str().unwrap_or("");
                    if since.is_empty() {
                        return ("Error: 'since' is required".to_string(), true);
                    }

                    match crate::git::changed_since(&repo.root, since) {
                        Ok(files) => {
                            if files.is_empty() {
                                return (format!("No changes since '{since}'"), false);
                            }
                            let mut out = format!("Files changed since {since}: {}\n\n", files.len());
                            let mut by_status: BTreeMap<String, Vec<&str>> = BTreeMap::new();
                            for f in &files {
                                by_status.entry(f.status.clone()).or_default().push(&f.path);
                            }
                            for (status, paths) in &by_status {
                                out.push_str(&format!("{} ({}):\n", status, paths.len()));
                                for p in paths {
                                    out.push_str(&format!("  {p}\n"));
                                }
                                out.push('\n');
                            }
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                "hotspots" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let limit = args["limit"].as_u64().unwrap_or(20).min(200) as usize;
                    let days = args["days"].as_u64().unwrap_or(90).min(365) as usize;

                    match crate::git::hot_files(&repo.root, limit, days) {
                        Ok(files) => {
                            if files.is_empty() {
                                return (format!("No file changes found in the last {days} days"), false);
                            }
                            let mut out = format!("Hot files (last {days} days, top {})\n\n", files.len());
                            let max_commits = files.first().map(|f| f.commits).unwrap_or(1);
                            let width = format!("{}", max_commits).len();
                            for (i, f) in files.iter().enumerate() {
                                out.push_str(&format!(
                                    "{:>3}. {:>w$} commits  {}\n",
                                    i + 1,
                                    f.commits,
                                    f.path,
                                    w = width
                                ));
                            }
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                "evolution" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let path = args["path"].as_str().unwrap_or("");
                    if path.is_empty() {
                        return ("Error: 'path' is required for evolution".to_string(), true);
                    }
                    let limit = args["limit"].as_u64().unwrap_or(10).min(100) as usize;

                    // Resolve start_line/end_line: prefer explicit params, fall back to AST symbol lookup
                    #[allow(unused_mut)]
                    let mut start_line = args["start_line"].as_u64().map(|n| n as usize);
                    #[allow(unused_mut)]
                    let mut end_line = args["end_line"].as_u64().map(|n| n as usize);

                    #[cfg(feature = "treesitter")]
                    if start_line.is_none() || end_line.is_none() {
                        if let Some(symbol_name) = args["symbol"].as_str() {
                            let ast_guard = repo.ast_index.read().unwrap();
                            if let Some(file_ast) = ast_guard.get(path) {
                                let matches = file_ast.find(symbol_name);
                                if let Some(sym) = matches.first() {
                                    start_line = Some(sym.start_line);
                                    end_line = Some(sym.end_line);
                                }
                            }
                        }
                    }

                    let start = match start_line {
                        Some(s) => s,
                        None => return ("Error: 'start_line' is required (or provide 'symbol' with treesitter feature)".to_string(), true),
                    };
                    let end = match end_line {
                        Some(e) => e,
                        None => return ("Error: 'end_line' is required (or provide 'symbol' with treesitter feature)".to_string(), true),
                    };

                    match crate::git::symbol_evolution(&repo.root, path, start, end, limit) {
                        Ok(commits) => {
                            if commits.is_empty() {
                                return (format!("No commits found touching {path} lines {start}-{end}"), false);
                            }
                            let mut out = format!("# {path} (lines {start}-{end}) — {} commits\n\n", commits.len());
                            for c in &commits {
                                out.push_str(&format!(
                                    "{} | {} | {} | {}\n",
                                    c.hash, c.author, c.date, c.message
                                ));
                                if c.files_changed.len() > 1 {
                                    let others: Vec<&str> = c
                                        .files_changed
                                        .iter()
                                        .filter(|f| f.as_str() != path)
                                        .map(|f| f.as_str())
                                        .take(10)
                                        .collect();
                                    if !others.is_empty() {
                                        out.push_str(&format!("  also: {}\n", others.join(", ")));
                                    }
                                }
                            }
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                "cochange" => {
                    let repo = match resolve_repo(state, &args) {
                        Ok(r) => r,
                        Err(e) => return (format!("Error: {e}"), true),
                    };
                    let path = args["path"].as_str().unwrap_or("");
                    if path.is_empty() {
                        return ("Error: 'path' is required for cochange".to_string(), true);
                    }
                    let days = args["days"].as_u64().unwrap_or(90).min(365) as usize;
                    let limit = args["limit"].as_u64().unwrap_or(20).min(200) as usize;

                    match crate::git::cochanged_files(&repo.root, path, days, limit) {
                        Ok(entries) => {
                            if entries.is_empty() {
                                return (format!("No cochanged files found for '{path}' in the last {days} days"), false);
                            }
                            let mut out = format!("# {path} — temporal coupling (last {days} days)\n\n");
                            out.push_str(&format!("{:<4} {:<8} {:<8} {}\n", "Rank", "Count", "Ratio", "File"));
                            out.push_str(&format!("{}\n", "-".repeat(60)));
                            for (i, e) in entries.iter().enumerate() {
                                out.push_str(&format!(
                                    "{:<4} {:<8} {:<8.2} {}\n",
                                    i + 1,
                                    e.cochange_count,
                                    e.coupling_ratio,
                                    e.path
                                ));
                            }
                            (out, false)
                        }
                        Err(e) => (format!("Error: {e}"), true),
                    }
                }
                _ => (format!("Error: Unknown cs_git action '{action}'. Use: blame, history, changed, hotspots, evolution, cochange"), true),
            }
        }

        // =================================================================
        // cs_status — merged status + session info
        // =================================================================
        "cs_status" => {
            let version = env!("CARGO_PKG_VERSION");
            let repo_count = state.repos.len();
            let mut out = format!(
                "CodeScope v{version} — {repo_count} repositor{} indexed\n\n",
                if repo_count == 1 { "y" } else { "ies" }
            );

            let mut total_files = 0usize;
            for repo in state.repos.values() {
                let file_count = repo.all_files.len();
                total_files += file_count;

                out.push_str(&format!(
                    "[{}] {}\n  Files: {} | Modules: {} | Import edges: {}\n",
                    repo.name,
                    repo.root.display(),
                    file_count,
                    repo.manifest.len(),
                    repo.import_graph.imports.len(),
                ));

                // Language breakdown
                let mut ext_counts: BTreeMap<String, usize> = BTreeMap::new();
                for f in &repo.all_files {
                    if !f.ext.is_empty() {
                        *ext_counts.entry(f.ext.clone()).or_default() += 1;
                    }
                }
                let mut sorted_exts: Vec<(String, usize)> = ext_counts.into_iter().collect();
                sorted_exts.sort_by(|a, b| b.1.cmp(&a.1));
                sorted_exts.truncate(8);

                let lang_str: Vec<String> = sorted_exts
                    .iter()
                    .map(|(ext, count)| {
                        if *count >= 1000 {
                            format!("{ext}({:.0}K)", *count as f64 / 1000.0)
                        } else {
                            format!("{ext}({count})")
                        }
                    })
                    .collect();
                if !lang_str.is_empty() {
                    out.push_str(&format!("  Languages: {}\n", lang_str.join(" ")));
                }
                out.push_str(&format!("  Last scan: {}ms\n", repo.scan_time_ms));

                #[cfg(feature = "semantic")]
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    let sp = &repo.semantic_progress;
                    let status = sp.status_label();
                    match sp.status.load(Relaxed) {
                        0 => out.push_str("  Semantic: disabled\n"),
                        1 => {
                            out.push_str("  Semantic: extracting chunks...\n");
                        }
                        2 => {
                            let done = sp.completed_batches.load(Relaxed);
                            let total = sp.total_batches.load(Relaxed);
                            let chunks = sp.total_chunks.load(Relaxed);
                            let device = sp.device.read().unwrap();
                            let pct = if total > 0 { done * 100 / total } else { 0 };
                            out.push_str(&format!(
                                "  Semantic: embedding on {device} — {done}/{total} batches ({pct}%), {chunks} chunks\n",
                            ));
                        }
                        3 => {
                            let chunks = sp.total_chunks.load(Relaxed);
                            let device = sp.device.read().unwrap();
                            out.push_str(&format!(
                                "  Semantic: ready ({chunks} chunks, {device})\n",
                            ));
                        }
                        4 => out.push_str("  Semantic: failed\n"),
                        _ => out.push_str(&format!("  Semantic: {status}\n")),
                    }
                }

                out.push('\n');
            }

            if !state.cross_repo_edges.is_empty() {
                out.push_str(&format!(
                    "Cross-repo: {} import edges\n\n",
                    state.cross_repo_edges.len()
                ));
            }

            out.push_str(&format!("Total: {} files across {} repo(s)", total_files, repo_count));

            // Append session info (was cs_session_info)
            if let Some(ref s) = session {
                let elapsed = s.started_at.elapsed();
                let mins = elapsed.as_secs() / 60;
                let secs = elapsed.as_secs() % 60;
                out.push_str(&format!(
                    "\n\nSession: {}m {}s, {} files read, ~{} tokens served",
                    mins,
                    secs,
                    s.files_read.len(),
                    s.total_tokens_served
                ));
                if !s.files_read.is_empty() {
                    out.push_str("\nFiles read:\n");
                    let mut sorted: Vec<(&String, &std::time::Instant)> =
                        s.files_read.iter().collect();
                    sorted.sort_by_key(|(_, t)| *t);
                    for (path, _) in sorted {
                        out.push_str(&format!("  {path}\n"));
                    }
                }
            }

            (out, false)
        }

        _ => (format!("Unknown tool: {name}"), true),
    }
}

// ---------------------------------------------------------------------------
// Mutating tool handlers (need write lock)
// ---------------------------------------------------------------------------

fn handle_rescan(state: &mut ServerState, args: &serde_json::Value) -> (String, bool) {
    let target_repo = args.get("repo").and_then(|v| v.as_str());
    let tok = state.tokenizer.clone();

    let repos_to_scan: Vec<String> = match target_repo {
        Some(name) => {
            if state.repos.contains_key(name) {
                vec![name.to_string()]
            } else {
                return (format!("Error: Unknown repo '{name}'"), true);
            }
        }
        None => state.repos.keys().cloned().collect(),
    };

    let mut results = Vec::new();
    for name in &repos_to_scan {
        let root = state.repos[name].root.clone();
        let new_state = crate::scan_repo(name, &root, &tok);
        results.push(format!(
            "[{name}] Rescanned: {} files, {} modules, {} import edges ({}ms)",
            new_state.all_files.len(),
            new_state.manifest.len(),
            new_state.import_graph.imports.len(),
            new_state.scan_time_ms,
        ));
        state.repos.insert(name.clone(), new_state);
    }

    // Rebuild cross-repo edges
    state.cross_repo_edges = crate::scan::resolve_cross_repo_imports(&state.repos);

    (results.join("\n"), false)
}

fn handle_add_repo(state: &mut ServerState, args: &serde_json::Value) -> (String, bool) {
    let name = match args["name"].as_str() {
        Some(n) => n.to_string(),
        None => return ("Error: 'name' is required".to_string(), true),
    };
    let root_str = match args["root"].as_str() {
        Some(r) => r,
        None => return ("Error: 'root' is required".to_string(), true),
    };
    let root = match std::path::PathBuf::from(root_str).canonicalize() {
        Ok(r) => r,
        Err(e) => return (format!("Error: Path not found: {e}"), true),
    };

    if state.repos.contains_key(&name) {
        return (format!("Error: Repo '{name}' already exists. Use cs_rescan to update it."), true);
    }

    let tok = state.tokenizer.clone();
    let new_state = crate::scan_repo(&name, &root, &tok);
    let summary = format!(
        "Added [{name}] {}: {} files, {} modules, {} import edges ({}ms)",
        root.display(),
        new_state.all_files.len(),
        new_state.manifest.len(),
        new_state.import_graph.imports.len(),
        new_state.scan_time_ms,
    );

    // Spawn background semantic indexing for the new repo
    #[cfg(feature = "semantic")]
    let semantic_summary = if state.semantic_enabled {
        let files = new_state.all_files.clone();
        let sem_handle = std::sync::Arc::clone(&new_state.semantic_index);
        let progress = std::sync::Arc::clone(&new_state.semantic_progress);
        let repo_root = root.clone();
        let model = state.semantic_model.clone();
        #[cfg(feature = "treesitter")]
        let ast_idx_clone = {
            let guard = new_state.ast_index.read().unwrap();
            guard.clone()
        };
        let thread_name = name.clone();
        std::thread::spawn(move || {
            tracing::info!(repo = thread_name.as_str(), "Building semantic index in background");
            let sem_start = std::time::Instant::now();
            if let Some(idx) = crate::semantic::build_semantic_index(
                &files,
                model.as_deref(),
                &progress,
                &repo_root,
                #[cfg(feature = "treesitter")]
                Some(&ast_idx_clone),
            ) {
                tracing::info!(
                    repo = thread_name.as_str(),
                    chunks = idx.chunk_meta.len(),
                    time_ms = sem_start.elapsed().as_millis() as u64,
                    "Semantic index ready"
                );
                *sem_handle.write().unwrap() = Some(idx);
            }
        });
        " Semantic indexing started in background."
    } else {
        ""
    };
    #[cfg(not(feature = "semantic"))]
    let semantic_summary = "";

    state.repos.insert(name.clone(), new_state);

    // Persist to global ~/.codescope/repos.toml so the repo survives server restarts
    let persist_note = match crate::merge_global_repos_toml(&name, &root) {
        Ok(()) => " Saved to ~/.codescope/repos.toml.",
        Err(e) => {
            tracing::warn!(repo = name.as_str(), error = %e, "Failed to persist repo to global config");
            ""
        }
    };

    // Rebuild cross-repo edges
    state.cross_repo_edges = crate::scan::resolve_cross_repo_imports(&state.repos);

    (format!("{summary}{semantic_summary}{persist_note}"), false)
}

// ---------------------------------------------------------------------------
// Protocol version negotiation
// ---------------------------------------------------------------------------

pub(crate) const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];
pub(crate) const LATEST_VERSION: &str = "2025-11-25";

/// Negotiate protocol version: echo client's version if supported, else return latest.
pub(crate) fn negotiate_version(client_version: &str) -> &'static str {
    if SUPPORTED_VERSIONS.contains(&client_version) {
        // Return the matching static str
        SUPPORTED_VERSIONS.iter().find(|&&v| v == client_version).copied().unwrap()
    } else {
        LATEST_VERSION
    }
}

// ---------------------------------------------------------------------------
// Shared JSON-RPC dispatch (used by both stdio and HTTP transports)
// ---------------------------------------------------------------------------

/// Process a single JSON-RPC request and return the response.
///
/// Returns `None` for notifications (no `id` field).
/// The `initialized` flag is checked by the caller — this function assumes
/// the request has already passed init enforcement.
pub fn dispatch_jsonrpc(
    state: &Arc<RwLock<ServerState>>,
    msg: &serde_json::Value,
    session: &mut Option<SessionState>,
) -> Option<serde_json::Value> {
    let method = msg["method"].as_str().unwrap_or("");
    let id = msg.get("id").cloned();

    // Notifications have no id and produce no response
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }

    let response = match method {
        "initialize" => {
            let client_version = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let negotiated = negotiate_version(client_version);
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "prompts": { "listChanged": false },
                        "resources": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "codescope",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": "CodeScope — search, browse, and read source code. Start with cs_search for discovery (uses semantic search when available, keyword matching as fallback). Use cs_grep for exact pattern matching. Use cs_read to read files. Use cs_imports to trace dependencies. Use cs_git for history analysis."
                }
            })
        }
        "tools/list" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": tool_definitions()
                }
            })
        }
        "tools/call" => {
            let tool_name = msg["params"]["name"].as_str().unwrap_or("");
            let arguments =
                msg["params"].get("arguments").cloned().unwrap_or(serde_json::json!({}));

            // Mutating tools need write lock
            let (text, is_error) = match tool_name {
                "cs_rescan" | "cs_add_repo" => {
                    let mut s = state.write().unwrap();
                    match tool_name {
                        "cs_rescan" => handle_rescan(&mut s, &arguments),
                        "cs_add_repo" => handle_add_repo(&mut s, &arguments),
                        _ => unreachable!(),
                    }
                }
                _ => {
                    let s = state.read().unwrap();
                    handle_tool_call(&s, tool_name, &arguments, session)
                }
            };

            // Never set isError: true — it triggers Claude Code's sibling tool call
            // cascade failure (all parallel calls get killed). Instead, prefix the
            // error message so the LLM can still detect and recover from failures.
            let content_text = if is_error { format!("\u{26a0} Error: {text}") } else { text };
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": content_text }],
                    "isError": false
                }
            })
        }
        "prompts/list" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "prompts": prompts_list()
                }
            })
        }
        "prompts/get" => {
            let prompt_name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(serde_json::json!({}));
            let state_r = state.read().unwrap();
            match get_prompt(&state_r, prompt_name, &args) {
                Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(e) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": e } }),
            }
        }
        "resources/list" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "resources": resources_list() }
            })
        }
        "resources/read" => {
            let uri = msg["params"]["uri"].as_str().unwrap_or("");
            let state_r = state.read().unwrap();
            match read_resource(&state_r, uri) {
                Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(e) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": e } }),
            }
        }
        "ping" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })
        }
        _ => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "Method not found" }
            })
        }
    };

    Some(response)
}

// ---------------------------------------------------------------------------
// MCP prompts and resources
// ---------------------------------------------------------------------------

fn prompts_list() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "implement-feature",
            "description": "Get context for implementing a new feature",
            "arguments": [
                { "name": "description", "description": "Feature description", "required": true }
            ]
        },
        {
            "name": "debug-error",
            "description": "Get context for debugging an error",
            "arguments": [
                { "name": "error_text", "description": "Error message or stack trace", "required": true },
                { "name": "file_path", "description": "File where error occurred", "required": false }
            ]
        },
        {
            "name": "write-tests",
            "description": "Get context for writing tests for a file",
            "arguments": [
                { "name": "file_path", "description": "File to write tests for", "required": true }
            ]
        },
        {
            "name": "review-code",
            "description": "Review code changes with project context",
            "arguments": [
                { "name": "diff", "description": "Code diff to review", "required": false }
            ]
        }
    ])
}

fn get_prompt(
    state: &ServerState,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let repo = state.default_repo();
    let conventions = crate::conventions::mine_conventions(&repo.all_files);
    let conv_text = crate::conventions::format_conventions(&conventions);

    match name {
        "implement-feature" => {
            let desc = args["description"].as_str().unwrap_or("");
            Ok(serde_json::json!({
                "description": format!("Context for implementing: {desc}"),
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": format!(
                        "I want to implement: {desc}\n\nProject conventions:\n{conv_text}\n\nPlease help me implement this feature following the project's conventions."
                    )}
                }]
            }))
        }
        "debug-error" => {
            let error_text = args["error_text"].as_str().unwrap_or("");
            let file_path = args["file_path"].as_str().unwrap_or("unknown");
            Ok(serde_json::json!({
                "description": format!("Context for debugging: {error_text}"),
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": format!(
                        "I'm debugging this error:\n```\n{error_text}\n```\n\nFile: {file_path}\n\nProject conventions:\n{conv_text}\n\nPlease help me debug this error."
                    )}
                }]
            }))
        }
        "write-tests" => {
            let file_path = args["file_path"].as_str().unwrap_or("");
            Ok(serde_json::json!({
                "description": format!("Context for writing tests: {file_path}"),
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": format!(
                        "I want to write tests for: {file_path}\n\nProject conventions:\n{conv_text}\n\nPlease help me write tests following the project's testing conventions."
                    )}
                }]
            }))
        }
        "review-code" => {
            let diff = args["diff"].as_str().unwrap_or("(no diff provided)");
            Ok(serde_json::json!({
                "description": "Code review with project context",
                "messages": [{
                    "role": "user",
                    "content": { "type": "text", "text": format!(
                        "Please review this code change:\n```\n{diff}\n```\n\nProject conventions:\n{conv_text}\n\nCheck for convention violations and potential issues."
                    )}
                }]
            }))
        }
        _ => Err(format!("Unknown prompt: {name}")),
    }
}

fn resources_list() -> serde_json::Value {
    serde_json::json!([
        { "uri": "conventions://summary", "name": "Project Conventions", "mimeType": "text/plain" },
        { "uri": "conventions://error-handling", "name": "Error Handling Conventions", "mimeType": "text/plain" },
        { "uri": "conventions://naming", "name": "Naming Conventions", "mimeType": "text/plain" },
        { "uri": "conventions://testing", "name": "Testing Conventions", "mimeType": "text/plain" }
    ])
}

fn read_resource(
    state: &ServerState,
    uri: &str,
) -> Result<serde_json::Value, String> {
    let repo = state.default_repo();
    let conventions = crate::conventions::mine_conventions(&repo.all_files);
    let text = match uri {
        "conventions://summary" => crate::conventions::format_conventions(&conventions),
        "conventions://error-handling" => {
            serde_json::to_string_pretty(&conventions.error_handling)
                .unwrap_or_else(|_| "serialization error".to_string())
        }
        "conventions://naming" => {
            serde_json::to_string_pretty(&conventions.naming)
                .unwrap_or_else(|_| "serialization error".to_string())
        }
        "conventions://testing" => {
            serde_json::to_string_pretty(&conventions.testing)
                .unwrap_or_else(|_| "serialization error".to_string())
        }
        _ => return Err(format!("Unknown resource: {uri}")),
    };
    Ok(serde_json::json!({
        "contents": [{ "uri": uri, "mimeType": "text/plain", "text": text }]
    }))
}

// ---------------------------------------------------------------------------
// MCP stdio server loop
// ---------------------------------------------------------------------------

/// Run the MCP stdio server loop, reading JSON-RPC from stdin and writing responses to stdout.
pub fn run_mcp(state: Arc<RwLock<ServerState>>) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut session = Some(SessionState::new());
    let mut initialized = false;

    {
        let s = state.read().unwrap();
        let total_files: usize = s.repos.values().map(|r| r.all_files.len()).sum();
        let total_modules: usize = s.repos.values().map(|r| r.manifest.len()).sum();
        let repo_names: Vec<&str> = s.repos.keys().map(|k| k.as_str()).collect();
        tracing::info!(
            files = total_files,
            modules = total_modules,
            repos = s.repos.len(),
            names = repo_names.join(", ").as_str(),
            "MCP server ready"
        );
    }

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": "Parse error" }
                });
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", err);
                let _ = out.flush();
                continue;
            }
        };

        let method = msg["method"].as_str().unwrap_or("");

        // Notifications produce no response
        if method == "notifications/initialized" || method == "notifications/cancelled" {
            continue;
        }

        // Init ordering enforcement: reject non-init requests before initialize
        if !initialized && method != "initialize" && method != "ping" {
            if let Some(id) = msg.get("id").cloned() {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32002,
                        "message": "Server not initialized. Send 'initialize' first."
                    }
                });
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", serde_json::to_string(&err).unwrap());
                let _ = out.flush();
            }
            continue;
        }

        if let Some(response) = dispatch_jsonrpc(&state, &msg, &mut session) {
            // Track initialization state
            if method == "initialize" {
                initialized = true;
            }

            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", serde_json::to_string(&response).unwrap());
            let _ = out.flush();
        }
    }
}
