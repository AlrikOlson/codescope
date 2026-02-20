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
// Tool definitions
// ---------------------------------------------------------------------------

fn tool_definitions() -> serde_json::Value {
    #[allow(unused_mut)]
    let mut tools = serde_json::json!([
        {
            "name": "cs_read_file",
            "description": "Read a source file.\n\nModes:\n- stubs (recommended first): structural outline with class/function signatures, no bodies. Use to understand file structure.\n- full: complete content. For large files, use start_line/end_line to read specific sections.\n\nWorkflow: cs_grep -> find line number -> cs_read_file with start_line/end_line for details.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "mode": { "type": "string", "enum": ["full", "stubs"], "description": "full = complete file, stubs = structural outline only. Default: full" },
                    "start_line": { "type": "integer", "description": "First line to return (1-based). Only applies to mode='full'." },
                    "end_line": { "type": "integer", "description": "Last line to return (1-based, inclusive). Only applies to mode='full'." },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "cs_read_files",
            "description": "Batch read multiple source files (max 50). Use mode='stubs' to read many files efficiently. For targeted reading of specific sections, use cs_read_file with start_line/end_line instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of relative paths from project root"
                    },
                    "mode": { "type": "string", "enum": ["full", "stubs"], "description": "full = complete files, stubs = structural outlines. Default: full" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["paths"]
            }
        },
        {
            "name": "cs_grep",
            "description": "Search source file contents (case-insensitive). Default match_mode='all' requires ALL terms present in a line. Use 'any' for OR, 'exact' for literal phrases, 'regex' for patterns.\n\nTips: Filter with ext='rs,go', path='server/src' prefix, or category. Follow up with cs_read_file for full context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (min 2 chars)." },
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
            "name": "cs_list_modules",
            "description": "List modules/categories with file counts. Use to discover available modules before drilling into specific ones.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prefix": { "type": "string", "description": "Filter modules by prefix (e.g. 'Runtime' or 'Runtime > Engine')" },
                    "limit": { "type": "integer", "description": "Max modules to return. Default: 100" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                }
            }
        },
        {
            "name": "cs_get_module_files",
            "description": "Get all files in a specific module/category. Use the exact module name from cs_list_modules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Module category path (e.g. 'Runtime > Renderer > Nanite')" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["module"]
            }
        },
        {
            "name": "cs_get_deps",
            "description": "Get public/private dependencies for a module.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Module name (e.g. 'Renderer', 'Core', 'my-library')" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["module"]
            }
        },
        {
            "name": "cs_read_context",
            "description": "Budget-aware batch read. Reads multiple files, compresses to fit token budget via importance-weighted allocation with block-level pruning (full stubs -> pruned -> manifest). Use instead of multiple cs_read_file calls when reading 3+ related files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of relative paths from project root"
                    },
                    "budget": { "type": "integer", "description": "Max token budget. Default: 50000" },
                    "ordering": { "type": "string", "enum": ["importance", "attention"], "description": "Output ordering. 'importance' (default): descending by relevance. 'attention': primacy/recency optimized — high-importance at start and end, medium in middle, exploiting LLM attention patterns." },
                    "include_seen": { "type": "boolean", "description": "If true, don't deprioritize files already read in this session. Default: false (previously-read files get lower priority to maximize new information)." },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["paths"]
            }
        },
        {
            "name": "cs_find_imports",
            "description": "Find import/include relationships for a file. Shows what a file imports and/or what files import it. Use to explore dependency chains and discover related files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "direction": { "type": "string", "enum": ["imports", "imported_by", "both"], "description": "Which direction to query. Default: both" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "cs_find",
            "description": "Combined search: fuzzy filename + content grep in one call. Returns a unified ranked list. Use this as your primary search tool for discovering files and modules.\n\nReturns files ranked by combined name relevance and content match density. Use fileLimit/moduleLimit to control result counts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (e.g. 'VolumetricCloud', 'config parser')" },
                    "match_mode": { "type": "string", "enum": ["all", "any", "exact", "regex"], "description": "How to match multi-word queries. 'all' (default): line must contain ALL terms. 'any': line contains ANY term (OR). 'exact': treat query as literal phrase. 'regex': raw regex pattern." },
                    "ext": { "type": "string", "description": "Comma-separated extensions to filter (e.g. 'h,cpp' or 'rs,ts')" },
                    "path": { "type": "string", "description": "Path prefix to filter files (e.g. 'server/src' or 'src/components')" },
                    "category": { "type": "string", "description": "Module category prefix to filter" },
                    "fileLimit": { "type": "integer", "description": "Max file results (default: 30)" },
                    "moduleLimit": { "type": "integer", "description": "Max module results (default: 5)" },
                    "repo": { "type": "string", "description": "Repository name (searches all repos if omitted)" }
                },
                "required": ["query"]
            }
        },
        // ---- New tools ----
        {
            "name": "cs_status",
            "description": "Show indexed repositories, file counts, language breakdown, and scan time.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "cs_rescan",
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
            "description": "Dynamically add a new repository to the index at runtime.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name/alias for the repository" },
                    "root": { "type": "string", "description": "Absolute path to the repository root" }
                },
                "required": ["name", "root"]
            }
        },
        {
            "name": "cs_blame",
            "description": "Git blame for a file. Shows who last modified each line, when, and in which commit. Optionally scope to a line range.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "start_line": { "type": "integer", "description": "First line (1-based, optional)" },
                    "end_line": { "type": "integer", "description": "Last line (1-based, inclusive, optional)" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "cs_file_history",
            "description": "Recent commits that touched a specific file. Shows commit hash, author, date, message, and which other files were changed in the same commit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "limit": { "type": "integer", "description": "Max commits to return (default: 10)" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "cs_changed_since",
            "description": "Files changed since a commit, branch, or tag. Use to see what changed between two points in history. Supports commit hashes (full or short), branch names (main, origin/main), and tags.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": { "type": "string", "description": "Commit hash, branch name, or tag to diff against HEAD" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["since"]
            }
        },
        {
            "name": "cs_hot_files",
            "description": "Most frequently changed files (churn ranking). Identifies code hotspots by counting how many commits touched each file within a time window. High-churn files often indicate areas needing refactoring or close review.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max files to return (default: 20)" },
                    "days": { "type": "integer", "description": "Look back N days (default: 90)" },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                }
            }
        },
        {
            "name": "cs_session_info",
            "description": "Show what files have been read in this MCP session. Useful for understanding context consumption and avoiding redundant reads.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "cs_impact",
            "description": "Impact analysis: given a file, find everything that depends on it (directly or transitively). Shows the full dependency chain with depth levels. Use to answer 'what breaks if I change this?'",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File to analyze impact for" },
                    "max_depth": { "type": "integer", "description": "Max traversal depth (default: 5)" },
                    "limit": { "type": "integer", "description": "Max files to show (default: 50). Remaining are counted but not listed." },
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "required": ["path"]
            }
        }
    ]);

    #[cfg(feature = "semantic")]
    {
        if let Some(arr) = tools.as_array_mut() {
            arr.push(serde_json::json!({
                "name": "cs_semantic_search",
                "description": "Search code by intent using semantic embeddings. Finds conceptually similar code even without keyword overlap. Requires --semantic flag at startup.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Natural language description of what you're looking for" },
                        "limit": { "type": "integer", "description": "Max results (default: 10)" },
                        "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                    },
                    "required": ["query"]
                }
            }));
        }
    }

    tools
}

