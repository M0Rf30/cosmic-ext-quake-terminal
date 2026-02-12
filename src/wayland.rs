use std::os::unix::io::AsFd;
use std::sync::mpsc as std_mpsc;

use cosmic_client_toolkit::toplevel_info::{ToplevelInfoHandler, ToplevelInfoState};
use cosmic_client_toolkit::toplevel_management::{ToplevelManagerHandler, ToplevelManagerState};
use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1::{
    self, ZcosmicToplevelHandleV1,
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use tokio::sync::mpsc as tokio_mpsc;
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, QueueHandle, WEnum};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1;

#[derive(Debug, Clone)]
pub enum ToplevelEvent {
    Ready(WaylandController),
    Found,
    Minimized,
    Activated,
    Closed,
}

#[derive(Debug, Clone)]
pub enum WaylandCommand {
    Minimize,
    Activate,
}

#[derive(Debug, Clone)]
pub struct WaylandController {
    cmd_tx: std_mpsc::Sender<WaylandCommand>,
}

impl WaylandController {
    pub fn minimize(&self) {
        let _ = self.cmd_tx.send(WaylandCommand::Minimize);
    }

    pub fn activate(&self) {
        let _ = self.cmd_tx.send(WaylandCommand::Activate);
    }
}

struct WaylandState {
    registry: RegistryState,
    toplevel_info: ToplevelInfoState,
    toplevel_manager: Option<ToplevelManagerState>,
    seat_state: SeatState,
    seat: Option<WlSeat>,
    target_app_id: String,
    our_handle: Option<ZcosmicToplevelHandleV1>,
    our_foreign_handle: Option<ExtForeignToplevelHandleV1>,
    event_tx: tokio_mpsc::UnboundedSender<ToplevelEvent>,
    last_minimized: Option<bool>,
}

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    fn runtime_add_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
        _version: u32,
    ) {
    }

    fn runtime_remove_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
    ) {
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, seat: WlSeat) {
        if self.seat.is_none() {
            self.seat = Some(seat);
        }
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: WlSeat,
        _capability: Capability,
    ) {
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}
}

impl ToplevelInfoHandler for WaylandState {
    fn toplevel_info_state(&mut self) -> &mut ToplevelInfoState {
        &mut self.toplevel_info
    }

    fn new_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        if let Some(info) = self.toplevel_info.info(toplevel) {
            if info.app_id == self.target_app_id {
                tracing::info!("Found our toplevel: app_id={}", info.app_id);
                self.our_handle = info.cosmic_toplevel.clone();
                self.our_foreign_handle = Some(toplevel.clone());
                self.last_minimized = None;
                let _ = self.event_tx.send(ToplevelEvent::Found);
            }
        }
    }

    fn update_toplevel(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        // Only track updates for the specific window we're managing
        let is_our_window = self
            .our_foreign_handle
            .as_ref()
            .is_some_and(|h| h == toplevel);
        if !is_our_window {
            return;
        }

        if let Some(info) = self.toplevel_info.info(toplevel) {
            self.our_handle = info.cosmic_toplevel.clone();

            let is_minimized = info
                .state
                .contains(&zcosmic_toplevel_handle_v1::State::Minimized);

            // Only emit events on actual state changes
            if self.last_minimized != Some(is_minimized) {
                self.last_minimized = Some(is_minimized);
                if is_minimized {
                    let _ = self.event_tx.send(ToplevelEvent::Minimized);
                } else if info
                    .state
                    .contains(&zcosmic_toplevel_handle_v1::State::Activated)
                {
                    let _ = self.event_tx.send(ToplevelEvent::Activated);
                }
            }
        }
    }

    fn toplevel_closed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        toplevel: &ExtForeignToplevelHandleV1,
    ) {
        let is_our_window = self
            .our_foreign_handle
            .as_ref()
            .is_some_and(|h| h == toplevel);
        if is_our_window {
            tracing::info!("Our toplevel closed");
            self.our_handle = None;
            self.our_foreign_handle = None;
            self.last_minimized = None;
            let _ = self.event_tx.send(ToplevelEvent::Closed);
        }
    }
}

impl ToplevelManagerHandler for WaylandState {
    fn toplevel_manager_state(&mut self) -> &mut ToplevelManagerState {
        // Safe: this handler is only dispatched when the protocol is bound,
        // which only happens when try_new() returned Some.
        self.toplevel_manager
            .as_mut()
            .expect("toplevel_manager_state called but manager not available")
    }

