use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Instant;

use crate::budget::{allocate_budget, ContextRequest, ContextResponse};
use crate::fuzzy::{preprocess_search_query, run_search, SearchResponse};
use crate::scan::get_category_path;
use crate::stubs::extract_stubs;
use crate::types::*;

/// Acquire read lock on server state, returning HTTP 500 if the lock is poisoned.
fn read_state(
    state: &std::sync::RwLock<ServerState>,
) -> Result<std::sync::RwLockReadGuard<'_, ServerState>, (StatusCode, Json<serde_json::Value>)> {
    state.read().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Internal server error" })),
        )
    })
}

// ---------------------------------------------------------------------------
// Static data endpoints (served from pre-computed HttpCache — no lock needed)
// ---------------------------------------------------------------------------

pub async fn api_tree(State(ctx): State<AppContext>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        ctx.cache.tree_json.clone(),
    )
}

pub async fn api_manifest(State(ctx): State<AppContext>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        ctx.cache.manifest_json.clone(),
    )
}

pub async fn api_deps(State(ctx): State<AppContext>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        ctx.cache.deps_json.clone(),
    )
}

// ---------------------------------------------------------------------------
// Single file read
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
}

#[derive(Serialize)]
pub(crate) struct FileResponse {
    content: String,
    lines: usize,
    size: u64,
    path: String,
    truncated: bool,
}

pub async fn api_file(
    State(ctx): State<AppContext>,
    Query(q): Query<FileQuery>,
) -> Result<Json<FileResponse>, (StatusCode, Json<serde_json::Value>)> {
    let s = read_state(&ctx.state)?;
    let repo = s.default_repo();

    let full_path = validate_path(&repo.root, &q.path).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
    })?;

    let metadata = fs::metadata(&full_path).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "File not found" })),
        )
    })?;

    let file_size = metadata.len();
    let raw = fs::read_to_string(&full_path).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Read error" })),
        )
    })?;

    let truncated = raw.len() > MAX_FILE_READ;
    let content = if truncated {
        let mut end = MAX_FILE_READ;
        while !raw.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        raw[..end].to_string()
    } else {
        raw
    };

    let lines = content.lines().count();

    Ok(Json(FileResponse {
        content,
        lines,
        size: file_size,
        path: q.path,
        truncated,
    }))
}

