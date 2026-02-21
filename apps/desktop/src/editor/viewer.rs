//! Code viewer component — displays file content with syntax highlighting.

use dioxus::prelude::*;
use crate::state::*;
use super::highlight;

const MAX_LINES: usize = 5000;

#[component]
pub fn CodeViewer(path: String) -> Element {
    // Read and highlight the file content.
    // We read SELECTED_PATH inside the memo so it re-runs when the selection changes.
    let highlighted = use_memo(move || {
        let selected = SELECTED_PATH.read();
        let path = match selected.as_ref() {
            Some(p) => p,
            None => return (vec![], false),
        };

        let core = CORE.read();
        let state = match core.as_ref() {
            Some(s) => s,
            None => return (vec!["// No project loaded".to_string()], false),
        };
        let repo = state.default_repo();
        let full_path = repo.root.join(path);
        let content = std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| format!("// Error reading file: {e}"));

        let ext = path.rsplit('.').next().unwrap_or("txt");
        let all_lines = highlight::highlight_code(&content, ext);
        let truncated = all_lines.len() > MAX_LINES;
        let lines = if truncated {
            all_lines[..MAX_LINES].to_vec()
        } else {
            all_lines
        };
        (lines, truncated)
    });

    let binding = highlighted.read();
    let (lines, truncated) = &*binding;

    rsx! {
        div {
            class: "code-viewer",
            // Inject syntax theme CSS
            style { {highlight::theme_css()} }

            pre {
                class: "code-content",
                for (i, line_html) in lines.iter().enumerate() {
                    div {
                        class: "code-line",
                        span { class: "line-number", "{i + 1}" }
                        span {
                            class: "line-text",
                            dangerous_inner_html: "{line_html}",
                        }
                    }
                }
                if *truncated {
                    div {
                        class: "code-truncated",
                        "File truncated at {MAX_LINES} lines"
                    }
                }
            }
        }
    }
}
