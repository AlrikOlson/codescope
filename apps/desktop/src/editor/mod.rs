//! Editor / code preview panel.

mod viewer;

use dioxus::prelude::*;
use crate::state::*;
use viewer::CodeViewer;

/// Editor panel — shows the selected file's content.
#[component]
pub fn EditorPanel() -> Element {
    let selected = SELECTED_PATH.read();

    match selected.as_ref() {
        Some(path) => {
            rsx! {
                div {
                    class: "editor-panel",

                    // Editor header (file path + info)
                    div {
                        class: "editor-header",
                        span { class: "editor-filepath", "{path}" }
                    }

                    // Code viewer
                    CodeViewer { path: path.clone() }
                }
            }
        }
        None => {
            rsx! {
                div {
                    class: "editor-panel editor-empty",
                    div {
                        class: "editor-empty-content",
                        svg {
                            width: "48",
                            height: "48",
                            view_box: "0 0 24 24",
                            fill: "none",
                            stroke: "currentColor",
                            stroke_width: "1",
                            opacity: "0.3",
                            path { d: "M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" }
                            polyline { points: "14 2 14 8 20 8" }
                        }
                        span { "Select a file to preview" }
                    }
                }
            }
        }
    }
}