// ---------------------------------------------------------------------------
// Batch file read
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BatchFilesRequest {
    paths: Vec<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum BatchFileEntry {
    Ok { content: String, size: u64 },
    Err { error: String },
}

#[derive(Serialize)]
pub(crate) struct BatchFilesResponse {
    files: HashMap<String, BatchFileEntry>,
}

pub async fn api_files(
    State(ctx): State<AppContext>,
    Json(body): Json<BatchFilesRequest>,
) -> Result<Json<BatchFilesResponse>, (StatusCode, Json<serde_json::Value>)> {
    let s = read_state(&ctx.state)?;
    let repo = s.default_repo();

    let mut files = HashMap::new();

    for p in &body.paths {
        match validate_path(&repo.root, p) {
            Err(e) => {
                files.insert(
                    p.clone(),
                    BatchFileEntry::Err {
                        error: e.to_string(),
                    },
                );
            }
            Ok(full_path) => match fs::read_to_string(&full_path) {
                Err(_) => {
                    files.insert(
                        p.clone(),
                        BatchFileEntry::Err {
                            error: "Read error".to_string(),
                        },
                    );
                }
                Ok(raw) => {
                    let use_stubs = body.mode.as_deref() == Some("stubs");
                    let content = if use_stubs {
                        let ext = p.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
                        extract_stubs(&raw, ext)
                    } else {
                        raw
                    };
                    let size = content.len() as u64;
                    files.insert(p.clone(), BatchFileEntry::Ok { content, size });
                }
            },
        }
    }

    Ok(Json(BatchFilesResponse { files }))
}

// ---------------------------------------------------------------------------
// Grep
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct GrepQuery {
    q: String,
    ext: Option<String>,
    cat: Option<String>,
    limit: Option<usize>,
    #[serde(rename = "maxPerFile")]
    max_per_file: Option<usize>,
}

#[derive(Serialize)]
struct GrepMatch {
    line: String,
    #[serde(rename = "lineNum")]
    line_num: usize,
}

#[derive(Serialize)]
struct GrepFileResult {
    path: String,
    desc: String,
    matches: Vec<GrepMatch>,
    score: f64,
}

#[derive(Serialize)]
pub(crate) struct GrepResponse {
    results: Vec<GrepFileResult>,
    #[serde(rename = "totalMatches")]
    total_matches: usize,
    #[serde(rename = "searchedFiles")]
    searched_files: usize,
    #[serde(rename = "queryTime")]
    query_time: u64,
}

pub async fn api_grep(
    State(ctx): State<AppContext>,
    Query(q): Query<GrepQuery>,
) -> Result<Json<GrepResponse>, (StatusCode, Json<serde_json::Value>)> {
    if q.q.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Query must be at least 2 characters" })),
        ));
    }

    let limit = q.limit.unwrap_or(100).min(500);
    let max_per_file = q.max_per_file.unwrap_or(5);
    let ext_filter: Option<HashSet<String>> = q.ext.as_ref().map(|exts| {
        exts.split(',')
            .map(|e| {
                let e = e.trim();
                if let Some(stripped) = e.strip_prefix('.') {
                    stripped.to_string()
                } else {
                    e.to_string()
                }
            })
            .collect()
    });

    // Multi-term OR: "cloud reconstruct" -> regex `cloud|reconstruct`
    let terms: Vec<String> = q.q.split_whitespace().map(|s| s.to_string()).collect();
    let pattern_str = terms
        .iter()
        .map(|t| regex::escape(t))
        .collect::<Vec<_>>()
        .join("|");
    let pattern = RegexBuilder::new(&pattern_str)
        .case_insensitive(true)
        .build()
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid pattern" })),
            )
        })?;

    // Heavy file I/O — clone Arc, acquire read lock inside blocking closure.
    // The read() call here is safe to unwrap: lock poisoning only occurs if a
    // writer panics, and we never hold a write lock in request handlers.
    let state = ctx.state.clone();
    let response = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;

        let s = state.read().expect("state lock poisoned");
        let repo = s.default_repo();
        let start = Instant::now();

        let candidates: Vec<&ScannedFile> = repo
            .all_files
            .iter()
            .filter(|f| {
                if let Some(ref exts) = ext_filter {
                    if !exts.contains(&f.ext) {
                        return false;
                    }
                }
                if let Some(ref cat) = q.cat {
                    let file_cat = get_category_path(&f.rel_path, &repo.config).join(" > ");
                    if !file_cat.starts_with(cat.as_str()) {
                        return false;
                    }
                }
                true
            })
            .collect();

        // Parallel grep: each file processed independently
        let terms_owned: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
        let mut file_results: Vec<(GrepFileResult, usize)> = candidates
            .par_iter()
            .filter_map(|file| {
                let content = fs::read_to_string(&file.abs_path).ok()?;
                let total_lines = content.lines().count().max(1);
                let mut file_matches = Vec::new();
                let mut total_match_count = 0usize;
                for (i, line) in content.lines().enumerate() {
                    if pattern.is_match(line) {
                        total_match_count += 1;
                        if file_matches.len() < max_per_file {
                            let trimmed = if line.len() > 200 {
                                format!("{}...", &line[..200])
                            } else {
                                line.to_string()
                            };
                            file_matches.push(GrepMatch {
                                line: trimmed,
                                line_num: i + 1,
                            });
                        }
                    }
                }
                if file_matches.is_empty() {
                    None
                } else {
                    let filename = file
                        .rel_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file.rel_path)
                        .to_lowercase();
                    let score = grep_relevance_score(
                        total_match_count, total_lines, &filename, &file.ext, &terms_owned,
                    );

                    Some((
                        GrepFileResult {
                            path: file.rel_path.clone(),
                            desc: file.desc.clone(),
                            matches: file_matches,
                            score,
                        },
                        total_match_count,
                    ))
                }
            })
            .collect();

        // Sort by relevance score (descending), then truncate to limit
        file_results.sort_by(|a, b| {
            b.0.score
                .partial_cmp(&a.0.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let searched_files = candidates.len();
        let mut results = Vec::new();
        let mut total_matches = 0usize;
        for (file_result, count) in file_results {
            if total_matches >= limit {
                break;
            }
            total_matches += count;
            results.push(file_result);
        }

        let query_time = start.elapsed().as_millis() as u64;

        GrepResponse {
            results,
            total_matches,
            searched_files,
            query_time,
        }
    })
    .await
    .unwrap();

    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(rename = "fileLimit")]
    file_limit: Option<usize>,
    #[serde(rename = "moduleLimit")]
    module_limit: Option<usize>,
}

pub async fn api_search(
    State(ctx): State<AppContext>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<serde_json::Value>)> {
    let s = read_state(&ctx.state)?;
    let repo = s.default_repo();
    let file_limit = q.file_limit.unwrap_or(80);
    let module_limit = q.module_limit.unwrap_or(8);
    let query = preprocess_search_query(&q.q);
    Ok(Json(run_search(
        &repo.search_files,
        &repo.search_modules,
        &query,
        file_limit,
        module_limit,
    )))
}

