use axum::{
    extract::{Json, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Arc;
use std::time::Instant;

use crate::budget::{allocate_budget, ContextRequest, ContextResponse};
use crate::fuzzy::{preprocess_search_query, run_search, SearchResponse};
use crate::scan::get_category_path;
use crate::stubs::extract_stubs;
use crate::types::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_definition_file(ext: &str) -> bool {
    matches!(ext, "h" | "hpp" | "hxx" | "d.ts" | "pyi")
}

// ---------------------------------------------------------------------------
// Static data endpoints
// ---------------------------------------------------------------------------

pub async fn api_tree(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        state.tree_json.clone(),
    )
}

pub async fn api_manifest(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        state.manifest_json.clone(),
    )
}

pub async fn api_deps(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [("content-type", "application/json")],
        state.deps_json.clone(),
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
    State(state): State<Arc<AppState>>,
    Query(q): Query<FileQuery>,
) -> Result<Json<FileResponse>, (StatusCode, Json<serde_json::Value>)> {
    let full_path = validate_path(&state.project_root, &q.path).map_err(|e| {
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
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchFilesRequest>,
) -> Result<Json<BatchFilesResponse>, (StatusCode, Json<serde_json::Value>)> {
    let mut files = HashMap::new();

    for p in &body.paths {
        match validate_path(&state.project_root, p) {
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
    State(state): State<Arc<AppState>>,
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
                if e.starts_with('.') {
                    e[1..].to_string()
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

    let config = ScanConfig::new(state.project_root.clone());

    // Heavy file I/O — run on blocking thread pool with rayon parallelism
    let response = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;

        let start = Instant::now();

        let candidates: Vec<&ScannedFile> = state
            .all_files
            .iter()
            .filter(|f| {
                if let Some(ref exts) = ext_filter {
                    if !exts.contains(&f.ext) {
                        return false;
                    }
                }
                if let Some(ref cat) = q.cat {
                    let file_cat = get_category_path(&f.rel_path, &config).join(" > ");
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
                    // BM25-lite relevance scoring
                    let tf = total_match_count as f64 / (total_match_count as f64 + 1.5);
                    let filename = file
                        .rel_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file.rel_path)
                        .to_lowercase();
                    let filename_bonus =
                        if terms_owned.iter().any(|t| filename.contains(t.as_str())) {
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
    State(state): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Json<SearchResponse> {
    let file_limit = q.file_limit.unwrap_or(80);
    let module_limit = q.module_limit.unwrap_or(8);
    let query = preprocess_search_query(&q.q);
    Json(run_search(
        &state.search_files,
        &state.search_modules,
        &query,
        file_limit,
        module_limit,
    ))
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
    State(state): State<Arc<AppState>>,
    Query(q): Query<ImportsQuery>,
) -> Json<ImportsResponse> {
    let direction = q.direction.as_deref().unwrap_or("both");
    let imports = if direction == "both" || direction == "imports" {
        state
            .import_graph
            .imports
            .get(&q.path)
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    let imported_by = if direction == "both" || direction == "imported_by" {
        state
            .import_graph
            .imported_by
            .get(&q.path)
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    Json(ImportsResponse {
        path: q.path,
        imports,
        imported_by,
    })
}

// ---------------------------------------------------------------------------
// Smart Context (token budget)
// ---------------------------------------------------------------------------

pub async fn api_context(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ContextRequest>,
) -> Json<ContextResponse> {
    let result = tokio::task::spawn_blocking(move || {
        let config = ScanConfig::new(state.project_root.clone());
        allocate_budget(
            &state.project_root,
            &body.paths,
            &state.all_files,
            body.budget,
            &body.unit,
            body.query.as_deref(),
            &state.deps,
            &state.stub_cache,
            &*state.tokenizer,
            &config,
        )
    })
    .await
    .unwrap();
    Json(result)
}
