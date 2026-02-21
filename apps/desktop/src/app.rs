//! Root application component — VS Code-like grid layout.

use dioxus::prelude::*;

use crate::search::SearchPanel;
use crate::sidebar::{ActivityBar, FileTree};
use crate::editor::EditorPanel;
use crate::state::*;

static VARIABLES_CSS: Asset = asset!("/assets/styles/variables.css");
static APP_CSS: Asset = asset!("/assets/styles/app.css");

#[component]
pub fn App() -> Element {
    // Transfer pre-scanned state into the GlobalSignal on first render
    use_hook(|| {
        if let Some(state) = crate::INITIAL_STATE.lock().unwrap().take() {
            *CORE.write() = Some(state);
        }
    });

    let mut is_dragging = use_signal(|| false);
    let sidebar_w = *SIDEBAR_WIDTH.read();

    rsx! {
        document::Stylesheet { href: VARIABLES_CSS }
        document::Stylesheet { href: APP_CSS }

        // Full-screen overlay to capture mouse during resize
        if *is_dragging.read() {
            div {
                class: "drag-overlay",
                onmousemove: move |e: Event<MouseData>| {
                    let x = e.client_coordinates().x;
                    let new_width = (x - 48.0).clamp(200.0, 600.0);
                    *SIDEBAR_WIDTH.write() = new_width;
                },
                onmouseup: move |_| {
                    *is_dragging.write() = false;
                },
            }
        }

        div {
            class: "app-shell",
            tabindex: "0",
            onkeydown: handle_keydown,

            // Titlebar (drag region)
            div {
                class: "titlebar",
                span { class: "titlebar-title", "CodeScope" }
            }

            // Activity bar (left icon strip)
            ActivityBar {}

            // Main content area
            div {
                class: "content-area",

                // Search bar (spans full width of content area)
                SearchPanel {}

                // Split: sidebar + drag handle + editor
                div {
                    class: "split-panel",
                    style: "grid-template-columns: {sidebar_w}px 4px 1fr;",

                    // Results / file tree sidebar
                    div {
                        class: "sidebar-panel",
                        match *ACTIVE_PANEL.read() {
                            Panel::Search => rsx! { ResultsSidebar {} },
                            Panel::FileTree => rsx! { FileTree {} },
                        }
                    }

                    // Drag handle
                    div {
                        class: "resize-handle",
                        onmousedown: move |e: Event<MouseData>| {
                            e.prevent_default();
                            *is_dragging.write() = true;
                        },
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

/// Global keyboard handler for app-level shortcuts.
fn handle_keydown(e: Event<KeyboardData>) {
    let key = e.key();
    let modifiers = e.modifiers();

    match key {
        Key::ArrowDown => {
            e.prevent_default();
            let results = RESULTS.read();
            let current = *ACTIVE_IDX.read();
            if !results.is_empty() && current < results.len() - 1 {
                let new_idx = current + 1;
                *ACTIVE_IDX.write() = new_idx;
                *SELECTED_PATH.write() = Some(results[new_idx].path.clone());
            }
        }
        Key::ArrowUp => {
            e.prevent_default();
            let results = RESULTS.read();
            let current = *ACTIVE_IDX.read();
            if !results.is_empty() && current > 0 {
                let new_idx = current - 1;
                *ACTIVE_IDX.write() = new_idx;
                *SELECTED_PATH.write() = Some(results[new_idx].path.clone());
            }
        }
        Key::Enter => {
            let results = RESULTS.read();
            let idx = *ACTIVE_IDX.read();
            if let Some(result) = results.get(idx) {
                *SELECTED_PATH.write() = Some(result.path.clone());
            }
        }
        Key::Escape => {
            *QUERY.write() = String::new();
            *RESULTS.write() = vec![];
            *MODULE_RESULTS.write() = vec![];
            *SELECTED_PATH.write() = None;
            *ACTIVE_IDX.write() = 0;
        }
        Key::Character(ref c) if c == "k" && (modifiers.contains(Modifiers::CONTROL) || modifiers.contains(Modifiers::META)) => {
            e.prevent_default();
            document::eval("document.querySelector('.search-input')?.focus()");
        }
        _ => {}
    }
}

/// Build HTML with matched characters wrapped in `<mark>` tags.
fn highlight_html(text: &str, indices: &[usize]) -> String {
    let index_set: std::collections::HashSet<usize> = indices.iter().copied().collect();
    let mut html = String::with_capacity(text.len() * 2);
    let mut in_mark = false;

    for (i, ch) in text.chars().enumerate() {
        let should_mark = index_set.contains(&i);
        if should_mark && !in_mark {
            html.push_str("<mark>");
            in_mark = true;
        } else if !should_mark && in_mark {
            html.push_str("</mark>");
            in_mark = false;
        }
        match ch {
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '&' => html.push_str("&amp;"),
            '"' => html.push_str("&quot;"),
            _ => html.push(ch),
        }
    }
    if in_mark {
        html.push_str("</mark>");
    }
    html
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
                    // Filename with match highlights
                    span {
                        class: "result-filename",
                        dangerous_inner_html: "{highlight_html(&result.filename, &result.filename_indices)}",
                    }
                    // Score
                    span {
                        class: "result-score",
                        {format!("{:.0}", result.score)}
                    }
                    // Full path with match highlights
                    div {
                        class: "result-path",
                        dangerous_inner_html: "{highlight_html(&result.path, &result.path_indices)}",
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
