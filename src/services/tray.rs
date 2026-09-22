use std::time::Duration;

use system_tray::client::{ActivateRequest, Client};
use system_tray::item::{IconPixmap, Status};
use system_tray::menu::TrayMenu;
use tokio::sync::{mpsc, watch};
use tracing::warn;

use crate::runtime;

#[derive(Debug, Clone, Default)]
pub struct TraySnapshot {
    pub items: Vec<TrayItem>,
}

#[derive(Debug, Clone)]
pub struct TrayItem {
    pub address: String,
    pub title: String,
    pub status: Status,
    pub icon_name: Option<String>,
    pub icon_pixmap: Option<Vec<IconPixmap>>,
    pub menu_path: Option<String>,
    pub menu: Option<TrayMenu>,
}

#[derive(Debug, Clone)]
pub enum TrayCommand {
    Activate(String),
    Secondary(String),
    MenuItem {
        address: String,
        path: String,
        id: i32,
    },
}

#[derive(Debug, Clone)]
pub struct TrayService {
    state: watch::Receiver<TraySnapshot>,
    commands: mpsc::Sender<TrayCommand>,
}

impl TrayService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(TraySnapshot::default());
        let (commands, command_rx) = mpsc::channel(32);
        runtime::spawn(run(state_tx, command_rx));
        Self { state, commands }
    }

    pub fn subscribe(&self) -> watch::Receiver<TraySnapshot> {
        self.state.clone()
    }

    pub fn send(&self, command: TrayCommand) {
        if self.commands.try_send(command).is_err() {
            warn!("tray command queue is full or closed");
        }
    }
}

async fn run(state: watch::Sender<TraySnapshot>, mut commands: mpsc::Receiver<TrayCommand>) {
    loop {
        let client = match Client::new().await {
            Ok(client) => client,
            Err(error) => {
                warn!(%error, "failed to connect StatusNotifier tray");
                state.send_replace(TraySnapshot::default());
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        let mut events = client.subscribe();
        publish(&state, &client);
        loop {
            tokio::select! {
                event = events.recv() => {
                    if event.is_err() { break; }
                    publish(&state, &client);
                }
                command = commands.recv() => {
                    let Some(command) = command else { return };
                    let request = match command {
                        TrayCommand::Activate(address) => ActivateRequest::Default { address, x: 0, y: 0 },
                        TrayCommand::Secondary(address) => ActivateRequest::Secondary { address, x: 0, y: 0 },
                        TrayCommand::MenuItem { address, path, id } => ActivateRequest::MenuItem {
                            address, menu_path: path, submenu_id: id,
                        },
                    };
                    if let Err(error) = client.activate(request).await {
                        warn!(%error, "tray activation failed");
                    }
                }
            }
        }
        state.send_replace(TraySnapshot::default());
    }
}

fn publish(state: &watch::Sender<TraySnapshot>, client: &Client) {
    let items = client.items();
    let Ok(items) = items.lock() else { return };
    let mut items = items
        .iter()
        .map(|(address, (item, menu))| TrayItem {
            address: address.clone(),
            title: item.title.clone().unwrap_or_else(|| item.id.clone()),
            status: item.status,
            icon_name: item.icon_name.clone(),
            icon_pixmap: item.icon_pixmap.clone(),
            menu_path: item.menu.clone(),
            menu: menu.clone(),
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.title.to_ascii_lowercase());
    state.send_replace(TraySnapshot { items });
}
