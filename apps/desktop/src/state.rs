//! Global application state using Dioxus signals.

use std::sync::Arc;

use codescope_core::fuzzy::{SearchFileResult, SearchModuleResult};
use codescope_core::types::RepoState;
use dioxus::prelude::*;

/// Which panel is active in the sidebar
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Search,
    FileTree,
}

/// Immutable snapshot of indexed repos — created once at startup, replaced on rescan.
pub struct AppState {
    pub repos: Vec<Arc<RepoState>>,
    pub default_repo_idx: usize,
}

impl AppState {
    /// Scan the current working directory and return an AppState.
    pub fn from_cwd() -> Self {
        let cwd = std::env::current_dir().expect("Could not determine current directory");
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        Self::from_path(&cwd)
    }

    /// Scan a specific path and return an AppState.
    pub fn from_path(root: &std::path::Path) -> Self {
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        let tok = codescope_core::tokenizer::create_tokenizer("bytes-estimate");
        let repo = codescope_core::scan_repo(&name, root, &tok);
        AppState {
            repos: vec![Arc::new(repo)],
            default_repo_idx: 0,
        }
    }

    pub fn default_repo(&self) -> &RepoState {
        &self.repos[self.default_repo_idx]
    }
}

// ---------------------------------------------------------------------------
// Global signals
// ---------------------------------------------------------------------------

/// Core indexed state — set once at startup
pub static CORE: GlobalSignal<Option<AppState>> = Signal::global(|| None);

/// Current search query
pub static QUERY: GlobalSignal<String> = Signal::global(|| String::new());

/// Search results (file matches)
pub static RESULTS: GlobalSignal<Vec<SearchFileResult>> = Signal::global(|| vec![]);

/// Search results (module matches)
pub static MODULE_RESULTS: GlobalSignal<Vec<SearchModuleResult>> = Signal::global(|| vec![]);

/// Index of the currently highlighted result
pub static ACTIVE_IDX: GlobalSignal<usize> = Signal::global(|| 0);

/// Path of the file currently shown in the preview
pub static SELECTED_PATH: GlobalSignal<Option<String>> = Signal::global(|| None);

/// Active sidebar panel
pub static ACTIVE_PANEL: GlobalSignal<Panel> = Signal::global(|| Panel::Search);

/// Extension filter (e.g. "rs", "ts")
pub static EXT_FILTER: GlobalSignal<Option<String>> = Signal::global(|| None);

/// Search timing in ms
pub static QUERY_TIME_MS: GlobalSignal<f64> = Signal::global(|| 0.0);
