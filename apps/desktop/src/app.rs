//! Root application component — VS Code-like grid layout.

use dioxus::prelude::*;

use crate::search::SearchPanel;
use crate::sidebar::ActivityBar;
use crate::editor::EditorPanel;
use crate::state::*;

static VARIABLES_CSS: Asset = asset!("/assets/styles/variables.css");
static APP_CSS: Asset = asset!("/assets/styles/app.css");

#[component]
pub fn App() -> Element {
    rsx! {
        document::Stylesheet { href: VARIABLES_CSS }
        document::Stylesheet { href: APP_CSS }

        div {
            class: "app-shell",

            // Titlebar (drag region)
            div {
                class: "titlebar",
                // The window-controls-overlay is handled by the OS decorations
                span { class: "titlebar-title", "CodeScope" }
            }

            // Activity bar (left icon strip)
            ActivityBar {}

            // Main content area
            div {
                class: "content-area",

                // Search bar (spans full width of content area)
                SearchPanel {}

                // Split: sidebar + editor
                div {
                    class: "split-panel",

                    // Results / file tree sidebar
                    div {
                        class: "sidebar-panel",
                        ResultsSidebar {}
                    }

                    // Code preview / editor
                    EditorPanel {}
                }
            }

            // Status bar
            StatusBar {}
        }
    }
}

/// Results sidebar — shows search results or file tree depending on active panel
#[component]
fn ResultsSidebar() -> Element {
    let results = RESULTS.read();
    let active_idx = ACTIVE_IDX.read();

    if results.is_empty() {
        return rsx! {
            div {
                class: "sidebar-empty",
                span { "Type to search..." }
            }
        };
    }

    rsx! {
        div {
            class: "results-list",
            for (i, result) in results.iter().enumerate() {
                div {
                    class: if i == *active_idx { "result-item active" } else { "result-item" },
                    onclick: {
                        let path = result.path.clone();
                        move |_| {
                            *ACTIVE_IDX.write() = i;
                            *SELECTED_PATH.write() = Some(path.clone());
                        }
                    },
                    // Filename
                    span {
                        class: "result-filename",
                        {result.path.rsplit('/').next().unwrap_or(&result.path)}
                    }
                    // Score
                    span {
                        class: "result-score",
                        {format!("{:.0}", result.score)}
                    }
                    // Full path
                    div {
                        class: "result-path",
                        {&*result.path}
                    }
                }
            }
        }
    }
}

/// Status bar at the bottom of the app
#[component]
fn StatusBar() -> Element {
    let core = CORE.read();
    let results = RESULTS.read();
    let query_time = QUERY_TIME_MS.read();

    let (file_count, repo_name) = if let Some(ref state) = *core {
        let repo = state.default_repo();
        (repo.all_files.len(), repo.name.clone())
    } else {
        (0, "no project".to_string())
    };

    rsx! {
        div {
            class: "statusbar",
            span { class: "statusbar-repo", "{repo_name}" }
            span { class: "statusbar-sep", "|" }
            span { class: "statusbar-files", "{file_count} files" }
            if !results.is_empty() {
                span { class: "statusbar-sep", "|" }
                span { class: "statusbar-results", "{results.len()} results in {query_time:.1}ms" }
            }
        }
    }
}
