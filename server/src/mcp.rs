use crate::budget::{allocate_budget, BudgetUnit, DEFAULT_TOKEN_BUDGET};
use crate::fuzzy::run_search;
use crate::scan::get_category_path;
use crate::stubs::extract_stubs;
use crate::types::*;
use regex::RegexBuilder;
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write as IoWrite};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_definition_file(ext: &str) -> bool {
    matches!(ext, "h" | "hpp" | "hxx" | "d.ts" | "pyi")
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "cs_read_file",
            "description": "Read a source file.\n\nModes:\n- stubs (recommended first): structural outline with class/function signatures, no bodies. Use to understand file structure.\n- full: complete content. For large files, use start_line/end_line to read specific sections.\n\nWorkflow: cs_grep -> find line number -> cs_read_file with start_line/end_line for details.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path from project root" },
                    "mode": { "type": "string", "enum": ["full", "stubs"], "description": "full = complete file, stubs = structural outline only. Default: full" },
                    "start_line": { "type": "integer", "description": "First line to return (1-based). Only applies to mode='full'." },
                    "end_line": { "type": "integer", "description": "Last line to return (1-based, inclusive). Only applies to mode='full'." }
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
                    "mode": { "type": "string", "enum": ["full", "stubs"], "description": "full = complete files, stubs = structural outlines. Default: full" }
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
                    "context": { "type": "integer", "description": "Lines of context before/after each match (0-10). Default: 2" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "cs_list_modules",
            "description": "List all modules/categories with file counts. Use to discover available modules before drilling into specific ones.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        },
        {
            "name": "cs_get_module_files",
            "description": "Get all files in a specific module/category. Use the exact module name from cs_list_modules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Module category path (e.g. 'Runtime > Renderer > Nanite')" }
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
                    "module": { "type": "string", "description": "Module name (e.g. 'Renderer', 'Core', 'my-library')" }
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
                    "moduleLimit": { "type": "integer", "description": "Max module results (default 8)" }
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
                    "budget": { "type": "integer", "description": "Max token budget. Default: 50000" }
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
                    "direction": { "type": "string", "enum": ["imports", "imported_by", "both"], "description": "Which direction to query. Default: both" }
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
                    "limit": { "type": "integer", "description": "Max results to return. Default: 30" }
                },
                "required": ["query"]
            }
        }
    ])
}

// ---------------------------------------------------------------------------
// Tool call handler
// ---------------------------------------------------------------------------

