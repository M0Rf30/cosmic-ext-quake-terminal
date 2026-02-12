use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use clap::Parser;
use cosmic::app::{Core, Settings, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Subscription;
use cosmic::widget::text;
use cosmic::{Application, Element};
use serde::{Deserialize, Serialize};

use crate::config::{QuakeConfig, CONFIG_VERSION};
use crate::process;
use crate::wayland::{self, ToplevelEvent, WaylandController};

const APP_ID: &str = "com.github.m0rf30.CosmicExtQuakeTerminal";

#[derive(Parser, Debug, Serialize, Deserialize, Clone)]
#[command(name = "cosmic-ext-quake-terminal")]
#[command(about = "Quake-style dropdown terminal for COSMIC Desktop")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: Option<QuakeAction>,
}

#[derive(Debug, Serialize, Deserialize, Clone, clap::Subcommand)]
pub enum QuakeAction {
    /// Toggle the quake terminal visibility
    Toggle,
}

impl std::fmt::Display for QuakeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuakeAction::Toggle => write!(f, "Toggle"),
        }
    }
}

impl std::str::FromStr for QuakeAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Toggle" => Ok(QuakeAction::Toggle),
            other => Err(format!("Unknown action: {other}")),
        }
    }
}

impl cosmic::app::CosmicFlags for Args {
    type SubCommand = QuakeAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&QuakeAction> {
        self.subcommand.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ToggleState {
    Idle,
    WaitingForWindow,
    Visible,
    Hidden,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle,
    ToplevelEvent(ToplevelEvent),
    TerminalExited,
    ConfigChanged(QuakeConfig),
}

pub struct QuakeTerminal {
    core: Core,
    config: QuakeConfig,
    config_handler: Option<cosmic_config::Config>,
    state: ToggleState,
    terminal_pid: Option<Arc<AtomicU32>>,
    terminal_app_id: String,
    wayland_controller: Option<WaylandController>,
}

impl Application for QuakeTerminal {
    type Message = Message;
    type Executor = cosmic::executor::single::Executor;
    type Flags = Args;

    const APP_ID: &'static str = APP_ID;

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let config_handler = cosmic_config::Config::new(APP_ID, CONFIG_VERSION).ok();
        let config = config_handler
            .as_ref()
            .and_then(|h| QuakeConfig::get_entry(h).ok())
            .unwrap_or_default();

        // Pre-compute the app_id for the configured terminal
        let terminal_app_id = process::get_app_id(&config.terminal_command);

        let app = Self {
            core,
            config,
            config_handler,
            state: ToggleState::Idle,
            terminal_pid: None,
            terminal_app_id,
            wayland_controller: None,
        };

        (app, Task::none())
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Toggle => self.handle_toggle(),
            Message::ToplevelEvent(event) => self.handle_toplevel_event(event),
            Message::TerminalExited => {
                tracing::info!("Terminal process exited");
                // Reap the zombie process
                if let Some(pid) = self.terminal_pid.take() {
                    let raw = pid.load(Ordering::Relaxed) as i32;
                    let _ = nix::sys::wait::waitpid(
                        nix::unistd::Pid::from_raw(raw),
                        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                    );
                }
                self.state = ToggleState::Idle;
            }
            Message::ConfigChanged(config) => {
                tracing::info!("Config changed");
                self.terminal_app_id = process::get_app_id(&config.terminal_command);
                self.config = config;
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Daemon mode - no main window
        text("").into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs = vec![wayland::toplevel_subscription(self.terminal_app_id.clone())
            .map(Message::ToplevelEvent)];

        // Monitor terminal process exit via kill(pid, 0)
        if let Some(ref pid_holder) = self.terminal_pid {
            let pid_holder = pid_holder.clone();
            subs.push(cosmic::iced::Subscription::run_with_id(
                "process-monitor",
                futures::stream::unfold(pid_holder, |pid_holder| async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        let pid = pid_holder.load(Ordering::Relaxed);
                        if pid != 0 {
                            let alive = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(pid as i32),
                                None,
                            )
                            .is_ok();
                            if !alive {
                                return Some((Message::TerminalExited, pid_holder));
                            }
                        }
                    }
                }),
            ));
        }

