//! Metadata strip showing result count, query time, and extension filters.

use dioxus::prelude::*;
use crate::state::*;

#[component]
pub fn MetadataStrip() -> Element {
    let results = RESULTS.read();
    let query_time = QUERY_TIME_MS.read();
    let query = QUERY.read();

    if query.trim().is_empty() {
        return rsx! {
            div { class: "metadata-strip hidden" }
        };
    }

    rsx! {
        div {
            class: "metadata-strip",
            span { class: "metadata-count", "{results.len()} results" }
            span { class: "metadata-sep", "\u{00B7}" }
            span { class: "metadata-time", "{query_time:.1}ms" }
        }
    }
}