fn handle_tool_call(state: &McpState, name: &str, args: &serde_json::Value) -> (String, bool) {
    let config = ScanConfig::new(state.project_root.clone());

    match name {
        "cs_read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let mode = args["mode"].as_str().unwrap_or("full");
            let start_line = args["start_line"].as_u64().map(|n| n.max(1) as usize);
            let end_line = args["end_line"].as_u64().map(|n| n as usize);
            match validate_path(&state.project_root, path) {
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
                match validate_path(&state.project_root, p) {
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

            let candidates: Vec<&ScannedFile> = state
                .all_files
                .iter()
                .filter(|f| {
                    if let Some(ref exts) = ext_filter {
                        if !exts.contains(&f.ext) {
                            return false;
                        }
                    }
                    if let Some(cat) = cat_filter {
                        let file_cat = get_category_path(&f.rel_path, &config).join(" > ");
                        if !file_cat.starts_with(cat) {
                            return false;
                        }
                    }
                    true
                })
                .collect();

            struct GrepFileHit {
                rel_path: String,
                desc: String,
                match_indices: Vec<usize>,
                total_match_count: usize,
                lines: Vec<String>,
                score: f64,
            }

            let mut file_hits: Vec<GrepFileHit> = Vec::new();

            for file in &candidates {
                let content = match fs::read_to_string(&file.abs_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len().max(1);

                let mut match_indices: Vec<usize> = Vec::new();
                let mut total_match_count = 0usize;
                for (i, line) in lines.iter().enumerate() {
                    if pattern.is_match(line) {
                        total_match_count += 1;
                        if match_indices.len() < max_per_file {
                            match_indices.push(i);
                        }
                    }
                }

                if match_indices.is_empty() {
                    continue;
                }

                // BM25-lite scoring
                let tf = total_match_count as f64 / (total_match_count as f64 + 1.5);
                let filename = file
                    .rel_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&file.rel_path)
                    .to_lowercase();
                let filename_bonus =
                    if terms_lower.iter().any(|t| filename.contains(t.as_str())) {
                        50.0
                    } else {
                        0.0
                    };
                let def_bonus = if is_definition_file(&file.ext) {
                    5.0
                } else {
                    0.0
                };
                let density = total_match_count as f64 / total_lines as f64 * 10.0;
                let score = tf * 20.0 + filename_bonus + def_bonus + density;

                file_hits.push(GrepFileHit {
                    rel_path: file.rel_path.clone(),
                    desc: file.desc.clone(),
                    match_indices,
                    total_match_count,
                    lines: lines.iter().map(|l| l.to_string()).collect(),
                    score,
                });
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
                        hit.rel_path,
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
                        hit.rel_path,
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
            let mut out = String::new();
            for (cat, files) in &state.manifest {
                out.push_str(&format!("{cat}  ({} files)\n", files.len()));
            }
            (
                format!("{} modules total\n\n{out}", state.manifest.len()),
                false,
            )
        }
        "cs_get_module_files" => {
            let module = args["module"].as_str().unwrap_or("");
            let prefix_dot = format!("{module} > ");
            let mut out = String::new();
            let mut count = 0;
            for (cat, files) in &state.manifest {
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
            let module = args["module"].as_str().unwrap_or("");
            match state.deps.get(module) {
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
            let raw_query = args["query"].as_str().unwrap_or("");
            let query = crate::fuzzy::preprocess_search_query(raw_query);
            let file_limit = args["fileLimit"].as_u64().unwrap_or(20) as usize;
            let module_limit = args["moduleLimit"].as_u64().unwrap_or(8) as usize;
            let resp = run_search(
                &state.search_files,
                &state.search_modules,
                &query,
                file_limit,
                module_limit,
            );

            let mut out = String::new();
            if !resp.modules.is_empty() {
                out.push_str("Modules:\n");
                for m in &resp.modules {
                    out.push_str(&format!(
                        "  {} ({} files, score {:.1})\n",
                        m.id, m.file_count, m.score
                    ));
                }
                out.push('\n');
            }
            if !resp.files.is_empty() {
                out.push_str("Files:\n");
                for f in &resp.files {
                    out.push_str(&format!("  {} — {} (score {:.1})\n", f.path, f.desc, f.score));
                }
            }
            if resp.modules.is_empty() && resp.files.is_empty() {
                out.push_str(&format!("No results for '{raw_query}'"));
            }
            out.push_str(&format!(
                "\n({:.1}ms, searched {} files / {} modules)",
                resp.query_time, resp.total_files, resp.total_modules
            ));
            (out, false)
        }
        "cs_read_context" => {
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
                &state.project_root,
                &paths,
                &state.all_files,
                budget,
                &unit,
                query,
                &state.deps,
                &state.stub_cache,
                &*state.tokenizer,
                &config,
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
            out.push_str("**Typical workflow:** cs_find -> pick top files -> cs_read_file (stubs) -> cs_read_file (full, start_line/end_line) for the specific section you need -> cs_grep to find usages.\n");

            (out, false)
        }
        "cs_find_imports" => {
            let path = args["path"].as_str().unwrap_or("");
            let direction = args["direction"].as_str().unwrap_or("both");

            let imports = if direction == "both" || direction == "imports" {
                state
                    .import_graph
                    .imports
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            };
            let imported_by = if direction == "both" || direction == "imported_by" {
                state
                    .import_graph
                    .imported_by
                    .get(path)
                    .cloned()
                    .unwrap_or_default()
            } else {
                vec![]
            };

            if imports.is_empty() && imported_by.is_empty() {
                return (
                    format!("No import relationships found for '{path}'"),
                    false,
                );
            }

            let mut out = format!("# {path}\n\n");
            if !imports.is_empty() {
                out.push_str(&format!("Imports ({} files):\n", imports.len()));
                for inc in &imports {
                    let desc = state
                        .all_files
                        .iter()
                        .find(|f| f.rel_path == *inc)
                        .map(|f| f.desc.as_str())
                        .unwrap_or("");
                    out.push_str(&format!("  {inc}  ({desc})\n"));
                }
                out.push('\n');
            }
            if !imported_by.is_empty() {
                out.push_str(&format!("Imported by ({} files):\n", imported_by.len()));
                for inc in &imported_by {
                    let desc = state
                        .all_files
                        .iter()
                        .find(|f| f.rel_path == *inc)
                        .map(|f| f.desc.as_str())
                        .unwrap_or("");
                    out.push_str(&format!("  {inc}  ({desc})\n"));
                }
            }
            (out, false)
        }
        "cs_find" => {
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

            // 1. Fuzzy filename search
            let query = crate::fuzzy::preprocess_search_query(raw_query);
            let search_resp =
                run_search(&state.search_files, &state.search_modules, &query, limit, 0);

            // 2. Content grep
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

            // Merge results
            struct FindResult {
                path: String,
                desc: String,
                name_score: f64,
                grep_score: f64,
                grep_count: usize,
                top_match: Option<String>,
            }

            let mut merged: std::collections::HashMap<String, FindResult> =
                std::collections::HashMap::new();

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
                merged.insert(
                    f.path.clone(),
                    FindResult {
                        path: f.path.clone(),
                        desc: f.desc.clone(),
                        name_score: f.score,
                        grep_score: 0.0,
                        grep_count: 0,
                        top_match: None,
                    },
                );
            }

            // Add grep results
            if let Ok(pattern) = pattern {
                let candidates: Vec<&ScannedFile> = state
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
                                get_category_path(&f.rel_path, &config).join(" > ");
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
                    for (_i, line) in lines.iter().enumerate() {
                        if pattern.is_match(line) {
                            match_count += 1;
                            if first_match.is_none() {
                                let trimmed = if line.len() > 120 {
                                    format!("{}...", &line[..120])
                                } else {
                                    line.to_string()
                                };
                                first_match = Some(trimmed);
                            }
                        }
                    }
                    if match_count == 0 {
                        continue;
                    }

                    // BM25-lite
                    let tf = match_count as f64 / (match_count as f64 + 1.5);
                    let filename = file
                        .rel_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file.rel_path)
                        .to_lowercase();
                    let filename_bonus =
                        if terms_lower.iter().any(|t| filename.contains(t.as_str())) {
                            50.0
                        } else {
                            0.0
                        };
                    let def_bonus = if is_definition_file(&file.ext) {
                        5.0
                    } else {
                        0.0
                    };
                    let density = match_count as f64 / total_lines as f64 * 10.0;
                    let grep_score = tf * 20.0 + filename_bonus + def_bonus + density;

                    let entry = merged
                        .entry(file.rel_path.clone())
                        .or_insert_with(|| FindResult {
                            path: file.rel_path.clone(),
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

            // Unified scoring
            let mut ranked: Vec<FindResult> = merged.into_values().collect();
            ranked.sort_by(|a, b| {
                let score_a = a.name_score * 0.6 + a.grep_score * 0.4;
                let score_b = b.name_score * 0.6 + b.grep_score * 0.4;
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
                out.push_str(&format!("  {} — {}{tag_str}\n", r.path, r.desc));
                if let Some(ref line) = r.top_match {
                    out.push_str(&format!("    > {}\n", line.trim()));
                }
            }

            (out, false)
        }
        _ => (format!("Unknown tool: {name}"), true),
    }
}

// ---------------------------------------------------------------------------
// MCP stdio server loop
// ---------------------------------------------------------------------------

pub fn run_mcp(state: McpState) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();

    eprintln!(
        "MCP server ready ({} files, {} modules)",
        state.all_files.len(),
        state.manifest.len()
    );

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
                            "version": "0.2.0"
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
                let (text, is_error) = handle_tool_call(&state, tool_name, &arguments);
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
