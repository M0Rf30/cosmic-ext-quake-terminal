use cosmic_config::CosmicConfigEntry;
use serde::{Deserialize, Serialize};

pub const CONFIG_VERSION: u64 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Monitor {
    #[default]
    Focused,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Position {
    #[default]
    Top,
    Bottom,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    Deserialize,
    cosmic_config::cosmic_config_derive::CosmicConfigEntry,
)]
#[version = 1]
pub struct QuakeConfig {
    pub terminal_command: String,
    pub terminal_args: Vec<String>,
    pub height_percent: u32,
    pub width_percent: u32,
    pub monitor: Monitor,
    pub position: Position,
}

impl Default for QuakeConfig {
    fn default() -> Self {
        Self {
            terminal_command: String::from("cosmic-term"),
            terminal_args: Vec::new(),
            height_percent: 40,
            width_percent: 100,
            monitor: Monitor::default(),
            position: Position::default(),
        }
    }
}