    fn capabilities(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _capabilities: Vec<
            WEnum<zcosmic_toplevel_manager_v1::ZcosmicToplelevelManagementCapabilitiesV1>,
        >,
    ) {
    }
}

smithay_client_toolkit::delegate_registry!(WaylandState);
smithay_client_toolkit::delegate_seat!(WaylandState);
cosmic_client_toolkit::delegate_toplevel_info!(WaylandState);
cosmic_client_toolkit::delegate_toplevel_manager!(WaylandState);

fn run_wayland_loop(
    target_app_id: String,
    event_tx: tokio_mpsc::UnboundedSender<ToplevelEvent>,
    cmd_rx: std_mpsc::Receiver<WaylandCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let registry = RegistryState::new(&globals);
    let seat_state = SeatState::new(&globals, &qh);
    let toplevel_info = ToplevelInfoState::new(&registry, &qh);
    let toplevel_manager = ToplevelManagerState::try_new(&registry, &qh);

    if toplevel_manager.is_none() {
        tracing::warn!("Toplevel manager not available - minimize/activate won't work");
    }

    let mut state = WaylandState {
        registry,
        toplevel_info,
        toplevel_manager,
        seat_state,
        seat: None,
        target_app_id,
        our_handle: None,
        our_foreign_handle: None,
        event_tx,
        last_minimized: None,
    };

    // Initial roundtrip to discover globals and existing toplevels
    event_queue.roundtrip(&mut state)?;

    loop {
        // Process commands from the app
        while let Ok(cmd) = cmd_rx.try_recv() {
            handle_command_inner(&state, cmd);
            let _ = conn.flush();
        }

        // Dispatch pending wayland events
        event_queue.dispatch_pending(&mut state)?;
        conn.flush()?;

        // Poll for new wayland events with a timeout
        if let Some(guard) = event_queue.prepare_read() {
            let fd = guard.connection_fd();
            let poll_fd = PollFd::new(fd.as_fd(), PollFlags::POLLIN);
            match poll(&mut [poll_fd], PollTimeout::from(100u16)) {
                Ok(_) => {
                    let _ = guard.read();
                }
                Err(_) => {
                    drop(guard);
                }
            }
        }
    }
}

fn handle_command_inner(state: &WaylandState, cmd: WaylandCommand) {
    let Some(ref handle) = state.our_handle else {
        tracing::warn!("No toplevel handle, cannot execute command: {cmd:?}");
        return;
    };
    let Some(ref manager_state) = state.toplevel_manager else {
        tracing::warn!("No toplevel manager available");
        return;
    };

    let manager = &manager_state.manager;

    match cmd {
        WaylandCommand::Minimize => {
            manager.set_minimized(handle);
        }
        WaylandCommand::Activate => {
            manager.unset_minimized(handle);
            if let Some(ref seat) = state.seat {
                manager.activate(handle, seat);
            }
        }
    }
}

pub fn toplevel_subscription(target_app_id: String) -> cosmic::iced::Subscription<ToplevelEvent> {
    struct ToplevelSub;

    cosmic::iced::Subscription::run_with_id(
        std::any::TypeId::of::<ToplevelSub>(),
        futures::stream::unfold(ToplevelSubState::Init(target_app_id), |state| async move {
            match state {
                ToplevelSubState::Init(target_app_id) => {
                    let (event_tx, event_rx) = tokio_mpsc::unbounded_channel();
                    let (cmd_tx, cmd_rx) = std_mpsc::channel();

                    let controller = WaylandController { cmd_tx };

                    std::thread::spawn(move || {
                        if let Err(e) = run_wayland_loop(target_app_id, event_tx, cmd_rx) {
                            tracing::error!("Wayland toplevel loop error: {e}");
                        }
                    });

                    Some((
                        ToplevelEvent::Ready(controller),
                        ToplevelSubState::Running(event_rx),
                    ))
                }
                ToplevelSubState::Running(mut rx) => {
                    let event = rx.recv().await?;
                    Some((event, ToplevelSubState::Running(rx)))
                }
            }
        }),
    )
}

enum ToplevelSubState {
    Init(String),
    Running(tokio_mpsc::UnboundedReceiver<ToplevelEvent>),
}
