//! File watcher for incremental live re-indexing.
//!
//! Watches all indexed repo roots for file changes and incrementally updates
//! the search index, manifest, and import graph without requiring a full rescan.

use crate::scan::{
    build_search_index, process_single_file, remove_manifest_entry, update_import_edges_for_file,
    update_manifest_entry,
};
use crate::types::ServerState;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Debounce window: wait this long after the last event before processing.
const DEBOUNCE_MS: u64 = 500;

/// Start a file watcher on all indexed repo roots. Returns the watcher handle
/// (must be kept alive — dropping it stops the watcher).
pub fn start_watcher(state: Arc<RwLock<ServerState>>) -> Option<RecommendedWatcher> {
    let (tx, rx) = mpsc::channel::<Event>();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("  [watch] Failed to create file watcher: {e}");
            return None;
        }
    };

    // Watch all repo roots
    {
        let s = state.read().unwrap();
        for repo in s.repos.values() {
            if let Err(e) = watcher.watch(&repo.root, RecursiveMode::Recursive) {
                eprintln!("  [watch] Failed to watch {}: {e}", repo.root.display());
            } else {
                eprintln!("  [watch] Watching {}", repo.root.display());
            }
        }
    }

    // Spawn debounce processor thread
    let state_clone = Arc::clone(&state);
    std::thread::spawn(move || {
        debounce_loop(rx, state_clone);
    });

    Some(watcher)
}

/// Collect file events and process them after a debounce period of quiet.
fn debounce_loop(rx: mpsc::Receiver<Event>, state: Arc<RwLock<ServerState>>) {
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        // Wait for events with a timeout
        match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
            Ok(event) => {
                let dominated_by_kind = matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                );
                if dominated_by_kind {
                    let now = Instant::now();
                    for path in event.paths {
                        pending.insert(path, now);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Check if any pending events are old enough to process
                if pending.is_empty() {
                    continue;
                }
                let cutoff = Instant::now() - Duration::from_millis(DEBOUNCE_MS);
                let ready: Vec<PathBuf> =
                    pending.iter().filter(|(_, t)| **t <= cutoff).map(|(p, _)| p.clone()).collect();

                if ready.is_empty() {
                    continue;
                }

                for path in &ready {
                    pending.remove(path);
                }

                process_changes(&ready, &state);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
    }
}

/// Process a batch of changed file paths, updating indexes incrementally.
fn process_changes(paths: &[PathBuf], state: &Arc<RwLock<ServerState>>) {
    // Read state to determine which repo owns each path and gather configs
    let s = state.read().unwrap();

    // Group paths by repo
    let mut repo_changes: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in paths {
        for repo in s.repos.values() {
            if path.starts_with(&repo.root) {
                repo_changes.entry(repo.name.clone()).or_default().push(path.clone());
                break;
            }
        }
    }
    drop(s); // release read lock

    if repo_changes.is_empty() {
        return;
    }

    // Process each repo's changes
    let mut state_w = state.write().unwrap();

    for (repo_name, changed_paths) in &repo_changes {
        let repo = match state_w.repos.get_mut(repo_name) {
            Some(r) => r,
            None => continue,
        };

        let mut changed_count = 0usize;
        let mut removed_count = 0usize;

        for abs_path in changed_paths {
            let rel_path = match abs_path.strip_prefix(&repo.root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };

            // Skip files in skip_dirs
            let parts: Vec<&str> = rel_path.split('/').collect();
            if parts.iter().any(|p| repo.config.skip_dirs.contains(*p)) {
                continue;
            }

            // Skip directories and non-existent paths (for remove events)
            if abs_path.is_dir() {
                continue;
            }

            if abs_path.exists() {
                // File created or modified
                match process_single_file(&repo.config, abs_path, &rel_path) {
                    Some(scanned) => {
                        // Update all_files
                        if let Some(pos) =
                            repo.all_files.iter().position(|f| f.rel_path == rel_path)
                        {
                            repo.all_files[pos] = scanned.clone();
                        } else {
                            repo.all_files.push(scanned.clone());
                        }

                        // Update manifest
                        update_manifest_entry(&mut repo.manifest, &scanned, &repo.config);

                        // Invalidate stub cache
                        repo.stub_cache.remove(&rel_path);

                        // Update import graph
                        update_import_edges_for_file(
                            &mut repo.import_graph,
                            &scanned,
                            &repo.all_files,
                        );

                        changed_count += 1;
                    }
                    None => {
                        // File doesn't match filters — treat as removal if it was indexed
                        remove_file_from_repo(repo, &rel_path);
                        removed_count += 1;
                    }
                }
            } else {
                // File deleted
                remove_file_from_repo(repo, &rel_path);
                removed_count += 1;
            }
        }

        if changed_count > 0 || removed_count > 0 {
            // Rebuild search index (fast — just bitmask computation)
            let (search_files, search_modules) = build_search_index(&repo.manifest);
            repo.search_files = search_files;
            repo.search_modules = search_modules;

            eprintln!(
                "  [watch] [{}] Updated {} file(s), removed {} file(s) ({} total indexed)",
                repo_name,
                changed_count,
                removed_count,
                repo.all_files.len()
            );
        }
    }
}

/// Remove a file from all repo indexes.
fn remove_file_from_repo(repo: &mut crate::types::RepoState, rel_path: &str) {
    repo.all_files.retain(|f| f.rel_path != rel_path);
    remove_manifest_entry(&mut repo.manifest, rel_path);
    repo.stub_cache.remove(rel_path);
    repo.import_graph.imports.remove(rel_path);
    for targets in repo.import_graph.imported_by.values_mut() {
        targets.retain(|t| t != rel_path);
    }
    repo.import_graph.imported_by.remove(rel_path);
}
