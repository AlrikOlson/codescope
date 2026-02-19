use crate::scan::get_category_path;
use crate::stubs::{extract_stubs, extract_tier4, parse_blocks, BlockKind, StubBlock};
use crate::tokenizer::Tokenizer;
use crate::types::{validate_path, CachedStub, DepEntry, ScanConfig, ScannedFile};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Default token budget for context requests when no explicit budget is provided.
pub const DEFAULT_TOKEN_BUDGET: usize = 50_000;

// ---------------------------------------------------------------------------
// Importance scoring
// ---------------------------------------------------------------------------

/// Static importance score (extension type, structural keywords, file size).
/// This is a small baseline — the cohesion score from the selection dominates.
pub fn importance_score(rel_path: &str, file_size: u64) -> f64 {
    let ext = rel_path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let filename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);

    // Headers/interfaces are more structurally useful than implementations
    let ext_weight: f64 = match ext {
        // Headers / interfaces / type definitions
        "h" | "hpp" | "hxx" | "d.ts" | "pyi" => 0.30,
        // Build / project files
        "cs" | "csproj" | "sln" | "cmake" | "gradle" => 0.20,
        // Primary source
        "rs" | "go" | "java" | "kt" | "scala" | "swift" | "ts" | "tsx" => 0.15,
        // Implementation / secondary source
        "cpp" | "cxx" | "cc" | "c" | "js" | "jsx" | "mjs" | "cjs" | "py" | "rb" => 0.12,
        // Shaders
        "usf" | "ush" | "hlsl" | "glsl" | "vert" | "frag" | "comp" | "wgsl" => 0.12,
        // Config
        "ini" | "cfg" | "toml" | "yaml" | "yml" | "json" | "xml" => 0.05,
        // Docs
        "md" | "rst" | "txt" | "adoc" => 0.03,
        // Unknown
        _ => 0.08,
    };

    let name_upper = stem.to_uppercase();
    let keywords: &[(&str, f64)] = &[
        ("MOD", 0.05),
        ("MODULE", 0.1),
        ("INTERFACE", 0.1),
        ("BASE", 0.05),
        ("TYPES", 0.05),
        ("INDEX", 0.05),
        ("LIB", 0.05),
        ("MAIN", 0.1),
        ("API", 0.1),
        ("SCHEMA", 0.05),
        ("MODEL", 0.05),
    ];
    let mut name_bonus = 0.0_f64;
    for (kw, bonus) in keywords {
        if name_upper.contains(kw) {
            name_bonus += bonus;
        }
    }
    name_bonus = name_bonus.min(0.2);

    let size_bonus: f64 = if file_size < 5_000 {
        0.05
    } else if file_size < 20_000 {
        0.02
    } else {
        0.0
    };

    ext_weight + name_bonus + size_bonus
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unit of measurement for the context budget: token count or raw character count.
#[derive(Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BudgetUnit {
    #[default]
    Tokens,
    Chars,
}

/// Incoming request for budget-aware file context, specifying paths, budget, and optional query.
#[derive(Deserialize)]
pub struct ContextRequest {
    pub paths: Vec<String>,
    #[serde(default = "default_budget")]
    pub budget: usize,
    #[serde(default)]
    pub unit: BudgetUnit,
    /// Original search query — used to rank results by relevance
    #[serde(default)]
    pub query: Option<String>,
}

fn default_budget() -> usize {
    DEFAULT_TOKEN_BUDGET
}

#[derive(Serialize)]
pub struct ContextFileEntry {
    pub content: String,
    pub tier: u8,
    pub tokens: usize,
    pub importance: f64,
    pub order: u32,
}

#[derive(Serialize)]
pub struct ContextSummary {
    #[serde(rename = "totalTokens")]
    pub total_tokens: usize,
    #[serde(rename = "totalChars")]
    pub total_chars: usize,
    pub budget: usize,
    pub unit: String,
    #[serde(rename = "tierCounts")]
    pub tier_counts: HashMap<String, usize>,
    #[serde(rename = "totalFiles")]
    pub total_files: usize,
}

#[derive(Serialize)]
pub struct ContextResponse {
    pub files: HashMap<String, ContextFileEntry>,
    pub summary: ContextSummary,
}

struct BudgetFile {
    path: String,
    desc: String,
    ext: String,
    importance: f64,
    tier1_content: Arc<str>,
    /// The final content for the current tier, kept in sync during demotion
    current_content: String,
    current_tier: u8,
    current_cost: usize,
}

