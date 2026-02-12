use std::process::Command;
use tracing::{error, info};

pub const QUAKE_APP_ID: &str = "cosmic-ext-quake-terminal";

pub struct SpawnResult {
    pub pid: u32,
    pub app_id: String,
}

pub fn spawn_terminal(command: &str, args: &[String]) -> Option<SpawnResult> {
    let (class_args, app_id) = get_class_args(command);

    let mut cmd = Command::new(command);
    cmd.args(&class_args);
    cmd.args(args);

    info!(
        "Spawning terminal: {} {:?} {:?} (tracking app_id={})",
        command, class_args, args, app_id
    );

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            // Intentionally drop the Child handle — the terminal process is
            // independent and will be reaped via waitpid when it exits.
            drop(child);
            Some(SpawnResult { pid, app_id })
        }
        Err(e) => {
            error!("Failed to spawn terminal '{}': {}", command, e);
            None
        }
    }
}

/// Returns the Wayland app_id that the given terminal will use.
pub fn get_app_id(command: &str) -> String {
    get_class_args(command).1
}

fn get_class_args(command: &str) -> (Vec<String>, String) {
    let binary = command.rsplit('/').next().unwrap_or(command);

    match binary {
        // ghostty on GTK ignores --class; it always uses its default app_id.
        // Use --gtk-single-instance=false to avoid joining an existing instance.
        "ghostty" => (
            vec!["--gtk-single-instance=false".into()],
            "com.mitchellh.ghostty".to_string(),
        ),
        // foot uses --app-id
        "foot" => (
            vec![format!("--app-id={QUAKE_APP_ID}")],
            QUAKE_APP_ID.to_string(),
        ),
        // Most terminals support --class
        "cosmic-term" | "alacritty" | "kitty" | "wezterm" => (
            vec!["--class".into(), QUAKE_APP_ID.into()],
            QUAKE_APP_ID.to_string(),
        ),
        // Default: try --class and hope it works
        _ => (
            vec!["--class".into(), QUAKE_APP_ID.into()],
            QUAKE_APP_ID.to_string(),
        ),
    }
}