        // Watch for config changes
        if self.config_handler.is_some() {
            subs.push(
                cosmic_config::config_subscription::<_, QuakeConfig>(
                    std::any::TypeId::of::<QuakeConfig>(),
                    APP_ID.into(),
                    CONFIG_VERSION,
                )
                .map(|update| {
                    if !update.errors.is_empty() {
                        tracing::warn!("Config errors: {:?}", update.errors);
                    }
                    Message::ConfigChanged(update.config)
                }),
            );
        }

        Subscription::batch(subs)
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;

        match msg.msg {
            Details::Activate => {
                return cosmic::task::message(Message::Toggle);
            }
            Details::ActivateAction { action, .. } => {
                if let Ok(cmd) = action.parse::<QuakeAction>() {
                    match cmd {
                        QuakeAction::Toggle => {
                            return cosmic::task::message(Message::Toggle);
                        }
                    }
                }
            }
            Details::Open { .. } => {}
        }
        Task::none()
    }
}

impl QuakeTerminal {
    fn handle_toggle(&mut self) {
        match self.state {
            ToggleState::Idle => {
                tracing::info!("Toggle: spawning terminal");
                let result = process::spawn_terminal(
                    &self.config.terminal_command,
                    &self.config.terminal_args,
                );
                if let Some(result) = result {
                    let pid = result.pid;
                    self.terminal_pid = Some(Arc::new(AtomicU32::new(pid)));
                    self.terminal_app_id = result.app_id;
                    self.state = ToggleState::WaitingForWindow;
                }
            }
            ToggleState::WaitingForWindow => {
                tracing::debug!("Toggle: still waiting for window to appear");
            }
            ToggleState::Visible => {
                tracing::info!("Toggle: hiding terminal");
                if let Some(ref controller) = self.wayland_controller {
                    controller.minimize();
                }
                self.state = ToggleState::Hidden;
            }
            ToggleState::Hidden => {
                tracing::info!("Toggle: showing terminal");
                if let Some(ref controller) = self.wayland_controller {
                    controller.activate();
                }
                self.state = ToggleState::Visible;
            }
        }
    }

    fn handle_toplevel_event(&mut self, event: ToplevelEvent) {
        match event {
            ToplevelEvent::Ready(controller) => {
                tracing::info!("Wayland toplevel controller ready");
                self.wayland_controller = Some(controller);
            }
            ToplevelEvent::Found => {
                tracing::info!("Terminal window found");
                if self.state == ToggleState::WaitingForWindow {
                    self.state = ToggleState::Visible;
                }
            }
            ToplevelEvent::Minimized => {
                if self.terminal_pid.is_some() {
                    self.state = ToggleState::Hidden;
                }
            }
            ToplevelEvent::Activated => {
                if self.terminal_pid.is_some() {
                    self.state = ToggleState::Visible;
                }
            }
            ToplevelEvent::Closed => {
                tracing::info!("Terminal window closed by compositor");
                self.state = ToggleState::Idle;
                if let Some(pid) = self.terminal_pid.take() {
                    let raw = pid.load(Ordering::Relaxed) as i32;
                    let nix_pid = nix::unistd::Pid::from_raw(raw);
                    let _ = nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGTERM);
                    let _ = nix::sys::wait::waitpid(
                        nix_pid,
                        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                    );
                }
            }
        }
    }
}

pub fn run() -> cosmic::iced::Result {
    let args = Args::parse();

    cosmic::app::run_single_instance::<QuakeTerminal>(
        Settings::default()
            .no_main_window(true)
            .exit_on_close(false),
        args,
    )
}