/// Cost measurement using the pluggable tokenizer.
fn measure(content: &str, unit: &BudgetUnit, tokenizer: &dyn Tokenizer) -> usize {
    match unit {
        BudgetUnit::Tokens => tokenizer.count_tokens(content),
        BudgetUnit::Chars => content.len(),
    }
}

// ---------------------------------------------------------------------------
// Intermediate struct for parallel file loading
// ---------------------------------------------------------------------------

struct LoadedFile {
    path: String,
    desc: String,
    ext: String,
    importance: f64,
    tier1_content: Arc<str>,
    tier1_cost: usize,
}

enum LoadResult {
    Ok(LoadedFile),
    Err(String, ContextFileEntry),
}

/// Compute importance from query terms + static heuristics.
fn compute_importance(path: &str, raw: &str, file_size: u64, query_terms: &[String]) -> f64 {
    let path_lower = path.to_lowercase();
    let mut query_path_score = 0.0_f64;
    for qt in query_terms {
        if path_lower.contains(qt.as_str()) {
            query_path_score += 10.0;
        }
    }

    let preview_end = raw.len().min(4000);
    let content_lower = raw[..preview_end].to_lowercase();
    let mut query_content_score = 0.0_f64;
    for qt in query_terms {
        if content_lower.contains(qt.as_str()) {
            query_content_score += 3.0;
        }
    }

    query_path_score + query_content_score + importance_score(path, file_size)
}

// ---------------------------------------------------------------------------
// Dependency connectivity
// ---------------------------------------------------------------------------

/// Find the dep module name for a file path by matching its category against
/// dep module category_paths. Returns the module with the longest matching prefix.
fn file_to_module<'a>(
    file_path: &str,
    dep_cat_prefixes: &'a [(String, String)], // (module_name, category_path)
    config: &ScanConfig,
) -> Option<&'a str> {
    let file_cat = get_category_path(file_path, config).join(" > ");
    let mut best: Option<&str> = None;
    let mut best_len = 0;
    for (module_name, cat_path) in dep_cat_prefixes {
        if (file_cat == *cat_path || file_cat.starts_with(&format!("{cat_path} > ")))
            && cat_path.len() > best_len
        {
            best = Some(module_name.as_str());
            best_len = cat_path.len();
        }
    }
    best
}

/// Build reverse dependency index: module_name -> set of modules that depend on it.
fn build_reverse_deps(deps: &BTreeMap<String, DepEntry>) -> HashMap<String, HashSet<String>> {
    let mut rev: HashMap<String, HashSet<String>> = HashMap::new();
    for (module, entry) in deps {
        for dep in entry.public.iter().chain(entry.private.iter()) {
            rev.entry(dep.clone())
                .or_default()
                .insert(module.clone());
        }
    }
    rev
}

// ---------------------------------------------------------------------------
// Block scoring + water-fill allocation + block pruning
// ---------------------------------------------------------------------------

/// Score a stub block by type priority + query relevance.
fn score_block(block: &StubBlock, query_terms: &[String]) -> f64 {
    let base = match block.kind {
        BlockKind::FunctionSig => 3.0,
        BlockKind::ClassDecl => 2.5,
        BlockKind::MacroDecl | BlockKind::AnnotatedBlock => 1.5,
        BlockKind::Misc | BlockKind::IncludeGroup => 0.5,
    };

    let mut query_bonus = 0.0;
    for term in query_terms {
        if !block.identifier.is_empty() && block.identifier.contains(term.as_str()) {
            query_bonus += 10.0;
        } else {
            let lower = block.full_text.to_lowercase();
            if lower.contains(term.as_str()) {
                query_bonus += 3.0;
            }
        }
    }

    base + query_bonus
}

