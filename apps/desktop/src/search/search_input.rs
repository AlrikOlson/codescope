//! Hero search input component with debounced search.

use dioxus::prelude::*;
use std::time::Instant;

use codescope_core::fuzzy::{preprocess_search_query, run_search};
use crate::state::*;

#[component]
pub fn SearchInput() -> Element {
    let mut debounce_gen = use_signal(|| 0u64);
    let query = QUERY.read();
    let has_query = !query.trim().is_empty();

    rsx! {
        div {
            class: "search-field",
            class: if has_query { "search-field has-query" } else { "search-field" },

            // Label
            span { class: "search-label", "SEARCH" }

            // Input row
            div {
                class: "search-input-row",

                // Search icon
                svg {
                    class: "search-icon",
                    width: "16",
                    height: "16",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                }

                // Input
                input {
                    class: "search-input",
                    r#type: "text",
                    placeholder: "Search files, modules...",
                    value: "{query}",
                    autofocus: true,
                    oninput: move |e: Event<FormData>| {
                        let value = e.value();
                        *QUERY.write() = value.clone();
                        *ACTIVE_IDX.write() = 0;
                        *EXT_FILTER.write() = None;

                        if value.trim().is_empty() {
                            *RESULTS.write() = vec![];
                            *MODULE_RESULTS.write() = vec![];
                            *SELECTED_PATH.write() = None;
                            return;
                        }

                        // Debounce: increment generation, spawn delayed search
                        let gen = *debounce_gen.read() + 1;
                        *debounce_gen.write() = gen;

                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                            if *debounce_gen.read() == gen {
                                run_search_query(&value);
                            }
                        });
                    },
                }

                // Clear button
                if has_query {
                    button {
                        class: "search-clear",
                        onclick: move |_| {
                            *QUERY.write() = String::new();
                            *RESULTS.write() = vec![];
                            *MODULE_RESULTS.write() = vec![];
                            *SELECTED_PATH.write() = None;
                            *ACTIVE_IDX.write() = 0;
                            *EXT_FILTER.write() = None;
                        },
                        "\u{00D7}"
                    }
                }
            }
        }
    }
}

/// Run the search query against the indexed repos and update global state.
fn run_search_query(query: &str) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        *RESULTS.write() = vec![];
        *MODULE_RESULTS.write() = vec![];
        *SELECTED_PATH.write() = None;
        return;
    }

    let core = CORE.read();
    let state = match core.as_ref() {
        Some(s) => s,
        None => return,
    };

    let repo = state.default_repo();
    let start = Instant::now();
    let processed = preprocess_search_query(trimmed);
    let response = run_search(
        &repo.search_files,
        &repo.search_modules,
        &processed,
        50,
        10,
    );
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    *QUERY_TIME_MS.write() = elapsed;
    *RESULTS.write() = response.files;
    *MODULE_RESULTS.write() = response.modules;

    // Auto-select first result
    let results = RESULTS.read();
    if let Some(first) = results.first() {
        *SELECTED_PATH.write() = Some(first.path.clone());
    } else {
        *SELECTED_PATH.write() = None;
    }
}
