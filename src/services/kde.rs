use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::services::compositor::{Command, Workspace, WorkspaceSnapshot};

type DesktopData = (i32, String, String);

pub async fn run(
    state_tx: watch::Sender<WorkspaceSnapshot>,
    mut commands: mpsc::Receiver<Command>,
) {
    let mut retry = Duration::from_millis(250);
    loop {
        match listen(&state_tx, &mut commands).await {
            Ok(()) => return,
            Err(error) => warn!(%error, "KWin virtual desktop service disconnected"),
        }
        let mut disconnected = state_tx.borrow().clone();
        disconnected.connected = false;
        state_tx.send_replace(disconnected);
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(5));
    }
}

async fn listen(
    state_tx: &watch::Sender<WorkspaceSnapshot>,
    commands: &mut mpsc::Receiver<Command>,
) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    let proxy = VirtualDesktopManagerProxy::new(&connection).await?;
    publish(&proxy, state_tx).await?;
    info!("connected to KWin virtual desktops");
    let mut interval = tokio::time::interval(Duration::from_millis(750));

    loop {
        tokio::select! {
            _ = interval.tick() => publish(&proxy, state_tx).await?,
            command = commands.recv() => {
                let Some(command) = command else {
                    return Ok(());
                };
                apply_command(&proxy, command).await?;
                publish(&proxy, state_tx).await?;
            }
        }
    }
}

async fn publish(
    proxy: &VirtualDesktopManagerProxy<'_>,
    state_tx: &watch::Sender<WorkspaceSnapshot>,
) -> zbus::Result<()> {
    let current = proxy.current().await?;
    let desktops = proxy.desktops().await?;
    state_tx.send_replace(snapshot(&current, desktops));
    Ok(())
}

fn snapshot(current: &str, mut desktops: Vec<DesktopData>) -> WorkspaceSnapshot {
    desktops.sort_by_key(|desktop| desktop.0);
    WorkspaceSnapshot {
        connected: true,
        supports_persistent: false,
        workspaces: desktops
            .into_iter()
            .map(|(position, reference, name)| {
                let active = reference == current;
                Workspace {
                    id: i64::from(position) + 1,
                    name: if name.is_empty() {
                        (position + 1).to_string()
                    } else {
                        name
                    },
                    reference,
                    monitor: String::new(),
                    windows: 0,
                    active,
                    visible: active,
                    urgent: false,
                    special: false,
                }
            })
            .collect(),
        ..WorkspaceSnapshot::default()
    }
}

async fn apply_command(
    proxy: &VirtualDesktopManagerProxy<'_>,
    command: Command,
) -> zbus::Result<()> {
    match command {
        Command::Focus { workspace, .. } => proxy.set_current(&workspace).await,
        Command::Relative(offset) => {
            let current = proxy.current().await?;
            let mut desktops = proxy.desktops().await?;
            desktops.sort_by_key(|desktop| desktop.0);
            if let Some(index) = desktops.iter().position(|desktop| desktop.1 == current) {
                let next = if offset < 0 {
                    index.checked_sub(1)
                } else if index + 1 < desktops.len() {
                    Some(index + 1)
                } else {
                    None
                };
                if let Some(next) = next {
                    proxy.set_current(&desktops[next].1).await?;
                }
            }
            Ok(())
        }
        Command::FocusWindow(_) => {
            debug!("KWin window focusing is not available through the virtual desktop interface");
            Ok(())
        }
    }
}

#[zbus::proxy(
    interface = "org.kde.KWin.VirtualDesktopManager",
    default_service = "org.kde.KWin",
    default_path = "/VirtualDesktopManager"
)]
trait VirtualDesktopManager {
    #[zbus(property)]
    fn current(&self) -> zbus::Result<String>;

    #[zbus(property)]
    fn set_current(&self, current: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn desktops(&self) -> zbus::Result<Vec<DesktopData>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_kwin_desktop_data() {
        let snapshot = snapshot(
            "desktop-b",
            vec![
                (1, "desktop-b".into(), "Writing".into()),
                (0, "desktop-a".into(), String::new()),
            ],
        );
        assert_eq!(snapshot.workspaces[0].name, "1");
        assert_eq!(snapshot.workspaces[1].name, "Writing");
        assert!(snapshot.workspaces[1].active);
        assert_eq!(snapshot.workspaces[1].reference, "desktop-b");
    }
}