// ---------------------------------------------------------------------------
// Tool call handler (read-only, takes &ServerState)
// ---------------------------------------------------------------------------

fn handle_tool_call(
    state: &ServerState,
    name: &str,
    args: &serde_json::Value,
    session: &mut Option<SessionState>,
) -> (String, bool) {
    match name {
        "cs_read_file" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let path = args["path"].as_str().unwrap_or("");
            let mode = args["mode"].as_str().unwrap_or("full");
            let start_line = args["start_line"].as_u64().map(|n| n.max(1) as usize);
            let end_line = args["end_line"].as_u64().map(|n| n as usize);
            match validate_path(&repo.root, path) {
                Err(e) => (format!("Error: {e}"), true),
                Ok(full_path) => match fs::read_to_string(&full_path) {
                    Err(_) => ("Error: Could not read file".to_string(), true),
                    Ok(raw) => {
                        // Record read in session
                        if let Some(ref mut s) = session {
                            let approx_tokens = raw.len() / 4;
                            s.record_read(path, approx_tokens);
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
                                return (format!("Error: start_line ({s}) > end_line ({e})"), true);
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
        }
        "cs_read_files" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let paths: Vec<&str> = args["paths"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
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
        "cs_grep" => {
            let repos = resolve_repos_for_search(state, args);
            if repos.is_empty() {
                return ("Error: No matching repos found".to_string(), true);
            }
            let multi = repos.len() > 1;

            let query = args["query"].as_str().unwrap_or("");
            if query.len() < 2 {
                return ("Error: Query must be at least 2 characters".to_string(), true);
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

            // Build match pattern based on match_mode
            let terms: Vec<&str> = query.split_whitespace().collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();

            // For "all" mode with multiple terms, we use OR pattern + post-filter
            // (Rust regex crate does not support lookahead assertions)
            let require_all_terms = match_mode == "all" && terms.len() > 1;

            let pattern = match match_mode {
                "exact" => RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build(),
                "regex" => RegexBuilder::new(query).case_insensitive(true).build(),
                _ => {
                    // "all" (multi-term), "any", or "all" (single-term) — use OR pattern
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

            // Sort by relevance score descending
            file_hits
                .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

            // Format top-N results — limit caps number of FILES, not line matches
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
        "cs_list_modules" => {
            let repo = match resolve_repo(state, args) {
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
        "cs_get_module_files" => {
            let repo = match resolve_repo(state, args) {
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
        "cs_get_deps" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let module = args["module"].as_str().unwrap_or("");
            match repo.deps.get(module) {
                None => (format!("No dependency info found for '{module}'"), true),
                Some(dep) => {
                    let mut out = format!("Module: {module}\nCategory: {}\n\n", dep.category_path);
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
        "cs_read_context" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let paths: Vec<String> = args["paths"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let budget = args["budget"].as_u64().unwrap_or(DEFAULT_TOKEN_BUDGET as u64) as usize;
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
            let seen = if include_seen { None } else { session.as_ref().map(|s| s.seen_paths()) };
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

            // Record reads in session
            if let Some(ref mut s) = session {
                for (path, entry) in &resp.files {
                    if !path.starts_with('_') {
                        s.record_read(path, entry.tokens);
                    }
                }
            }

            // Format as human-readable text for MCP
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

            // Tier breakdown
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

            // Sort paths for consistent output
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
        }
        "cs_find_imports" => {
            let repo = match resolve_repo(state, args) {
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
            let imported_by: Vec<String> = if direction == "both" || direction == "imported_by" {
                repo.import_graph.imported_by.get(path).cloned().unwrap_or_default()
            } else {
                vec![]
            };

            // Add cross-repo edges
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
        "cs_find" => {
            let repos = resolve_repos_for_search(state, args);
            if repos.is_empty() {
                return ("Error: No matching repos found".to_string(), true);
            }
            let multi = repos.len() > 1;

            let raw_query = args["query"].as_str().unwrap_or("");
            if raw_query.len() < 2 {
                return ("Error: Query must be at least 2 characters".to_string(), true);
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
                    // "all" (multi-term), "any", or "all" (single-term) — use OR pattern
                    // For "all" multi-term: post-filter ensures ALL terms are present
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

                // Collect module results
                for m in search_resp.modules {
                    all_modules.push((repo, m));
                }

                // Add fuzzy search results
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

            // Unified scoring — adaptive weights with score normalization
            let (name_w, grep_w) = if terms.len() > 1 { (0.4, 0.6) } else { (0.6, 0.4) };
            let mut ranked: Vec<FindResult> = merged.into_values().collect();

            // Normalize scores to 0-1 range so weighting works correctly
            let max_name = ranked.iter().map(|r| r.name_score).fold(0.0f64, f64::max).max(1.0);
            let max_grep = ranked.iter().map(|r| r.grep_score).fold(0.0f64, f64::max).max(1.0);

            ranked.sort_by(|a, b| {
                let norm_a =
                    (a.name_score / max_name) * name_w + (a.grep_score / max_grep) * grep_w;
                let norm_b =
                    (b.name_score / max_name) * name_w + (b.grep_score / max_grep) * grep_w;
                // Dual-match boost: files matching both name AND content get 1.25x
                let boost_a = if a.name_score > 0.0 && a.grep_count > 0 { 1.25 } else { 1.0 };
                let boost_b = if b.name_score > 0.0 && b.grep_count > 0 { 1.25 } else { 1.0 };
                (norm_b * boost_b)
                    .partial_cmp(&(norm_a * boost_a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ranked.truncate(file_limit);

            let query_time = start.elapsed().as_millis();
            let mut out = format!(
                "Found {} results for \"{}\" ({query_time}ms)\n\n",
                ranked.len(),
                raw_query
            );

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
                let source = match (has_name, has_content) {
                    (true, true) => "name+content",
                    (true, false) => "name",
                    (false, true) => "content",
                    (false, false) => "",
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

        // =====================================================================
        // Git-aware tools
        // =====================================================================
        "cs_blame" => {
            let repo = match resolve_repo(state, args) {
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
        "cs_file_history" => {
            let repo = match resolve_repo(state, args) {
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
        "cs_changed_since" => {
            let repo = match resolve_repo(state, args) {
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
                    // Group by status
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
        "cs_hot_files" => {
            let repo = match resolve_repo(state, args) {
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

        "cs_session_info" => match session {
            Some(ref s) => {
                let elapsed = s.started_at.elapsed();
                let mins = elapsed.as_secs() / 60;
                let secs = elapsed.as_secs() % 60;
                let mut out = format!(
                    "Session: {}m {}s, {} files read, ~{} tokens served\n\n",
                    mins,
                    secs,
                    s.files_read.len(),
                    s.total_tokens_served
                );
                if !s.files_read.is_empty() {
                    out.push_str("Files read:\n");
                    let mut sorted: Vec<(&String, &std::time::Instant)> =
                        s.files_read.iter().collect();
                    sorted.sort_by_key(|(_, t)| *t);
                    for (path, _) in sorted {
                        out.push_str(&format!("  {path}\n"));
                    }
                }
                (out, false)
            }
            None => ("Session tracking not available (HTTP mode)".to_string(), false),
        },

        // =====================================================================
        // Status & management tools
        // =====================================================================
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
            (out, false)
        }

        "cs_impact" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let path = args["path"].as_str().unwrap_or("");
            let max_depth = args["max_depth"].as_u64().unwrap_or(5).min(20) as usize;
            let file_limit = args["limit"].as_u64().unwrap_or(50).min(500) as usize;

            if path.is_empty() {
                return ("Error: path is required".to_string(), true);
            }

            // BFS over imported_by graph
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
                // Local imports
                if let Some(dependents) = repo.import_graph.imported_by.get(&current) {
                    for dep in dependents {
                        if visited.insert(dep.clone()) {
                            queue.push_back((dep.clone(), depth + 1));
                        }
                    }
                }
                // Cross-repo imports (files that import this file from other repos)
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
        }

        #[cfg(feature = "semantic")]
        "cs_semantic_search" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let query = args["query"].as_str().unwrap_or("");
            if query.is_empty() {
                return ("Error: 'query' is required".to_string(), true);
            }
            let limit = args["limit"].as_u64().unwrap_or(10).min(50) as usize;

            let sem_guard = repo.semantic_index.read().unwrap();
            let index = match sem_guard.as_ref() {
                Some(idx) => idx,
                None => {
                    use std::sync::atomic::Ordering::Relaxed;
                    let sp = &repo.semantic_progress;
                    let msg = match sp.status.load(Relaxed) {
                        1 => "Semantic index is extracting chunks — please try again shortly.".to_string(),
                        2 => {
                            let done = sp.completed_batches.load(Relaxed);
                            let total = sp.total_batches.load(Relaxed);
                            let chunks = sp.total_chunks.load(Relaxed);
                            let device = sp.device.read().unwrap();
                            let pct = if total > 0 { done * 100 / total } else { 0 };
                            format!(
                                "Semantic index is building: {done}/{total} batches ({pct}%) on {device}, {chunks} chunks. Try again shortly."
                            )
                        }
                        4 => "Semantic index failed to build. Check server logs for details.".to_string(),
                        _ => "Semantic index not available. This binary may not include semantic search support.".to_string(),
                    };
                    return (format!("Error: {msg}"), true);
                }
            };

            let start = std::time::Instant::now();
            match crate::semantic::semantic_search(index, query, limit) {
                Ok(results) => {
                    let query_time = start.elapsed().as_millis();
                    let mut out = format!(
                        "Semantic search: {} results for \"{}\" ({}ms)\n\n",
                        results.len(),
                        query,
                        query_time
                    );
                    for (i, r) in results.iter().enumerate() {
                        out.push_str(&format!(
                            "{}. {} (line ~{}, score {:.3})\n   {}\n\n",
                            i + 1,
                            r.file_path,
                            r.start_line,
                            r.score,
                            r.snippet.replace('\n', "\n   ")
                        ));
                    }
                    (out, false)
                }
                Err(e) => (format!("Error: Semantic search failed: {e}"), true),
            }
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
        let thread_name = name.clone();
        std::thread::spawn(move || {
            eprintln!("  [{thread_name}] Building semantic index in background...");
            let sem_start = std::time::Instant::now();
            if let Some(idx) = crate::semantic::build_semantic_index(
                &files,
                model.as_deref(),
                &progress,
                &repo_root,
            ) {
                eprintln!(
                    "  [{thread_name}] Semantic index ready: {} chunks ({}ms)",
                    idx.chunk_meta.len(),
                    sem_start.elapsed().as_millis()
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

    state.repos.insert(name, new_state);

    // Rebuild cross-repo edges
    state.cross_repo_edges = crate::scan::resolve_cross_repo_imports(&state.repos);

    (format!("{summary}{semantic_summary}"), false)
}

// ---------------------------------------------------------------------------
// MCP stdio server loop
// ---------------------------------------------------------------------------

pub fn run_mcp(state: Arc<RwLock<ServerState>>) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut session = Some(SessionState::new());

    {
        let s = state.read().unwrap();
        let total_files: usize = s.repos.values().map(|r| r.all_files.len()).sum();
        let total_modules: usize = s.repos.values().map(|r| r.manifest.len()).sum();
        let repo_names: Vec<&str> = s.repos.keys().map(|k| k.as_str()).collect();
        eprintln!(
            "MCP server ready ({} files, {} modules, {} repo(s): {})",
            total_files,
            total_modules,
            s.repos.len(),
            repo_names.join(", ")
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
        let id = msg.get("id").cloned();

        if method == "notifications/initialized" || method == "notifications/cancelled" {
            continue;
        }

        let response = match method {
            "initialize" => {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "codescope",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "CodeScope — search, browse, and read source files in any codebase. Start with cs_find (combined filename + content search) for discovery. Use cs_find_imports to trace import dependencies. Use cs_grep for targeted content search with context. Use cs_read_file to read specific files or line ranges. Use cs_read_context for budget-aware batch reads of 3+ files."
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
                        handle_tool_call(&s, tool_name, &arguments, &mut session)
                    }
                };

                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": text }],
                        "isError": is_error
                    }
                })
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

        let mut out = stdout.lock();
        let _ = writeln!(out, "{}", serde_json::to_string(&response).unwrap());
        let _ = out.flush();
    }
}