// ---------------------------------------------------------------------------
// Unified Find (combined name + content search)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FindQuery {
    q: String,
    ext: Option<String>,
    cat: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct FindResultEntry {
    path: String,
    filename: String,
    dir: String,
    ext: String,
    desc: String,
    category: String,
    #[serde(rename = "nameScore")]
    name_score: f64,
    #[serde(rename = "grepScore")]
    grep_score: f64,
    #[serde(rename = "combinedScore")]
    combined_score: f64,
    #[serde(rename = "matchType")]
    match_type: String,
    #[serde(rename = "grepCount")]
    grep_count: usize,
    #[serde(rename = "topMatch")]
    top_match: Option<String>,
    #[serde(rename = "topMatchLine")]
    top_match_line: Option<usize>,
    #[serde(rename = "filenameIndices")]
    filename_indices: Vec<usize>,
}

#[derive(Serialize)]
pub(crate) struct FindResponse {
    results: Vec<FindResultEntry>,
    #[serde(rename = "queryTime")]
    query_time: u64,
    #[serde(rename = "extCounts")]
    ext_counts: HashMap<String, usize>,
    #[serde(rename = "catCounts")]
    cat_counts: HashMap<String, usize>,
}

struct MergedFind {
    path: String,
    filename: String,
    dir: String,
    ext: String,
    desc: String,
    category: String,
    name_score: f64,
    grep_score: f64,
    grep_count: usize,
    top_match: Option<String>,
    top_match_line: Option<usize>,
    filename_indices: Vec<usize>,
}

pub async fn api_find(
    State(ctx): State<AppContext>,
    Query(q): Query<FindQuery>,
) -> Result<Json<FindResponse>, (StatusCode, Json<serde_json::Value>)> {
    if q.q.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Query must be at least 1 character" })),
        ));
    }

    let limit = q.limit.unwrap_or(50).min(200);
    let ext_filter: Option<HashSet<String>> = q.ext.as_ref().map(|exts| {
        exts.split(',')
            .map(|e| {
                let e = e.trim();
                if let Some(stripped) = e.strip_prefix('.') {
                    stripped.to_string()
                } else {
                    e.to_string()
                }
            })
            .collect()
    });
    let cat_filter = q.cat.clone();
    let raw_query = q.q.clone();

    let state = ctx.state.clone();
    let response = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;

        let s = state.read().expect("state lock poisoned");
        let repo = s.default_repo();
        let start = Instant::now();

        let mut merged: HashMap<String, MergedFind> = HashMap::new();

        // 1. Fuzzy filename search
        let query = preprocess_search_query(&raw_query);
        let search_resp = run_search(&repo.search_files, &repo.search_modules, &query, limit, 0);

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
                MergedFind {
                    path: f.path.clone(),
                    filename: f.filename.clone(),
                    dir: f.dir.clone(),
                    ext: f.ext.clone(),
                    desc: f.desc.clone(),
                    category: f.category.clone(),
                    name_score: f.score,
                    grep_score: 0.0,
                    grep_count: 0,
                    top_match: None,
                    top_match_line: None,
                    filename_indices: f.filename_indices.clone(),
                },
            );
        }

        // 2. Content grep (only if query is >= 2 chars)
        if raw_query.len() >= 2 {
            let terms: Vec<String> = raw_query.split_whitespace().map(|s| s.to_string()).collect();
            let terms_lower: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
            let pattern_str = terms
                .iter()
                .map(|t| regex::escape(t))
                .collect::<Vec<_>>()
                .join("|");

            if let Ok(pattern) = RegexBuilder::new(&pattern_str)
                .case_insensitive(true)
                .build()
            {
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
                                get_category_path(&f.rel_path, &repo.config).join(" > ");
                            if !file_cat.starts_with(cat.as_str()) {
                                return false;
                            }
                        }
                        true
                    })
                    .collect();

                let grep_results: Vec<(String, f64, usize, Option<String>, Option<usize>, String, String, String, String)> = candidates
                    .par_iter()
                    .filter_map(|file| {
                        let content = fs::read_to_string(&file.abs_path).ok()?;
                        let total_lines = content.lines().count().max(1);
                        let mut match_count = 0usize;
                        let mut first_match: Option<String> = None;
                        let mut first_match_line: Option<usize> = None;
                        for (i, line) in content.lines().enumerate() {
                            if pattern.is_match(line) {
                                match_count += 1;
                                if first_match.is_none() {
                                    let trimmed = if line.len() > 120 {
                                        format!("{}...", &line[..120])
                                    } else {
                                        line.to_string()
                                    };
                                    first_match = Some(trimmed);
                                    first_match_line = Some(i + 1);
                                }
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
                            match_count, total_lines, &filename, &file.ext, &terms_lower,
                        );

                        let fname = file
                            .rel_path
                            .rsplit('/')
                            .next()
                            .unwrap_or(&file.rel_path)
                            .to_string();
                        let dir = file
                            .rel_path
                            .rsplit_once('/')
                            .map(|(d, _)| d.to_string())
                            .unwrap_or_default();
                        let ext = file.ext.clone();
                        let category = get_category_path(&file.rel_path, &repo.config).join(" > ");

                        Some((
                            file.rel_path.clone(),
                            grep_score,
                            match_count,
                            first_match,
                            first_match_line,
                            fname,
                            dir,
                            ext,
                            category,
                        ))
                    })
                    .collect();

                for (path, grep_score, match_count, first_match, first_match_line, fname, dir, ext, category) in grep_results {
                    let entry = merged.entry(path.clone()).or_insert_with(|| MergedFind {
                        path,
                        filename: fname,
                        dir,
                        ext,
                        desc: String::new(),
                        category,
                        name_score: 0.0,
                        grep_score: 0.0,
                        grep_count: 0,
                        top_match: None,
                        top_match_line: None,
                        filename_indices: Vec::new(),
                    });
                    entry.grep_score = grep_score;
                    entry.grep_count = match_count;
                    entry.top_match = first_match;
                    entry.top_match_line = first_match_line;
                }
            }
        }

        // 3. Score, sort, truncate
        let mut ranked: Vec<MergedFind> = merged.into_values().collect();
        ranked.sort_by(|a, b| {
            let score_a = a.name_score * 0.6 + a.grep_score * 0.4;
            let score_b = b.name_score * 0.6 + b.grep_score * 0.4;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        ranked.truncate(limit);

        // 4. Build response
        let mut ext_counts: HashMap<String, usize> = HashMap::new();
        let mut cat_counts: HashMap<String, usize> = HashMap::new();
        let results: Vec<FindResultEntry> = ranked
            .into_iter()
            .map(|r| {
                let combined_score = r.name_score * 0.6 + r.grep_score * 0.4;
                let match_type = if r.name_score > 0.0 && r.grep_count > 0 {
                    "both".to_string()
                } else if r.name_score > 0.0 {
                    "name".to_string()
                } else {
                    "content".to_string()
                };

                *ext_counts.entry(r.ext.clone()).or_insert(0) += 1;
                if !r.category.is_empty() {
                    *cat_counts.entry(r.category.clone()).or_insert(0) += 1;
                }

                FindResultEntry {
                    path: r.path,
                    filename: r.filename,
                    dir: r.dir,
                    ext: r.ext,
                    desc: r.desc,
                    category: r.category,
                    name_score: r.name_score,
                    grep_score: r.grep_score,
                    combined_score,
                    match_type,
                    grep_count: r.grep_count,
                    top_match: r.top_match,
                    top_match_line: r.top_match_line,
                    filename_indices: r.filename_indices,
                }
            })
            .collect();

        let query_time = start.elapsed().as_millis() as u64;

        FindResponse {
            results,
            query_time,
            ext_counts,
            cat_counts,
        }
    })
    .await
    .unwrap();

    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Import graph
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ImportsQuery {
    path: String,
    direction: Option<String>, // "imports" | "imported_by" | "both" (default)
}

