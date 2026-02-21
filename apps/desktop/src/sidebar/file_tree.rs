//! File tree component (placeholder for future implementation).

use dioxus::prelude::*;

#[component]
pub fn FileTree() -> Element {
    rsx! {
        div {
            class: "file-tree",
            div {
                class: "file-tree-placeholder",
                "File tree coming soon"
            }
        }
    }
}