/// Water-fill budget allocation: distribute tokens proportionally by importance.
/// Returns per-file total budgets (0 = manifest, >0 = stubs/pruned content budget).
fn allocate_file_budgets(
    files: &[(f64, usize, usize)], // (importance, tier1_cost, manifest_cost)
    total_budget: usize,
) -> Vec<usize> {
    let n = files.len();
    if n == 0 {
        return vec![];
    }

    // All files start as manifest; upgrades cost additional tokens
    let total_manifest: usize = files.iter().map(|(_, _, mc)| mc).sum();
    let upgrade_budget = total_budget.saturating_sub(total_manifest);
    if upgrade_budget == 0 {
        return vec![0; n];
    }

    let min_useful_upgrade = 30usize;
    let weights: Vec<f64> = files.iter().map(|(imp, _, _)| imp.powf(1.5)).collect();
    let total_weight: f64 = weights.iter().sum();

    let mut budgets = vec![0usize; n];
    let mut locked = vec![false; n];
    let mut remaining = upgrade_budget;
    let mut remaining_weight = total_weight;

    if remaining_weight <= 0.0 {
        return budgets;
    }

    // Iterative convergence: lock files that clearly fit or clearly don't
    for _ in 0..5 {
        let mut changed = false;
        for i in 0..n {
            if locked[i] {
                continue;
            }
            if remaining_weight <= 0.0 || remaining == 0 {
                break;
            }

            let ideal_upgrade = (weights[i] / remaining_weight * remaining as f64) as usize;
            let tier1_cost = files[i].1;
            let manifest_cost = files[i].2;
            let upgrade_cost = tier1_cost.saturating_sub(manifest_cost);

            if ideal_upgrade >= upgrade_cost {
                // Full stubs: lock at tier1 cost, free surplus upgrade budget
                budgets[i] = tier1_cost;
                remaining = remaining.saturating_sub(upgrade_cost);
                remaining_weight -= weights[i];
                locked[i] = true;
                changed = true;
            } else if ideal_upgrade < min_useful_upgrade {
                // Too small to be useful — stays manifest
                budgets[i] = 0;
                remaining_weight -= weights[i];
                locked[i] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Remaining unlocked files: split leftover upgrade budget proportionally
    let unlocked_weight: f64 = (0..n)
        .filter(|&i| !locked[i])
        .map(|i| weights[i])
        .sum();

    if unlocked_weight > 0.0 && remaining > 0 {
        for i in 0..n {
            if locked[i] {
                continue;
            }
            let upgrade_share = (weights[i] / unlocked_weight * remaining as f64) as usize;
            // Total file budget = freed manifest slot + upgrade share
            budgets[i] = files[i].2 + upgrade_share;
        }
    }

    budgets
}

/// Prune blocks within a file to fit its allocated budget.
/// Keeps the highest-scored blocks (full or summary), preserving original order.
fn prune_blocks(blocks: &[StubBlock], query_terms: &[String], file_budget: usize) -> String {
    let mut scored: Vec<(usize, f64)> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (i, score_block(b, query_terms)))
        .collect();

    // Sort by score descending — highest priority blocks first
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected: Vec<(usize, String)> = Vec::new();
    let mut used = 0;

    for (idx, _score) in &scored {
        let block = &blocks[*idx];
        if used + block.full_tokens <= file_budget {
            selected.push((*idx, block.full_text.clone()));
            used += block.full_tokens;
        } else if block.summary_tokens > 0 && used + block.summary_tokens <= file_budget {
            selected.push((*idx, block.summary_text.clone()));
            used += block.summary_tokens;
        }
        // else: block doesn't fit at all — skip
    }

    // Re-sort by original position to preserve file structure
    selected.sort_by_key(|(idx, _)| *idx);

    selected
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("")
}

// ---------------------------------------------------------------------------
// Budget allocation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn allocate_budget(
    project_root: &Path,
    paths: &[String],
    all_files: &[ScannedFile],
    budget: usize,
    unit: &BudgetUnit,
    query: Option<&str>,
    deps: &BTreeMap<String, DepEntry>,
    stub_cache: &dashmap::DashMap<String, CachedStub>,
    tokenizer: &dyn Tokenizer,
    config: &ScanConfig,
) -> ContextResponse {
    let desc_map: HashMap<&str, &str> = all_files
        .iter()
        .map(|f| (f.rel_path.as_str(), f.desc.as_str()))
        .collect();

    // Query terms are the primary relevance signal
    let query_terms: Vec<String> = query
        .map(|q| {
            q.split_whitespace()
                .filter(|w| w.len() >= 2)
                .map(|w| w.to_lowercase())
                .collect()
        })
        .unwrap_or_default();

    // Build dep connectivity structures (only if we have a query)
    let dep_cat_prefixes: Vec<(String, String)> = deps
        .iter()
        .map(|(name, entry)| (name.clone(), entry.category_path.clone()))
        .collect();
    let reverse_deps = build_reverse_deps(deps);

    // Phase 1: Load files in parallel, using cache for stubs + reads
    let load_results: Vec<LoadResult> = paths
        .par_iter()
        .map(|p| {
            let ext = p.rsplit_once('.').map(|(_, e)| e).unwrap_or("").to_string();
            let desc = desc_map.get(p.as_str()).copied().unwrap_or("").to_string();

            // Check cache first
            if let Some(cached) = stub_cache.get(p.as_str()) {
                let file_size = cached.raw.len() as u64;
                let cost = match unit {
                    BudgetUnit::Tokens => cached.fast_tokens,
                    BudgetUnit::Chars => cached.tier1.len(),
                };

                let importance = compute_importance(p, &cached.raw, file_size, &query_terms);

                return LoadResult::Ok(LoadedFile {
                    path: p.clone(),
                    desc,
                    ext,
                    importance,
                    tier1_content: Arc::clone(&cached.tier1),
                    tier1_cost: cost,
                });
            }

            // Cache miss: read from disk, compute stubs, cache result
            match validate_path(project_root, p) {
                Err(e) => LoadResult::Err(
                    p.clone(),
                    ContextFileEntry {
                        content: format!("Error: {e}"),
                        tier: 0,
                        tokens: 0,
                        importance: 0.0,
                        order: u32::MAX,
                    },
                ),
                Ok(full_path) => match fs::read_to_string(&full_path) {
                    Err(_) => LoadResult::Err(
                        p.clone(),
                        ContextFileEntry {
                            content: "Error: could not read file".into(),
                            tier: 0,
                            tokens: 0,
                            importance: 0.0,
                            order: u32::MAX,
                        },
                    ),
                    Ok(raw) => {
                        let file_size = raw.len() as u64;
                        let tier1 = extract_stubs(&raw, &ext);
                        let fast_tokens = tokenizer.count_tokens(&tier1);
                        let cost = match unit {
                            BudgetUnit::Tokens => fast_tokens,
                            BudgetUnit::Chars => tier1.len(),
                        };

                        let raw_arc: Arc<str> = Arc::from(raw.as_str());
                        let tier1_arc: Arc<str> = Arc::from(tier1.as_str());

                        // Store in cache
                        stub_cache.insert(
                            p.clone(),
                            CachedStub {
                                raw: Arc::clone(&raw_arc),
                                tier1: Arc::clone(&tier1_arc),
                                fast_tokens,
                            },
                        );

                        let importance =
                            compute_importance(p, &raw, file_size, &query_terms);

                        LoadResult::Ok(LoadedFile {
                            path: p.clone(),
                            desc,
                            ext,
                            importance,
                            tier1_content: tier1_arc,
                            tier1_cost: cost,
                        })
                    }
                },
            }
        })
        .collect();

    // Separate into files and errors
    let mut files: Vec<BudgetFile> = Vec::with_capacity(paths.len());
    let mut errors: HashMap<String, ContextFileEntry> = HashMap::new();

    for result in load_results {
        match result {
            LoadResult::Err(path, entry) => {
                errors.insert(path, entry);
            }
            LoadResult::Ok(loaded) => {
                let current_content = loaded.tier1_content.to_string();
                files.push(BudgetFile {
                    path: loaded.path,
                    desc: loaded.desc,
                    ext: loaded.ext,
                    importance: loaded.importance,
                    tier1_content: loaded.tier1_content,
                    current_content,
                    current_tier: 1,
                    current_cost: loaded.tier1_cost,
                });
            }
        }
    }

    // Phase 1b: Dependency connectivity bonus
    if !query_terms.is_empty() && !deps.is_empty() {
        let query_threshold = 3.0;
        let mut matched_modules: HashSet<String> = HashSet::new();
        for file in &files {
            if file.importance >= query_threshold {
                if let Some(m) = file_to_module(&file.path, &dep_cat_prefixes, config) {
                    matched_modules.insert(m.to_string());
                }
            }
        }

        if !matched_modules.is_empty() {
            let mut connected: HashSet<String> = HashSet::new();
            for m in &matched_modules {
                if let Some(entry) = deps.get(m) {
                    for dep in entry.public.iter().chain(entry.private.iter()) {
                        if !matched_modules.contains(dep) {
                            connected.insert(dep.clone());
                        }
                    }
                }
                if let Some(dependents) = reverse_deps.get(m) {
                    for dep in dependents {
                        if !matched_modules.contains(dep) {
                            connected.insert(dep.clone());
                        }
                    }
                }
            }

            for file in files.iter_mut() {
                if let Some(m) = file_to_module(&file.path, &dep_cat_prefixes, config) {
                    if connected.contains(m) {
                        file.importance += 5.0;
                    }
                }
            }
        }
    }

    // Phase 2: Check budget — if T1 fits, we're done
    let mut total: usize = files.iter().map(|f| f.current_cost).sum();
    if total <= budget {
        return build_context_response(files, errors, budget, unit, tokenizer);
    }

    // Phase 3: Water-fill budget allocation — distribute tokens by importance
    let file_specs: Vec<(f64, usize, usize)> = files
        .iter()
        .map(|f| {
            let manifest_cost = measure(&extract_tier4(&f.path, &f.desc), unit, tokenizer);
            (f.importance, f.current_cost, manifest_cost)
        })
        .collect();
    let file_budgets = allocate_file_budgets(&file_specs, budget);

    // Phase 4: Apply per-file budgets via block pruning
    for (idx, file) in files.iter_mut().enumerate() {
        let fb = file_budgets[idx];
        if fb == 0 {
            // Manifest
            file.current_content = extract_tier4(&file.path, &file.desc);
            file.current_tier = 4;
            file.current_cost = measure(&file.current_content, unit, tokenizer);
        } else if fb >= file.current_cost {
            // Full stubs (tier 1) — keep as-is
        } else {
            // Pruned — parse blocks and keep top blocks within budget
            let blocks = parse_blocks(&file.tier1_content, &file.ext);
            let pruned = prune_blocks(&blocks, &query_terms, fb);
            if !pruned.trim().is_empty() {
                file.current_content = pruned;
                file.current_tier = 2;
                file.current_cost = measure(&file.current_content, unit, tokenizer);
            } else {
                file.current_content = extract_tier4(&file.path, &file.desc);
                file.current_tier = 4;
                file.current_cost = measure(&file.current_content, unit, tokenizer);
            }
        }
    }

    // Phase 5: Safety valve — if manifest overhead pushed over budget, trim
    total = files.iter().map(|f| f.current_cost).sum();
    if total > budget {
        files.sort_by(|a, b| {
            a.importance
                .partial_cmp(&b.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for file in files.iter_mut() {
            if total <= budget {
                break;
            }
            if file.current_tier >= 4 {
                continue;
            }
            let old_cost = file.current_cost;
            file.current_content = extract_tier4(&file.path, &file.desc);
            file.current_tier = 4;
            file.current_cost = measure(&file.current_content, unit, tokenizer);
            total = total - old_cost + file.current_cost;
        }
    }

    build_context_response(files, errors, budget, unit, tokenizer)
}

fn build_context_response(
    mut files: Vec<BudgetFile>,
    errors: HashMap<String, ContextFileEntry>,
    budget: usize,
    unit: &BudgetUnit,
    tokenizer: &dyn Tokenizer,
) -> ContextResponse {
    // Demote files whose current content is empty to tier 4 (manifest line)
    for file in files.iter_mut() {
        if file.current_tier < 4 && file.current_content.trim().is_empty() {
            file.current_content = extract_tier4(&file.path, &file.desc);
            file.current_tier = 4;
            file.current_cost = measure(&file.current_content, unit, tokenizer);
        }
    }

    // Sort by importance descending (query relevance dominates)
    files.sort_by(|a, b| {
        b.importance
            .partial_cmp(&a.importance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut result_files: HashMap<String, ContextFileEntry> = HashMap::new();
    let mut tier_counts: HashMap<String, usize> = HashMap::new();
    let mut total_tokens = 0usize;
    let mut total_chars = 0usize;
    let mut order = 0u32;

    for (path, mut entry) in errors {
        entry.order = u32::MAX;
        result_files.insert(path, entry);
    }

    for file in files {
        let tier = file.current_tier;
        *tier_counts.entry(tier.to_string()).or_insert(0) += 1;

        let content = file.current_content;
        let chars = content.len();
        let tok = tokenizer.count_tokens(&content);
        total_tokens += tok;
        total_chars += chars;

        result_files.insert(
            file.path,
            ContextFileEntry {
                content,
                tier,
                tokens: tok,
                importance: file.importance,
                order,
            },
        );
        order += 1;
    }

    let unit_str = match unit {
        BudgetUnit::Tokens => "tokens",
        BudgetUnit::Chars => "chars",
    };

    ContextResponse {
        summary: ContextSummary {
            total_tokens,
            total_chars,
            budget,
            unit: unit_str.to_string(),
            tier_counts,
            total_files: result_files.len(),
        },
        files: result_files,
    }
}
