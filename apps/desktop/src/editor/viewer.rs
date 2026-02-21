//! Code viewer component — displays file content with line numbers.

use dioxus::prelude::*;
use crate::state::*;

#[component]
pub fn CodeViewer(path: String) -> Element {
    // Read the file from disk using the repo root
    let content = use_memo(move || {
        let core = CORE.read();
        let state = match core.as_ref() {
            Some(s) => s,
            None => return String::from("// No project loaded"),
        };
        let repo = state.default_repo();
        let full_path = repo.root.join(&path);
        std::fs::read_to_string(&full_path)
            .unwrap_or_else(|e| format!("// Error reading file: {e}"))
    });

    // Collect lines as owned Strings to avoid lifetime issues with the ReadableRef
    let binding = content.read();
    let lines: Vec<&str> = binding.lines().collect();

    rsx! {
        div {
            class: "code-viewer",
            pre {
                class: "code-content",
                for (i, line) in lines.iter().enumerate() {
                    div {
                        class: "code-line",
                        span { class: "line-number", "{i + 1}" }
                        span { class: "line-text", "{line}" }
                    }
                }
            }
        }
    }
}
