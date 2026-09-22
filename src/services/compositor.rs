use std::env;

use tokio::sync::{mpsc, watch};
use tracing::warn;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub connected: bool,
    pub supports_persistent: bool,
    pub workspaces: Vec<Workspace>,
    pub active_title: String,
    pub active_class: String,
    pub submap: String,
    pub keyboard_layout: String,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Window {
    pub address: String,
    pub title: String,
    pub class: String,
    pub workspace_id: i64,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub reference: String,
    pub monitor: String,
    pub windows: u32,
    pub active: bool,
    pub visible: bool,
    pub urgent: bool,
    pub special: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Focus {
        id: i64,
        workspace: String,
        current_monitor: bool,
    },
    Relative(i8),
    FocusWindow(String),
}

#[derive(Debug, Clone)]
pub struct CompositorService {
    state: watch::Receiver<WorkspaceSnapshot>,
    commands: mpsc::Sender<Command>,
}

impl CompositorService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(WorkspaceSnapshot::default());
        let (commands, command_rx) = mpsc::channel(16);

        match backend() {
            Backend::Niri => {
                crate::runtime::spawn(crate::services::niri::run_events(state_tx));
                crate::runtime::spawn(crate::services::niri::run_commands(command_rx));
            }
            Backend::Sway => {
                crate::runtime::spawn(crate::services::sway::run_events(state_tx));
                crate::runtime::spawn(crate::services::sway::run_commands(command_rx));
            }
            Backend::Kde => {
                crate::runtime::spawn(crate::services::kde::run(state_tx, command_rx));
            }
            Backend::Hyprland => {
                crate::runtime::spawn(crate::services::hyprland::run_events(state_tx));
                crate::runtime::spawn(crate::services::hyprland::run_commands(command_rx));
            }
        }

        Self { state, commands }
    }

    pub fn subscribe(&self) -> watch::Receiver<WorkspaceSnapshot> {
        self.state.clone()
    }

    pub fn send(&self, command: Command) {
        if self.commands.try_send(command).is_err() {
            warn!("compositor command queue is full or closed");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Hyprland,
    Niri,
    Sway,
    Kde,
}

fn backend() -> Backend {
    if let Ok(value) = env::var("BEARBAR_COMPOSITOR") {
        match value.to_ascii_lowercase().as_str() {
            "hyprland" => return Backend::Hyprland,
            "niri" => return Backend::Niri,
            "sway" => return Backend::Sway,
            "kde" | "plasma" => return Backend::Kde,
            _ => warn!(
                value,
                "unknown BEARBAR_COMPOSITOR value; using auto-detection"
            ),
        }
    }
    if env::var_os("NIRI_SOCKET").is_some() {
        Backend::Niri
    } else if env::var_os("SWAYSOCK").is_some() || env::var_os("I3SOCK").is_some() {
        Backend::Sway
    } else if env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.to_ascii_lowercase().contains("kde"))
        || env::var_os("KDE_FULL_SESSION").is_some()
    {
        Backend::Kde
    } else {
        Backend::Hyprland
    }
}
