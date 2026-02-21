//! Search panel — input field + metadata strip + filter chips.

mod search_input;
mod metadata_strip;
mod filters;

use dioxus::prelude::*;
use search_input::SearchInput;
use metadata_strip::MetadataStrip;

/// Search panel spanning the full width of the content area.
#[component]
pub fn SearchPanel() -> Element {
    rsx! {
        div {
            class: "search-panel",
            SearchInput {}
            MetadataStrip {}
        }
    }
}
