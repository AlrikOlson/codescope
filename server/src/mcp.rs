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
        Some(name) => state
            .repos
            .get(name)
            .ok_or_else(|| {
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
            "description": "Search source file contents (case-insensitive). Multi-word queries use OR matching: 'cloud reconstruct' finds lines containing 'cloud' OR 'reconstruct'. Returns matching lines with surrounding context.\n\nTips: Use specific single terms for precision. Filter with ext='rs,go' or category prefix. Follow up with cs_read_file start_line/end_line to read full context around matches.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (min 2 chars). Multiple words are OR-matched." },
                    "ext": { "type": "string", "description": "Comma-separated extensions to filter (e.g. 'h,cpp' or 'rs,go')" },
                    "category": { "type": "string", "description": "Module category prefix to filter" },
                    "limit": { "type": "integer", "description": "Max total matches to return. Default: 100" },
                    "context": { "type": "integer", "description": "Lines of context before/after each match (0-10). Default: 2" },
                    "repo": { "type": "string", "description": "Repository name (searches all repos if omitted)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "cs_list_modules",
            "description": "List all modules/categories with file counts. Use to discover available modules before drilling into specific ones.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "repo": { "type": "string", "description": "Repository name (optional if single repo)" }
                },
                "additionalProperties": false
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
            "name": "cs_search",
            "description": "Fuzzy search for files and modules by name. Space-separated terms must all match. File extensions are auto-stripped ('main.rs' works). CamelCase queries are case-sensitive. For searching file CONTENTS, use cs_grep instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query (e.g. 'config parser', 'FMeshBatch', 'main.rs')" },
                    "fileLimit": { "type": "integer", "description": "Max file results (default 20)" },
                    "moduleLimit": { "type": "integer", "description": "Max module results (default 8)" },
                    "repo": { "type": "string", "description": "Repository name (searches all repos if omitted)" }
                },
                "required": ["query"]
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
            "description": "Combined search: fuzzy filename match + content grep in one call. Returns a unified ranked list. Use this as your first search tool — it replaces calling cs_search + cs_grep separately.\n\nReturns files ranked by combined name relevance and content match density.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms (e.g. 'VolumetricCloud', 'config parser')" },
                    "ext": { "type": "string", "description": "Comma-separated extensions to filter (e.g. 'h,cpp' or 'rs,ts')" },
                    "category": { "type": "string", "description": "Module category prefix to filter" },
                    "limit": { "type": "integer", "description": "Max results to return. Default: 30" },
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
            "name": "cs_impact",
            "description": "Impact analysis: given a file, find everything that depends on it (directly or transitively). Shows the full dependency chain with depth levels. Use to answer 'what breaks if I change this?'",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File to analyze impact for" },
                    "max_depth": { "type": "integer", "description": "Max traversal depth (default: 5)" },
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

