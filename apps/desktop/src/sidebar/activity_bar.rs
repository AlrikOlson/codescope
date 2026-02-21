//! Activity bar — vertical icon strip on the left edge.

use dioxus::prelude::*;
use crate::state::*;

#[component]
pub fn ActivityBar() -> Element {
    let active_panel = ACTIVE_PANEL.read();

    rsx! {
        nav {
            class: "activity-bar",

            // Search icon
            button {
                class: if *active_panel == Panel::Search { "activity-btn active" } else { "activity-btn" },
                title: "Search",
                onclick: move |_| { *ACTIVE_PANEL.write() = Panel::Search; },
                svg {
                    width: "22",
                    height: "22",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    circle { cx: "11", cy: "11", r: "8" }
                    line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                }
            }

            // File tree icon
            button {
                class: if *active_panel == Panel::FileTree { "activity-btn active" } else { "activity-btn" },
                title: "Explorer",
                onclick: move |_| { *ACTIVE_PANEL.write() = Panel::FileTree; },
                svg {
                    width: "22",
                    height: "22",
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    path { d: "M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z" }
                }
            }
        }
    }
}
