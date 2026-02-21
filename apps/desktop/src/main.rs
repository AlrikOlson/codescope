//! CodeScope Desktop — Dioxus-powered codebase explorer.

use std::sync::Mutex;

use dioxus::prelude::*;

mod app;
mod state;
mod search;
mod sidebar;
mod editor;
mod components;

use app::App;
use state::AppState;

/// Pre-runtime storage — scanned before Dioxus launches, consumed on first render.
pub static INITIAL_STATE: Mutex<Option<AppState>> = Mutex::new(None);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codescope=info".parse().unwrap()),
        )
        .with_target(false)
        .init();

    // Scan repos at startup (blocking) — store in Mutex, NOT in the signal
    let initial_state = AppState::from_cwd();
    *INITIAL_STATE.lock().unwrap() = Some(initial_state);

    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, WindowBuilder, LogicalSize};

        LaunchBuilder::new()
            .with_cfg(
                Config::default()
                    .with_menu(None)
                    .with_background_color((10, 10, 10, 255))
                    .with_disable_context_menu(true)
                    .with_window(
                        WindowBuilder::new()
                            .with_title("CodeScope")
                            .with_inner_size(LogicalSize::new(1400.0, 900.0))
                            .with_min_inner_size(LogicalSize::new(800.0, 500.0))
                            .with_resizable(true)
                            .with_decorations(true),
                    ),
            )
            .launch(App);
    }

    #[cfg(not(feature = "desktop"))]
    {
        dioxus::launch(App);
    }
}
