//! FZF v2 fuzzy matching with 64-bit bitmask pre-filter for O(1) candidate rejection
//! and Smith-Waterman dynamic programming for scoring with CamelCase, delimiter, and
//! consecutive character bonuses.
//!
//! Used by `cs_search` for ranked file and module discovery.

use crate::types::{SearchFileEntry, SearchModuleEntry};
use rayon::prelude::*;
use serde::Serialize;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Scoring constants (fzf v2)
// ---------------------------------------------------------------------------

const SCORE_MATCH: i32 = 16;
const SCORE_GAP_START: i32 = -3;
const SCORE_GAP_EXTENSION: i32 = -1;
const BONUS_BOUNDARY: i32 = 8;
const BONUS_CAMEL_CASE: i32 = 7;
const BONUS_CONSECUTIVE: i32 = 4;
const BONUS_FIRST_CHAR_MULTIPLIER: i32 = 2;
const BONUS_BOUNDARY_WHITE: i32 = 10;
const BONUS_BOUNDARY_DELIMITER: i32 = 9;

// ---------------------------------------------------------------------------
// Character classification
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum CharClass {
    Lower,
    Upper,
    Digit,
    White,
    Delimiter,
    NonWord,
}

fn char_class(b: u8) -> CharClass {
    match b {
        b'a'..=b'z' => CharClass::Lower,
        b'A'..=b'Z' => CharClass::Upper,
        b'0'..=b'9' => CharClass::Digit,
        b' ' | b'\t' | b'\n' | b'\r' => CharClass::White,
        b'/' | b'_' | b'-' | b'.' | b':' | b';' | b'|' | b'>' => CharClass::Delimiter,
        _ => CharClass::NonWord,
    }
}

fn compute_bonus(prev: CharClass, curr: CharClass) -> i32 {
    match prev {
        CharClass::White => match curr {
            CharClass::White => 0,
            _ => BONUS_BOUNDARY_WHITE,
        },
        CharClass::Delimiter => match curr {
            CharClass::Delimiter => 0,
            _ => BONUS_BOUNDARY_DELIMITER,
        },
        CharClass::NonWord => match curr {
            CharClass::NonWord => 0,
            _ => BONUS_BOUNDARY,
        },
        CharClass::Lower => match curr {
            CharClass::Upper => BONUS_CAMEL_CASE,
            _ => 0,
        },
        CharClass::Digit => match curr {
            CharClass::Lower | CharClass::Upper => BONUS_BOUNDARY,
            _ => 0,
        },
        CharClass::Upper => 0,
    }
}

// ---------------------------------------------------------------------------
// Bitmask pre-filter
// ---------------------------------------------------------------------------

/// Compute a 64-bit character bitmask for O(1) rejection of non-matching candidates.
/// a-z → bits 0-25, 0-9 → bits 26-35, specials → bits 36-39
pub fn char_bitmask(s: &str) -> u64 {
    let mut mask: u64 = 0;
    for &b in s.as_bytes() {
        let idx = match b {
            b'a'..=b'z' => (b - b'a') as u32,
            b'A'..=b'Z' => (b.to_ascii_lowercase() - b'a') as u32,
            b'0'..=b'9' => (b - b'0') as u32 + 26,
            b'_' => 36,
            b'-' => 37,
            b'.' => 38,
            b'/' => 39,
            _ => continue,
        };
        mask |= 1u64 << idx;
    }
    mask
}

fn has_uppercase(s: &str) -> bool {
    s.bytes().any(|b| b.is_ascii_uppercase())
}

#[inline]
fn chars_match(text_byte: u8, pattern_byte: u8, case_sensitive: bool) -> bool {
    if case_sensitive {
        text_byte == pattern_byte
    } else {
        text_byte.eq_ignore_ascii_case(&pattern_byte)
    }
}

