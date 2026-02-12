mod app;
mod config;
mod i18n;
mod process;
mod wayland;

use cosmic::Application;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() -> cosmic::iced::Result {
    init_logging();
    tracing::info!(
        "{} v{}",
        app::QuakeTerminal::APP_ID,
        env!("CARGO_PKG_VERSION")
    );
    i18n::localize();
    app::run()
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            EnvFilter::new("warn,cosmic_ext_quake_terminal=debug")
        } else {
            EnvFilter::new("warn,cosmic_ext_quake_terminal=info")
        }
    });

    let fmt_layer = fmt::layer().with_target(true);

    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    if let Ok(journald) = tracing_journald::layer() {
        registry.with(journald).init();
    } else {
        registry.init();
    }
}
