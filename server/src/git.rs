//! Git-aware intelligence: blame, file history, changed files, and churn analysis.

use git2::{BlameOptions, Repository, Sort, Time};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct BlameLine {
    pub line: usize,
    pub author: String,
    pub date: String,
    pub commit: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
    pub files_changed: Vec<String>,
}

#[derive(Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct HotFile {
    pub path: String,
    pub commits: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_git_time(time: Time) -> String {
    let secs = time.seconds();
    // Format as ISO-ish date: YYYY-MM-DD HH:MM
    let dt = chrono_from_epoch(secs, time.offset_minutes());
    dt
}

/// Simple epoch -> date string without pulling in chrono.
fn chrono_from_epoch(epoch: i64, offset_minutes: i32) -> String {
    let adjusted = epoch + (offset_minutes as i64) * 60;
    // Days since epoch
    let days = adjusted / 86400;
    let rem = adjusted % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;

    // Calculate year/month/day from days since 1970-01-01
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{mins:02}")
}

fn days_to_ymd(mut days: i64) -> (i64, i64, i64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn status_char(delta: git2::Delta) -> &'static str {
    match delta {
        git2::Delta::Added => "added",
        git2::Delta::Deleted => "deleted",
        git2::Delta::Modified => "modified",
        git2::Delta::Renamed => "renamed",
        git2::Delta::Copied => "copied",
        git2::Delta::Typechange => "typechange",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Git blame for a file, optionally scoped to a line range.
pub fn blame(
    repo_root: &Path,
    rel_path: &str,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<Vec<BlameLine>, String> {
    let repo = Repository::open(repo_root).map_err(|e| format!("Failed to open repo: {e}"))?;

    let mut opts = BlameOptions::new();
    if let Some(s) = start {
        opts.min_line(s);
    }
    if let Some(e) = end {
        opts.max_line(e);
    }

    let blame = repo
        .blame_file(Path::new(rel_path), Some(&mut opts))
        .map_err(|e| format!("Blame failed: {e}"))?;

    // Read file content for line text
    let file_path = repo_root.join(rel_path);
    let content =
        std::fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {e}"))?;
    let lines: Vec<&str> = content.lines().collect();

    let mut result = Vec::new();
    for hunk_idx in 0..blame.len() {
        let hunk = blame.get_index(hunk_idx).unwrap();
        let sig = hunk.final_signature();
        let author = sig.name().unwrap_or("unknown").to_string();
        let commit_id = hunk.final_commit_id();
        let date = match repo.find_commit(commit_id) {
            Ok(c) => format_git_time(c.time()),
            Err(_) => "unknown".to_string(),
        };
        let short_hash = &commit_id.to_string()[..8];

        let start_line = hunk.final_start_line();
        let num_lines = hunk.lines_in_hunk();

        for i in 0..num_lines {
            let line_num = start_line + i;
            let line_content = lines.get(line_num - 1).copied().unwrap_or("").to_string();

            result.push(BlameLine {
                line: line_num,
                author: author.clone(),
                date: date.clone(),
                commit: short_hash.to_string(),
                content: line_content,
            });
        }
    }

    Ok(result)
}

/// Recent commits that touched a specific file.
pub fn file_history(
    repo_root: &Path,
    rel_path: &str,
    limit: usize,
) -> Result<Vec<CommitInfo>, String> {
    let repo = Repository::open(repo_root).map_err(|e| format!("Failed to open repo: {e}"))?;

    let mut revwalk = repo.revwalk().map_err(|e| format!("Revwalk failed: {e}"))?;
    revwalk.push_head().map_err(|e| format!("push_head failed: {e}"))?;
    revwalk.set_sorting(Sort::TIME).map_err(|e| format!("set_sorting failed: {e}"))?;

    let mut results = Vec::new();

    for oid in revwalk {
        if results.len() >= limit {
            break;
        }
        let oid = match oid {
            Ok(o) => o,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Diff this commit vs its parent to see if it touched the file
        let tree = match commit.tree() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut touched = false;
        let mut files_changed = Vec::new();

        diff.foreach(
            &mut |delta, _| {
                if let Some(path) = delta.new_file().path().and_then(|p| p.to_str()) {
                    files_changed.push(path.to_string());
                    if path == rel_path {
                        touched = true;
                    }
                }
                true
            },
            None,
            None,
            None,
        )
        .ok();

        if !touched {
            continue;
        }

        let sig = commit.author();
        results.push(CommitInfo {
            hash: oid.to_string()[..8].to_string(),
            author: sig.name().unwrap_or("unknown").to_string(),
            date: format_git_time(sig.when()),
            message: commit.message().unwrap_or("").lines().next().unwrap_or("").to_string(),
            files_changed,
        });
    }

    Ok(results)
}

/// Files changed since a given commit, branch, or tag.
pub fn changed_since(repo_root: &Path, since: &str) -> Result<Vec<ChangedFile>, String> {
    let repo = Repository::open(repo_root).map_err(|e| format!("Failed to open repo: {e}"))?;

    // Resolve the "since" reference to a commit
    let base_obj =
        repo.revparse_single(since).map_err(|e| format!("Cannot resolve '{since}': {e}"))?;
    let base_commit =
        base_obj.peel_to_commit().map_err(|e| format!("'{since}' is not a commit: {e}"))?;
    let base_tree = base_commit.tree().map_err(|e| format!("Failed to get tree: {e}"))?;

    // Get HEAD tree
    let head = repo.head().map_err(|e| format!("Failed to get HEAD: {e}"))?;
    let head_commit = head.peel_to_commit().map_err(|e| format!("HEAD is not a commit: {e}"))?;
    let head_tree = head_commit.tree().map_err(|e| format!("Failed to get HEAD tree: {e}"))?;

    let diff = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
        .map_err(|e| format!("Diff failed: {e}"))?;

    let mut results = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string();
            results.push(ChangedFile { path, status: status_char(delta.status()).to_string() });
            true
        },
        None,
        None,
        None,
    )
    .map_err(|e| format!("Diff iteration failed: {e}"))?;

    Ok(results)
}

/// Most frequently changed files (churn ranking) within recent N days.
pub fn hot_files(repo_root: &Path, limit: usize, days: usize) -> Result<Vec<HotFile>, String> {
    let repo = Repository::open(repo_root).map_err(|e| format!("Failed to open repo: {e}"))?;

    let mut revwalk = repo.revwalk().map_err(|e| format!("Revwalk failed: {e}"))?;
    revwalk.push_head().map_err(|e| format!("push_head failed: {e}"))?;
    revwalk.set_sorting(Sort::TIME).map_err(|e| format!("set_sorting failed: {e}"))?;

    // Calculate cutoff time
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
        as i64;
    let cutoff = now - (days as i64) * 86400;

    let mut file_counts: HashMap<String, usize> = HashMap::new();

    for oid in revwalk {
        let oid = match oid {
            Ok(o) => o,
            Err(_) => continue,
        };
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Stop when we pass the cutoff
        if commit.time().seconds() < cutoff {
            break;
        }

        let tree = match commit.tree() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None) {
            Ok(d) => d,
            Err(_) => continue,
        };

        diff.foreach(
            &mut |delta, _| {
                if let Some(path) = delta.new_file().path().and_then(|p| p.to_str()) {
                    *file_counts.entry(path.to_string()).or_default() += 1;
                }
                true
            },
            None,
            None,
            None,
        )
        .ok();
    }

    let mut sorted: Vec<HotFile> =
        file_counts.into_iter().map(|(path, commits)| HotFile { path, commits }).collect();
    sorted.sort_by(|a, b| b.commits.cmp(&a.commits));
    sorted.truncate(limit);

    Ok(sorted)
}
