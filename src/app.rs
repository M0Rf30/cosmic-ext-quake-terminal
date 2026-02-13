use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use clap::Parser;
use cosmic::app::{Core, Settings, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::event::{self, Event};
use cosmic::iced::window;
use cosmic::iced::Alignment;
use cosmic::iced::Length;
use cosmic::iced::Subscription;
use cosmic::widget::{self, container, header_bar, scrollable, settings, text, text_input};
use cosmic::{Application, ApplicationExt, Element};
use serde::{Deserialize, Serialize};

use crate::config::{QuakeConfig, CONFIG_VERSION};
use crate::fl;
use crate::process;
use crate::wayland::{self, ToplevelEvent, WaylandController};

/// (command, display_name, icon_name)
const KNOWN_TERMINALS: &[(&str, &str, &str)] = &[
    ("cosmic-term", "cosmic-terminal", "com.system76.CosmicTerm"),
    ("alacritty", "alacritty", "Alacritty"),
    ("kitty", "kitty", "kitty"),
    ("foot", "foot", "foot"),
    ("wezterm", "wezterm", "org.wezfurlong.wezterm"),
    ("ghostty", "ghostty", "com.mitchellh.ghostty"),
];

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
    /// Open the settings window
    Settings,
}

impl std::fmt::Display for QuakeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuakeAction::Toggle => write!(f, "Toggle"),
            QuakeAction::Settings => write!(f, "Settings"),
        }
    }
}

impl std::str::FromStr for QuakeAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Toggle" => Ok(QuakeAction::Toggle),
            "Settings" => Ok(QuakeAction::Settings),
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
    OpenSettings,
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    CloseWindow(window::Id),
    SetTerminalCommand(usize),
    SetTerminalArgs(String),
}

pub struct QuakeTerminal {
    core: Core,
    config: QuakeConfig,
    config_handler: Option<cosmic_config::Config>,
    state: ToggleState,
    terminal_pid: Option<Arc<AtomicU32>>,
    terminal_app_id: String,
    wayland_controller: Option<WaylandController>,
    settings_window_id: Option<window::Id>,
}

impl Application for QuakeTerminal {
    type Message = Message;
    type Executor = cosmic::executor::single::Executor;
    type Flags = Args;

    const APP_ID: &'static str = APP_ID;

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
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
            settings_window_id: None,
        };

        // Dispatch the initial action from CLI flags (first-instance case)
        let task = match flags.subcommand {
            Some(QuakeAction::Settings) => cosmic::task::message(Message::OpenSettings),
            Some(QuakeAction::Toggle) => cosmic::task::message(Message::Toggle),
            None => Task::none(),
        };

        (app, task)
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
                // Only reap the zombie process — do NOT reset state.
                // Many terminals fork (parent exits, child keeps running),
                // so PID death does not mean the window is gone.
                // State is driven by ToplevelEvent::Closed instead.
                tracing::info!("Terminal process exited (reaping zombie)");
                if let Some(pid) = self.terminal_pid.take() {
                    let raw = pid.load(Ordering::Relaxed) as i32;
                    let _ = nix::sys::wait::waitpid(
                        nix::unistd::Pid::from_raw(raw),
                        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                    );
                }
            }
            Message::ConfigChanged(config) => {
                tracing::info!("Config changed");
                self.terminal_app_id = process::get_app_id(&config.terminal_command);
                self.config = config;
            }
            Message::OpenSettings => {
                if self.settings_window_id.is_some() {
                    return Task::none();
                }
                let settings = window::Settings {
                    size: cosmic::iced::Size::new(500.0, 450.0),
                    resizable: true,
                    decorations: false,
                    ..window::Settings::default()
                };
                let (id, task) = window::open(settings);
                self.settings_window_id = Some(id);
                let title = fl!("settings-title");
                return task.discard().chain(self.set_window_title(title, id));
            }
            Message::WindowOpened(_id) => {}
            Message::CloseWindow(id) => {
                if self.settings_window_id == Some(id) {
                    self.settings_window_id = None;
                    return window::close(id);
                }
            }
            Message::WindowClosed(id) => {
                if self.settings_window_id == Some(id) {
                    self.settings_window_id = None;
                }
            }
            Message::SetTerminalCommand(index) => {
                if let Some(&(command, _, _)) = KNOWN_TERMINALS.get(index) {
                    if let Some(ref handler) = self.config_handler {
                        let _ = self.config.set_terminal_command(handler, command.into());
                    }
                }
            }
            Message::SetTerminalArgs(args_str) => {
                let args: Vec<String> = if args_str.trim().is_empty() {
                    Vec::new()
                } else {
                    args_str.split_whitespace().map(String::from).collect()
                };
                if let Some(ref handler) = self.config_handler {
                    let _ = self.config.set_terminal_args(handler, args);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Daemon mode - no main window
        text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        if self.settings_window_id != Some(id) {
            return text("").into();
        }

        let terminal_index = self.terminal_index();

        let mut terminal_section = settings::section().title(fl!("settings-terminal"));

        for (i, &(_, display_name, icon_name)) in KNOWN_TERMINALS.iter().enumerate() {
            let icon = widget::icon::from_name(icon_name).size(24).prefer_svg(true);
            let label = widget::row::with_children(vec![icon.into(), text(display_name).into()])
                .spacing(12)
                .align_y(Alignment::Center);

            terminal_section = terminal_section.add(widget::radio(
                label,
                i,
                Some(terminal_index),
                Message::SetTerminalCommand,
            ));
        }

        let terminal_section = terminal_section.add(settings::item(
            fl!("terminal-args"),
            text_input(
                fl!("terminal-args-placeholder"),
                self.config.terminal_args.join(" "),
            )
            .on_input(Message::SetTerminalArgs),
        ));

        let content = settings::view_column(vec![terminal_section.into()]).padding([0, 24]);

        let header = header_bar()
            .title(fl!("settings-title"))
            .on_close(Message::CloseWindow(id));

        container(widget::column().push(header).push(scrollable(content)))
            .class(cosmic::style::Container::Background)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
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

        // Watch for window events (settings window open/close)
        subs.push(event::listen_with(|event, _status, id| match event {
            Event::Window(window::Event::CloseRequested) => Some(Message::CloseWindow(id)),
            Event::Window(window::Event::Opened { .. }) => Some(Message::WindowOpened(id)),
            Event::Window(window::Event::Closed) => Some(Message::WindowClosed(id)),
            _ => None,
        }));

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
                        QuakeAction::Settings => {
                            return cosmic::task::message(Message::OpenSettings);
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
    fn terminal_index(&self) -> usize {
        KNOWN_TERMINALS
            .iter()
            .position(|&(cmd, _, _)| cmd == self.config.terminal_command)
            .unwrap_or(0)
    }

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