fn handle_tool_call(state: &ServerState, name: &str, args: &serde_json::Value) -> (String, bool) {
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
                        if mode == "stubs" {
                            let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
                            let content = extract_stubs(&raw, ext);
                            let lines = content.lines().count();
                            (
                                format!("# {path}\n({lines} lines, stubs)\n\n{content}"),
                                false,
                            )
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
                            (
                                format!("# {path} (lines {s}-{e} of {total})\n\n{content}"),
                                false,
                            )
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
                return (
                    "Error: Query must be at least 2 characters".to_string(),
                    true,
                );
            }

            let limit = args["limit"].as_u64().unwrap_or(100).min(500) as usize;
            let max_per_file = 8;
            let context_lines = args["context"].as_u64().unwrap_or(2).min(10) as usize;
            let ext_filter: Option<HashSet<String>> = args["ext"].as_str().map(|exts| {
                exts.split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_string())
                    .collect()
            });
            let cat_filter = args["category"].as_str();

            // Multi-term OR
            let terms: Vec<&str> = query.split_whitespace().collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
            let pattern_str = terms
                .iter()
                .map(|t| regex::escape(t))
                .collect::<Vec<_>>()
                .join("|");
            let pattern = match RegexBuilder::new(&pattern_str)
                .case_insensitive(true)
                .build()
            {
                Ok(p) => p,
                Err(_) => return ("Error: Invalid pattern".to_string(), true),
            };

            let start = std::time::Instant::now();

            struct GrepFileHit {
                display_path: String,
                desc: String,
                match_indices: Vec<usize>,
                total_match_count: usize,
                lines: Vec<String>,
                score: f64,
            }

            let mut file_hits: Vec<GrepFileHit> = Vec::new();

            for repo in &repos {
                let config = &repo.config;
                let idf_weights: Vec<f64> = terms_lower
                    .iter()
                    .map(|t| repo.term_doc_freq.idf(t))
                    .collect();
                let candidates: Vec<&ScannedFile> = repo
                    .all_files
                    .iter()
                    .filter(|f| {
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

                for file in &candidates {
                    let content = match fs::read_to_string(&file.abs_path) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    let lines: Vec<&str> = content.lines().collect();
                    let total_lines = lines.len().max(1);

                    let mut match_indices: Vec<usize> = Vec::new();
                    let mut total_match_count = 0usize;
                    let mut first_match_line_idx = usize::MAX;
                    let mut terms_seen = std::collections::HashSet::new();
                    for (i, line) in lines.iter().enumerate() {
                        if pattern.is_match(line) {
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
                    }

                    if match_indices.is_empty() {
                        continue;
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
                        if first_match_line_idx == usize::MAX { 0 } else { first_match_line_idx },
                        &idf_weights,
                    );

                    file_hits.push(GrepFileHit {
                        display_path: repo_path(repo, &file.rel_path, multi),
                        desc: file.desc.clone(),
                        match_indices,
                        total_match_count,
                        lines: lines.iter().map(|l| l.to_string()).collect(),
                        score,
                    });
                }
            }

            // Sort by relevance score descending
            file_hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Format top-N results within limit
            let mut results = Vec::new();
            let mut total_matches: usize = 0;

            let truncate = |line: &str| -> String {
                if line.len() > 200 {
                    format!("{}...", &line[..200])
                } else {
                    line.to_string()
                }
            };

            for hit in &file_hits {
                if total_matches >= limit {
                    break;
                }
                total_matches += hit.total_match_count;

                if context_lines == 0 {
                    let file_lines: Vec<String> = hit
                        .match_indices
                        .iter()
                        .map(|&i| format!("  L{}: {}", i + 1, truncate(&hit.lines[i])))
                        .collect();
                    results.push(format!(
                        "{}  ({}, score {:.0})\n{}",
                        hit.display_path,
                        hit.desc,
                        hit.score,
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
                        "{}  ({}, score {:.0})\n{}",
                        hit.display_path,
                        hit.desc,
                        hit.score,
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
            let mut out = String::new();
            for (cat, files) in &repo.manifest {
                out.push_str(&format!("{cat}  ({} files)\n", files.len()));
            }
            (
                format!("{} modules total\n\n{out}", repo.manifest.len()),
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
                None => (
                    format!("No dependency info found for '{module}'"),
                    true,
                ),
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
        "cs_search" => {
            let repos = resolve_repos_for_search(state, args);
            if repos.is_empty() {
                return ("Error: No matching repos found".to_string(), true);
            }
            let multi = repos.len() > 1;

            let raw_query = args["query"].as_str().unwrap_or("");
            let query = crate::fuzzy::preprocess_search_query(raw_query);
            let file_limit = args["fileLimit"].as_u64().unwrap_or(20) as usize;
            let module_limit = args["moduleLimit"].as_u64().unwrap_or(8) as usize;

            let mut all_modules = Vec::new();
            let mut all_files = Vec::new();
            let mut total_files = 0usize;
            let mut total_modules = 0usize;

            let start = std::time::Instant::now();

            for repo in &repos {
                let resp = run_search(
                    &repo.search_files,
                    &repo.search_modules,
                    &query,
                    file_limit,
                    module_limit,
                );
                total_files += resp.total_files;
                total_modules += resp.total_modules;

                for m in resp.modules {
                    all_modules.push((repo, m));
                }
                for f in resp.files {
                    all_files.push((repo, f));
                }
            }

            // Sort by score descending
            all_modules.sort_by(|a, b| {
                b.1.score
                    .partial_cmp(&a.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            all_files.sort_by(|a, b| {
                b.1.score
                    .partial_cmp(&a.1.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            all_modules.truncate(module_limit);
            all_files.truncate(file_limit);

            let query_time = start.elapsed().as_secs_f64() * 1000.0;
            let mut out = String::new();
            if !all_modules.is_empty() {
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
            if !all_files.is_empty() {
                out.push_str("Files:\n");
                for (repo, f) in &all_files {
                    let prefix = if multi { format!("[{}] ", repo.name) } else { String::new() };
                    out.push_str(&format!("  {prefix}{} — {} (score {:.1})\n", f.path, f.desc, f.score));
                }
            }
            if all_modules.is_empty() && all_files.is_empty() {
                out.push_str(&format!("No results for '{raw_query}'"));
            }
            out.push_str(&format!(
                "\n({:.1}ms, searched {} files / {} modules)",
                query_time, total_files, total_modules
            ));
            (out, false)
        }
        "cs_read_context" => {
            let repo = match resolve_repo(state, args) {
                Ok(r) => r,
                Err(e) => return (format!("Error: {e}"), true),
            };
            let paths: Vec<String> = args["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
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
            let resp = allocate_budget(
                &repo.root,
                &paths,
                &repo.all_files,
                budget,
                &unit,
                query,
                &repo.deps,
                &repo.stub_cache,
                &*state.tokenizer,
                &repo.config,
            );

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

            // Append AI-friendly usage hint
            out.push_str("\n---\n");
            out.push_str("## How to explore further with CodeScope tools\n\n");
            out.push_str("The above is a **compressed overview** — stubs show signatures only, pruned files omit low-priority blocks, and manifest entries are one-line summaries. To dig deeper:\n\n");
            out.push_str("**Read full source for a specific file:**\n");
            out.push_str("  cs_read_file({ path: \"path/to/File.h\", mode: \"full\" })\n");
            out.push_str("  -> Returns complete file contents. Use start_line/end_line to read a specific range.\n\n");
            out.push_str("**Read structural outline first (recommended):**\n");
            out.push_str("  cs_read_file({ path: \"...\", mode: \"stubs\" })\n");
            out.push_str("  -> Class/function signatures without bodies. Use this BEFORE mode=\"full\" on large files.\n\n");
            out.push_str("**Search for code patterns or keywords:**\n");
            out.push_str("  cs_grep({ query: \"FogVolume\", ext: \"h,cpp\", context: 3 })\n");
            out.push_str("  -> Searches file contents. Returns matching lines with surrounding context.\n\n");
            out.push_str("**Find files by name + content (best first search):**\n");
            out.push_str("  cs_find({ query: \"VolumetricCloud\" })\n");
            out.push_str("  -> Combined fuzzy filename + content grep, ranked by relevance.\n\n");
            out.push_str("**Trace import dependencies:**\n");
            out.push_str("  cs_find_imports({ path: \"...\", direction: \"both\" })\n");
            out.push_str("  -> Shows what a file imports and what imports it. Essential for understanding coupling.\n\n");
            out.push_str("**Impact analysis:**\n");
            out.push_str("  cs_impact({ path: \"path/to/file.rs\" })\n");
            out.push_str("  -> Find everything that depends on this file. Answers 'what breaks if I change this?'\n\n");
            out.push_str("**Typical workflow:** cs_find -> pick top files -> cs_read_file (stubs) -> cs_read_file (full, start_line/end_line) for the specific section you need -> cs_grep to find usages.\n");

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
                repo.import_graph
                    .imports
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            };
            let imported_by: Vec<String> = if direction == "both" || direction == "imported_by" {
                repo.import_graph
                    .imported_by
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            };

            // Add cross-repo edges
            let mut cross_imports = Vec::new();
            let mut cross_imported_by = Vec::new();
            for edge in &state.cross_repo_edges {
                if edge.from_repo == repo.name && edge.from_file == path
                    && (direction == "both" || direction == "imports")
                {
                    cross_imports.push(format!("[{}] {}", edge.to_repo, edge.to_file));
                }
                if edge.to_repo == repo.name && edge.to_file == path
                    && (direction == "both" || direction == "imported_by")
                {
                    cross_imported_by.push(format!("[{}] {}", edge.from_repo, edge.from_file));
                }
            }

            if imports.is_empty() && imported_by.is_empty()
                && cross_imports.is_empty() && cross_imported_by.is_empty()
            {
                return (
                    format!("No import relationships found for '{path}'"),
                    false,
                );
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
                out.push_str(&format!("Cross-repo imported by ({} files):\n", cross_imported_by.len()));
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
                return (
                    "Error: Query must be at least 2 characters".to_string(),
                    true,
                );
            }
            let limit = args["limit"].as_u64().unwrap_or(30).min(100) as usize;
            let ext_filter: Option<HashSet<String>> = args["ext"].as_str().map(|exts| {
                exts.split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_string())
                    .collect()
            });
            let cat_filter = args["category"].as_str().map(|s| s.to_string());

            let start = std::time::Instant::now();

            // Content grep pattern
            let terms: Vec<&str> = raw_query.split_whitespace().collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
            let pattern_str = terms
                .iter()
                .map(|t| regex::escape(t))
                .collect::<Vec<_>>()
                .join("|");
            let pattern = RegexBuilder::new(&pattern_str)
                .case_insensitive(true)
                .build();

            struct FindResult {
                display_path: String,
                desc: String,
                name_score: f64,
                grep_score: f64,
                grep_count: usize,
                top_match: Option<String>,
            }

            let mut merged: std::collections::HashMap<String, FindResult> =
                std::collections::HashMap::new();

            for repo in &repos {
                let config = &repo.config;

                // 1. Fuzzy filename search
                let query = crate::fuzzy::preprocess_search_query(raw_query);
                let search_resp =
                    run_search(&repo.search_files, &repo.search_modules, &query, limit, 0);

                // Add fuzzy search results
                for f in &search_resp.files {
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
                        },
                    );
                }

                // 2. Content grep
                if let Ok(ref pattern) = pattern {
                    let idf_weights: Vec<f64> = terms_lower
                        .iter()
                        .map(|t| repo.term_doc_freq.idf(t))
                        .collect();
                    let candidates: Vec<&ScannedFile> = repo
                        .all_files
                        .iter()
                        .filter(|f| {
                            if let Some(ref exts) = ext_filter {
                                if !exts.contains(&f.ext) {
                                    return false;
                                }
                            }
                            if let Some(ref cat) = cat_filter {
                                let file_cat =
                                    get_category_path(&f.rel_path, config).join(" > ");
                                if !file_cat.starts_with(cat.as_str()) {
                                    return false;
                                }
                            }
                            true
                        })
                        .collect();

                    for file in &candidates {
                        let content = match fs::read_to_string(&file.abs_path) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };
                        let lines: Vec<&str> = content.lines().collect();
                        let total_lines = lines.len().max(1);
                        let mut match_count = 0usize;
                        let mut first_match: Option<String> = None;
                        let mut first_match_line_idx = usize::MAX;
                        let mut terms_seen = std::collections::HashSet::new();
                        for (i, line) in lines.iter().enumerate() {
                            if pattern.is_match(line) {
                                match_count += 1;
                                if first_match.is_none() {
                                    first_match_line_idx = i;
                                    let trimmed = if line.len() > 120 {
                                        format!("{}...", &line[..120])
                                    } else {
                                        line.to_string()
                                    };
                                    first_match = Some(trimmed);
                                }
                                let line_lower = line.to_lowercase();
                                for (ti, term) in terms_lower.iter().enumerate() {
                                    if line_lower.contains(term.as_str()) {
                                        terms_seen.insert(ti);
                                    }
                                }
                            }
                        }
                        if match_count == 0 {
                            continue;
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
                            if first_match_line_idx == usize::MAX { 0 } else { first_match_line_idx },
                            &idf_weights,
                        );

                        let key = repo_path(repo, &file.rel_path, multi);
                        let entry = merged
                            .entry(key.clone())
                            .or_insert_with(|| FindResult {
                                display_path: key,
                                desc: file.desc.clone(),
                                name_score: 0.0,
                                grep_score: 0.0,
                                grep_count: 0,
                                top_match: None,
                            });
                        entry.grep_score = grep_score;
                        entry.grep_count = match_count;
                        entry.top_match = first_match;
                    }
                }
            }

            // Unified scoring — adaptive weights based on query shape
            let (name_w, grep_w) = if terms.len() > 1 { (0.4, 0.6) } else { (0.6, 0.4) };
            let mut ranked: Vec<FindResult> = merged.into_values().collect();
            ranked.sort_by(|a, b| {
                let score_a = a.name_score * name_w + a.grep_score * grep_w;
                let score_b = b.name_score * name_w + b.grep_score * grep_w;
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            ranked.truncate(limit);

            let query_time = start.elapsed().as_millis();
            let mut out = format!(
                "Found {} results for \"{}\" ({query_time}ms)\n\n",
                ranked.len(),
                raw_query
            );

            for r in &ranked {
                let mut tags = Vec::new();
                if r.name_score > 0.0 {
                    tags.push("name match".to_string());
                }
                if r.grep_count > 0 {
                    tags.push(format!("{} content matches", r.grep_count));
                }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", tags.join(" + "))
                };
                out.push_str(&format!("  {} — {}{tag_str}\n", r.display_path, r.desc));
                if let Some(ref line) = r.top_match {
                    out.push_str(&format!("    > {}\n", line.trim()));
                }
            }

            (out, false)
        }

        // =====================================================================
        // New tools
        // =====================================================================

        "cs_status" => {
            let version = env!("CARGO_PKG_VERSION");
            let repo_count = state.repos.len();
            let mut out = format!("CodeScope v{version} — {repo_count} repositor{} indexed\n\n",
                if repo_count == 1 { "y" } else { "ies" });

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
                out.push_str(&format!("  Last scan: {}ms\n\n", repo.scan_time_ms));
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
                        out.push_str(&format!("  {f}\n"));
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

            let index = match &repo.semantic_index {
                Some(idx) => idx,
                None => {
                    return (
                        "Error: Semantic index not available. Start the server with --semantic flag to enable.".to_string(),
                        true,
                    );
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

fn handle_rescan(
    state: &mut ServerState,
    args: &serde_json::Value,
) -> (String, bool) {
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

fn handle_add_repo(
    state: &mut ServerState,
    args: &serde_json::Value,
) -> (String, bool) {
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
    state.repos.insert(name, new_state);

    // Rebuild cross-repo edges
    state.cross_repo_edges = crate::scan::resolve_cross_repo_imports(&state.repos);

    (summary, false)
}

// ---------------------------------------------------------------------------
// MCP stdio server loop
// ---------------------------------------------------------------------------

pub fn run_mcp(state: Arc<RwLock<ServerState>>) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();

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
                let arguments = msg["params"]
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

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
                        handle_tool_call(&s, tool_name, &arguments)
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