#[derive(Serialize)]
pub(crate) struct ImportsResponse {
    path: String,
    imports: Vec<String>,
    #[serde(rename = "importedBy")]
    imported_by: Vec<String>,
}

pub async fn api_imports(
    State(ctx): State<AppContext>,
    Query(q): Query<ImportsQuery>,
) -> Result<Json<ImportsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let s = read_state(&ctx.state)?;
    let repo = s.default_repo();
    let direction = q.direction.as_deref().unwrap_or("both");
    let imports = if direction == "both" || direction == "imports" {
        repo.import_graph
            .imports
            .get(&q.path)
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    let imported_by = if direction == "both" || direction == "imported_by" {
        repo.import_graph
            .imported_by
            .get(&q.path)
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    Ok(Json(ImportsResponse {
        path: q.path,
        imports,
        imported_by,
    }))
}

// ---------------------------------------------------------------------------
// Smart Context (token budget)
// ---------------------------------------------------------------------------

pub async fn api_context(
    State(ctx): State<AppContext>,
    Json(body): Json<ContextRequest>,
) -> Json<ContextResponse> {
    let state = ctx.state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let s = state.read().expect("state lock poisoned");
        let repo = s.default_repo();
        allocate_budget(
            &repo.root,
            &body.paths,
            &repo.all_files,
            body.budget,
            &body.unit,
            body.query.as_deref(),
            &repo.deps,
            &repo.stub_cache,
            &*s.tokenizer,
            &repo.config,
        )
    })
    .await
    .unwrap();
    Json(result)
}