fn find_substring(text: &[u8], pattern: &[u8], case_sensitive: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    if pattern.len() > text.len() {
        return None;
    }
    'outer: for i in 0..=text.len() - pattern.len() {
        for (j, &pb) in pattern.iter().enumerate() {
            if !chars_match(text[i + j], pb, case_sensitive) {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

// ---------------------------------------------------------------------------
// Smith-Waterman DP fuzzy matcher (fzf v2 style)
// ---------------------------------------------------------------------------

pub(crate) fn fuzzy_score_v2(
    text: &str,
    pattern: &str,
    case_sensitive: bool,
) -> Option<(f64, Vec<usize>)> {
    if pattern.is_empty() {
        return Some((0.0, vec![]));
    }
    let tb = text.as_bytes();
    let pb = pattern.as_bytes();
    let m = pb.len();
    let n = tb.len();
    if m > n {
        return None;
    }

    // Subsequence check + bounds narrowing (left-to-right)
    let mut pi = 0;
    let mut end_bound = 0;
    for (i, &b) in tb.iter().enumerate() {
        if pi < m && chars_match(b, pb[pi], case_sensitive) {
            pi += 1;
            end_bound = i;
        }
    }
    if pi < m {
        return None;
    }

    // Tighten from right-to-left
    pi = m;
    let mut start_bound = end_bound;
    for i in (0..=end_bound).rev() {
        if pi > 0 && chars_match(tb[i], pb[pi - 1], case_sensitive) {
            pi -= 1;
            start_bound = i;
        }
    }

    let w = end_bound - start_bound + 1;

    // Bonus array for the window
    let mut bonus = vec![0i32; w];
    for (j, slot) in bonus.iter_mut().enumerate() {
        let pos = start_bound + j;
        let prev_class = if pos == 0 { CharClass::White } else { char_class(tb[pos - 1]) };
        *slot = compute_bonus(prev_class, char_class(tb[pos]));
    }

    // Fast path: exact substring match
    if let Some(sub_pos) = find_substring(&tb[start_bound..=end_bound], pb, case_sensitive) {
        let abs_pos = start_bound + sub_pos;
        let mut score = SCORE_MATCH * m as i32;
        let first_bonus = if abs_pos == 0 {
            compute_bonus(CharClass::White, char_class(tb[0]))
        } else {
            compute_bonus(char_class(tb[abs_pos - 1]), char_class(tb[abs_pos]))
        };
        score += first_bonus * BONUS_FIRST_CHAR_MULTIPLIER;
        for k in 1..m {
            let b =
                if abs_pos + k < start_bound + w { bonus[abs_pos + k - start_bound] } else { 0 };
            score += std::cmp::max(b, BONUS_CONSECUTIVE);
        }
        let indices: Vec<usize> = (abs_pos..abs_pos + m).collect();
        return Some((score as f64, indices));
    }

    // DP matrices
    let mut h = vec![i32::MIN / 2; m * w];
    let mut c = vec![0u16; m * w];
    let mut dir = vec![false; m * w];

    for i in 0..m {
        let mut in_gap = false;
        for j in 0..w {
            let pos = start_bound + j;
            let idx = i * w + j;

            if chars_match(tb[pos], pb[i], case_sensitive) {
                let mut score = SCORE_MATCH;
                let b = bonus[j];
                let prev_consec = if i > 0 && j > 0 { c[(i - 1) * w + (j - 1)] } else { 0 };

                if prev_consec > 0 {
                    score += std::cmp::max(b, BONUS_CONSECUTIVE);
                } else {
                    score += b;
                }

                if i == 0 {
                    score += b * (BONUS_FIRST_CHAR_MULTIPLIER - 1);
                }

                let diag = if i > 0 && j > 0 {
                    h[(i - 1) * w + (j - 1)]
                } else if i == 0 {
                    0
                } else {
                    i32::MIN / 2
                };

                let left = if j > 0 {
                    h[idx - 1] + if in_gap { SCORE_GAP_EXTENSION } else { SCORE_GAP_START }
                } else {
                    i32::MIN / 2
                };

                let match_score = diag.saturating_add(score);

                if match_score >= left {
                    h[idx] = match_score;
                    c[idx] = prev_consec + 1;
                    dir[idx] = true;
                } else {
                    h[idx] = left;
                    c[idx] = 0;
                    dir[idx] = false;
                }
                in_gap = false;
            } else {
                h[idx] = if j > 0 {
                    h[idx - 1] + if in_gap { SCORE_GAP_EXTENSION } else { SCORE_GAP_START }
                } else {
                    i32::MIN / 2
                };
                c[idx] = 0;
                dir[idx] = false;
                in_gap = true;
            }
        }
    }

    // Find best end position in last row
    let last_row = (m - 1) * w;
    let mut best_score = i32::MIN;
    let mut best_j = 0;
    for j in 0..w {
        if h[last_row + j] > best_score {
            best_score = h[last_row + j];
            best_j = j;
        }
    }

    if best_score <= 0 {
        return None;
    }

    // Traceback
    let mut indices = Vec::with_capacity(m);
    let mut i = m - 1;
    let mut j = best_j;
    loop {
        let idx = i * w + j;
        if dir[idx] {
            indices.push(start_bound + j);
            if i == 0 {
                break;
            }
            i -= 1;
            j -= 1;
        } else {
            if j == 0 {
                break;
            }
            j -= 1;
        }
    }
    indices.reverse();

    if indices.len() != m {
        return None;
    }

    Some((best_score as f64, indices))
}

// ---------------------------------------------------------------------------
// Search types and scoring
// ---------------------------------------------------------------------------

struct TokenInfo {
    lower: String,
    case_sensitive: bool,
    mask: u64,
}

/// A scored file match from a fuzzy search query, with match position indices.
#[derive(Serialize)]
pub struct SearchFileResult {
    pub path: String,
    pub filename: String,
    pub dir: String,
    pub ext: String,
    pub desc: String,
    pub category: String,
    pub score: f64,
    #[serde(rename = "filenameIndices")]
    pub filename_indices: Vec<usize>,
    #[serde(rename = "pathIndices")]
    pub path_indices: Vec<usize>,
}

/// A scored module match from a fuzzy search query, with match position indices.
#[derive(Serialize)]
pub struct SearchModuleResult {
    pub id: String,
    pub name: String,
    #[serde(rename = "fileCount")]
    pub file_count: usize,
    pub score: f64,
    #[serde(rename = "matchedIndices")]
    pub matched_indices: Vec<usize>,
}

/// Combined search response containing ranked file and module results with timing metadata.
#[derive(Serialize)]
pub struct SearchResponse {
    pub files: Vec<SearchFileResult>,
    pub modules: Vec<SearchModuleResult>,
    #[serde(rename = "queryTime")]
    pub query_time: f64,
    #[serde(rename = "totalFiles")]
    pub total_files: usize,
    #[serde(rename = "totalModules")]
    pub total_modules: usize,
}

fn score_module(m: &SearchModuleEntry, tokens: &[TokenInfo]) -> Option<SearchModuleResult> {
    let mut total_score = 0.0;
    let mut all_indices = Vec::new();

    for token in tokens {
        let (text, pattern) = if token.case_sensitive {
            (&m.name, token.lower.as_str())
        } else {
            (&m.name_lower, token.lower.as_str())
        };

        let name_passes = (token.mask & m.name_mask) == token.mask;
        if name_passes {
            if let Some((score, indices)) = fuzzy_score_v2(text, pattern, token.case_sensitive) {
                total_score += score * 2.0;
                all_indices.extend(indices);
                continue;
            }
        }

        let text = if token.case_sensitive { &m.id } else { &m.id_lower };
        let id_passes = (token.mask & m.id_mask) == token.mask;
        if id_passes {
            if let Some((score, indices)) = fuzzy_score_v2(text, pattern, token.case_sensitive) {
                total_score += score;
                all_indices.extend(indices);
                continue;
            }
        }

        return None;
    }

    total_score += (m.file_count as f64 + 1.0).log2() * 2.0;

    Some(SearchModuleResult {
        id: m.id.clone(),
        name: m.name.clone(),
        file_count: m.file_count,
        score: total_score,
        matched_indices: all_indices,
    })
}

fn score_file(f: &SearchFileEntry, tokens: &[TokenInfo]) -> Option<SearchFileResult> {
    let mut total_score = 0.0;
    let mut filename_indices = Vec::new();
    let mut path_indices = Vec::new();

    for token in tokens {
        let pattern = &token.lower;

        let fname_text = if token.case_sensitive { &f.filename } else { &f.filename_lower };
        let fname_passes = (token.mask & f.filename_mask) == token.mask;
        if fname_passes {
            // Exact filename match bonus: if the query matches the filename stem exactly,
            // give a massive score so it always ranks first
            let stem =
                f.filename_lower.rsplit_once('.').map(|(s, _)| s).unwrap_or(&f.filename_lower);
            if pattern == stem || pattern == &f.filename_lower {
                // Exact match — give maximum possible score
                total_score += 10000.0;
                filename_indices.extend(0..f.filename.len());
                continue;
            }
            // Prefix match bonus: "Actor" matching "Actor.h" stem "actor"
            if stem.starts_with(pattern.as_str()) && pattern.len() >= 3 {
                total_score += 5000.0 + (pattern.len() as f64 / stem.len() as f64) * 1000.0;
                filename_indices.extend(0..pattern.len());
                continue;
            }

            if let Some((score, indices)) =
                fuzzy_score_v2(fname_text, pattern, token.case_sensitive)
            {
                total_score += score * 2.0;
                filename_indices.extend(indices);
                continue;
            }
        }

        let path_text = if token.case_sensitive { &f.path } else { &f.path_lower };
        let path_passes = (token.mask & f.path_mask) == token.mask;
        if path_passes {
            if let Some((score, indices)) = fuzzy_score_v2(path_text, pattern, token.case_sensitive)
            {
                total_score += score;
                path_indices.extend(indices);
                continue;
            }
        }

        let desc_passes = (token.mask & f.desc_mask) == token.mask;
        if desc_passes {
            let desc_text = if token.case_sensitive { &f.desc } else { &f.desc_lower };
            if let Some((score, _)) = fuzzy_score_v2(desc_text, pattern, token.case_sensitive) {
                total_score += score * 0.5;
                continue;
            }
        }

        return None;
    }

    Some(SearchFileResult {
        path: f.path.clone(),
        filename: f.filename.clone(),
        dir: f.dir.clone(),
        ext: f.ext.clone(),
        desc: f.desc.clone(),
        category: f.category.clone(),
        score: total_score,
        filename_indices,
        path_indices,
    })
}

// ---------------------------------------------------------------------------
// Public search entry point
// ---------------------------------------------------------------------------

/// Execute a fuzzy search query against the file and module indexes, returning ranked results.
pub fn run_search(
    search_files: &[SearchFileEntry],
    search_modules: &[SearchModuleEntry],
    query: &str,
    file_limit: usize,
    module_limit: usize,
) -> SearchResponse {
    let start = Instant::now();
    let trimmed = query.trim();

    if trimmed.is_empty() {
        return SearchResponse {
            files: vec![],
            modules: vec![],
            query_time: 0.0,
            total_files: search_files.len(),
            total_modules: search_modules.len(),
        };
    }

    let tokens: Vec<TokenInfo> = trimmed
        .split_whitespace()
        .map(|t| {
            let case_sensitive = has_uppercase(t);
            let lower = t.to_lowercase();
            let mask = char_bitmask(&lower);
            TokenInfo { lower, case_sensitive, mask }
        })
        .collect();

    let mut module_results: Vec<SearchModuleResult> =
        search_modules.par_iter().filter_map(|m| score_module(m, &tokens)).collect();
    module_results.sort_unstable_by(|a, b| {
        b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
    });
    module_results.truncate(module_limit);

    let mut file_results: Vec<SearchFileResult> =
        search_files.par_iter().filter_map(|f| score_file(f, &tokens)).collect();

    if file_results.len() > file_limit {
        file_results.select_nth_unstable_by(file_limit, |a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
        });
        file_results.truncate(file_limit);
    }
    file_results.sort_unstable_by(|a, b| {
        b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
    });

    let query_time = start.elapsed().as_secs_f64() * 1000.0;

    SearchResponse {
        files: file_results,
        modules: module_results,
        query_time,
        total_files: search_files.len(),
        total_modules: search_modules.len(),
    }
}

// ---------------------------------------------------------------------------
// Query preprocessing
// ---------------------------------------------------------------------------

const KNOWN_EXTS: &[&str] = &[
    "h", "hpp", "hxx", "cpp", "cxx", "cc", "c", "cs", "py", "rb", "lua", "ini", "cfg", "conf",
    "toml", "yaml", "yml", "json", "xml", "usf", "ush", "hlsl", "glsl", "vert", "frag", "comp",
    "wgsl", "js", "ts", "jsx", "tsx", "mjs", "cjs", "rs", "go", "java", "kt", "scala", "swift",
    "css", "scss", "less", "sass", "html", "htm", "vue", "svelte", "sh", "bash", "zsh", "ps1",
    "psm1", "psd1", "bat", "cmd", "md", "rst", "txt", "adoc", "cmake", "make", "gradle", "csproj",
    "sln",
];

/// Strip known file extensions from search tokens so fuzzy search matches the stem.
/// For example, "VolumetricCloudRendering.usf" becomes "VolumetricCloudRendering".
pub fn preprocess_search_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| {
            if let Some((stem, ext)) = token.rsplit_once('.') {
                if KNOWN_EXTS.contains(&ext) && !stem.is_empty() {
                    return stem;
                }
            }
            token
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_filename_match_scores_highest() {
        let files = vec![SearchFileEntry {
            path: "src/api.rs".into(),
            path_lower: "src/api.rs".into(),
            filename: "api.rs".into(),
            filename_lower: "api.rs".into(),
            dir: "src".into(),
            ext: "rs".into(),
            desc: "API handler".into(),
            desc_lower: "api handler".into(),
            category: "src".into(),
            filename_mask: char_bitmask("api.rs"),
            path_mask: char_bitmask("src/api.rs"),
            desc_mask: char_bitmask("api handler"),
        }];
        let result = run_search(&files, &[], "api", 10, 10);
        assert_eq!(result.files.len(), 1);
        // Exact stem match should give the 10000.0 bonus
        assert!(
            result.files[0].score >= 10000.0,
            "exact match score {} should be >= 10000",
            result.files[0].score
        );
    }

    #[test]
    fn prefix_match_scores_higher_than_substring() {
        let prefix_file = SearchFileEntry {
            path: "src/Actor.h".into(),
            path_lower: "src/actor.h".into(),
            filename: "Actor.h".into(),
            filename_lower: "actor.h".into(),
            dir: "src".into(),
            ext: "h".into(),
            desc: "".into(),
            desc_lower: "".into(),
            category: "src".into(),
            filename_mask: char_bitmask("actor.h"),
            path_mask: char_bitmask("src/actor.h"),
            desc_mask: 0,
        };
        let substring_file = SearchFileEntry {
            path: "src/MyActorComponent.h".into(),
            path_lower: "src/myactorcomponent.h".into(),
            filename: "MyActorComponent.h".into(),
            filename_lower: "myactorcomponent.h".into(),
            dir: "src".into(),
            ext: "h".into(),
            desc: "".into(),
            desc_lower: "".into(),
            category: "src".into(),
            filename_mask: char_bitmask("myactorcomponent.h"),
            path_mask: char_bitmask("src/myactorcomponent.h"),
            desc_mask: 0,
        };
        let result = run_search(&[prefix_file, substring_file], &[], "actor", 10, 10);
        assert!(result.files.len() == 2);
        // Prefix match (Actor.h) should rank first
        assert_eq!(result.files[0].filename, "Actor.h");
        assert!(result.files[0].score > result.files[1].score);
    }

    #[test]
    fn camelcase_boundary_bonus() {
        // "SM" should match "SearchModule" via CamelCase boundaries
        let score = fuzzy_score_v2("SearchModule", "SM", true);
        assert!(score.is_some(), "CamelCase pattern SM should match SearchModule");
        let (s, _) = score.unwrap();
        assert!(s > 0.0, "CamelCase match should have positive score");
    }

    #[test]
    fn non_matching_returns_none() {
        let score = fuzzy_score_v2("hello", "xyz", false);
        assert!(score.is_none(), "non-matching pattern should return None");
    }

    #[test]
    fn empty_query_returns_empty_results() {
        let result = run_search(&[], &[], "", 10, 10);
        assert!(result.files.is_empty());
        assert!(result.modules.is_empty());
    }
}
