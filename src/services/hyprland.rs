use std::cmp::Ordering;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::services::compositor::{Command, Window, Workspace, WorkspaceSnapshot};

pub async fn run_events(state_tx: watch::Sender<WorkspaceSnapshot>) {
    let mut retry = Duration::from_millis(250);

    loop {
        match event_socket_path().and_then(|path| Ok((path.clone(), request_socket_path()?))) {
            Ok((event_path, request_path)) => match UnixStream::connect(&event_path).await {
                Ok(stream) => {
                    info!(path = %event_path.display(), "connected to Hyprland events");
                    retry = Duration::from_millis(250);

                    publish_snapshot(&request_path, &state_tx).await;
                    let mut lines = BufReader::new(stream).lines();

                    loop {
                        match lines.next_line().await {
                            Ok(Some(line)) => {
                                if event_needs_refresh(&line) {
                                    debug!(event = %line, "refreshing Hyprland state");
                                    publish_snapshot(&request_path, &state_tx).await;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                warn!(%error, "Hyprland event socket failed");
                                break;
                            }
                        }
                    }
                }
                Err(error) => warn!(%error, "cannot connect to Hyprland event socket"),
            },
            Err(error) => warn!(%error, "cannot locate Hyprland IPC sockets"),
        }

        let mut disconnected = state_tx.borrow().clone();
        disconnected.connected = false;
        state_tx.send_replace(disconnected);

        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(5));
    }
}

async fn publish_snapshot(path: &PathBuf, state_tx: &watch::Sender<WorkspaceSnapshot>) {
    match load_snapshot(path).await {
        Ok(snapshot) => {
            state_tx.send_replace(snapshot);
        }
        Err(error) => warn!(%error, "failed to read Hyprland workspace snapshot"),
    }
}

pub async fn run_commands(mut commands: mpsc::Receiver<Command>) {
    while let Some(command) = commands.recv().await {
        let (request, legacy_request) = match command {
            Command::Focus {
                id: _,
                workspace,
                current_monitor,
            } => {
                let legacy_dispatcher = if current_monitor {
                    "focusworkspaceoncurrentmonitor"
                } else {
                    "workspace"
                };
                let selector = workspace_selector(&workspace);
                let current_monitor = if current_monitor {
                    ", on_current_monitor = true"
                } else {
                    ""
                };
                (
                    format!(
                        "/dispatch hl.dsp.focus({{ workspace = {}{current_monitor} }})",
                        lua_string(&selector)
                    ),
                    format!("/dispatch {legacy_dispatcher} {selector}"),
                )
            }
            Command::Relative(offset) if offset < 0 => (
                "/dispatch hl.dsp.focus({ workspace = \"e-1\" })".into(),
                "/dispatch workspace e-1".into(),
            ),
            Command::Relative(_) => (
                "/dispatch hl.dsp.focus({ workspace = \"e+1\" })".into(),
                "/dispatch workspace e+1".into(),
            ),
            Command::FocusWindow(address) => (
                format!(
                    "/dispatch hl.dsp.focus({{ window = {} }})",
                    lua_string(&format!("address:{address}"))
                ),
                format!("/dispatch focuswindow address:{address}"),
            ),
        };

        match request_socket_path() {
            Ok(path) => {
                if let Err(error) = dispatch(&path, &request, &legacy_request).await {
                    warn!(%error, "Hyprland command failed");
                }
            }
            Err(error) => warn!(%error, "cannot locate Hyprland request socket"),
        }
    }
}

async fn dispatch(path: &PathBuf, request: &str, legacy_request: &str) -> Result<(), String> {
    let response = send_request(path, request).await?;
    if response_is_ok(&response) {
        return Ok(());
    }

    debug!(
        response = %String::from_utf8_lossy(&response).trim(),
        "Hyprland rejected the Lua dispatcher; trying the pre-0.55 syntax"
    );
    let response = send_request(path, legacy_request).await?;
    if response_is_ok(&response) {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&response).trim().to_owned())
    }
}

fn response_is_ok(response: &[u8]) -> bool {
    String::from_utf8_lossy(response).trim() == "ok"
}

fn lua_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn workspace_selector(name: &str) -> String {
    if name.parse::<i64>().is_ok() || name.starts_with("special:") {
        name.to_owned()
    } else {
        format!("name:{name}")
    }
}

async fn load_snapshot(path: &PathBuf) -> Result<WorkspaceSnapshot, String> {
    let (workspaces, monitors, clients, active_window, devices) = tokio::try_join!(
        request_json::<Vec<WorkspaceReply>>(path, "j/workspaces"),
        request_json::<Vec<MonitorReply>>(path, "j/monitors"),
        request_json::<Vec<ClientReply>>(path, "j/clients"),
        request_json::<ActiveWindowReply>(path, "j/activewindow"),
        request_json::<DevicesReply>(path, "j/devices"),
    )?;
    let submap = request_text(path, "submap").await.unwrap_or_default();

    let mut result = workspaces
        .into_iter()
        .map(|workspace| {
            let active = monitors
                .iter()
                .any(|monitor| monitor.focused && monitor.active_workspace.id == workspace.id);
            let visible = monitors
                .iter()
                .any(|monitor| monitor.active_workspace.id == workspace.id);
            let urgent = clients
                .iter()
                .any(|client| client.urgent && client.workspace.id == workspace.id);

            Workspace {
                id: workspace.id,
                reference: workspace.name.clone(),
                special: workspace.id < 0 || workspace.name.starts_with("special:"),
                name: workspace.name,
                monitor: workspace.monitor,
                windows: workspace.windows.max(0) as u32,
                active,
                visible,
                urgent,
            }
        })
        .collect::<Vec<_>>();

    result.sort_by(workspace_order);

    let windows = clients
        .iter()
        .filter(|client| !client.address.is_empty() && client.mapped)
        .map(|client| Window {
            address: client.address.clone(),
            title: client.title.clone(),
            class: client.class.clone(),
            workspace_id: client.workspace.id,
            active: client.address == active_window.address,
        })
        .collect();
    let keyboard_layout = devices
        .keyboards
        .iter()
        .find(|keyboard| keyboard.main)
        .or_else(|| devices.keyboards.first())
        .map(|keyboard| keyboard.active_keymap.clone())
        .unwrap_or_default();

    Ok(WorkspaceSnapshot {
        connected: true,
        supports_persistent: true,
        workspaces: result,
        active_title: active_window.title,
        active_class: active_window.class,
        submap,
        keyboard_layout,
        windows,
    })
}

fn workspace_order(left: &Workspace, right: &Workspace) -> Ordering {
    match (left.name.parse::<i64>(), right.name.parse::<i64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.name.cmp(&right.name),
    }
}

async fn request_json<T>(path: &PathBuf, request: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let response = send_request(path, request).await?;
    serde_json::from_slice(&response).map_err(|error| error.to_string())
}

async fn request_text(path: &PathBuf, request: &str) -> Result<String, String> {
    let response = send_request(path, request).await?;
    Ok(String::from_utf8_lossy(&response).trim().to_owned())
}

async fn send_request(path: &PathBuf, request: &str) -> Result<Vec<u8>, String> {
    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stream.shutdown().await.map_err(|error| error.to_string())?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|error| error.to_string())?;
    Ok(response)
}

fn event_needs_refresh(line: &str) -> bool {
    let Some((event, _)) = line.split_once(">>") else {
        return false;
    };

    matches!(
        event,
        "workspace"
            | "workspacev2"
            | "focusedmon"
            | "focusedmonv2"
            | "createworkspace"
            | "createworkspacev2"
            | "destroyworkspace"
            | "destroyworkspacev2"
            | "moveworkspace"
            | "moveworkspacev2"
            | "renameworkspace"
            | "activespecial"
            | "activespecialv2"
            | "openwindow"
            | "closewindow"
            | "movewindow"
            | "movewindowv2"
            | "urgent"
            | "monitoradded"
            | "monitoraddedv2"
            | "monitorremoved"
            | "monitorremovedv2"
            | "activewindow"
            | "activewindowv2"
            | "windowtitle"
            | "windowtitlev2"
            | "submap"
            | "activelayout"
    )
}

fn request_socket_path() -> Result<PathBuf, String> {
    socket_path(".socket.sock")
}

fn event_socket_path() -> Result<PathBuf, String> {
    socket_path(".socket2.sock")
}

fn socket_path(socket: &str) -> Result<PathBuf, String> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "XDG_RUNTIME_DIR is not set".to_owned())?;
    let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .ok_or_else(|| "HYPRLAND_INSTANCE_SIGNATURE is not set".to_owned())?;

    Ok(runtime.join("hypr").join(signature).join(socket))
}

#[derive(Debug, Deserialize)]
struct WorkspaceReply {
    id: i64,
    name: String,
    monitor: String,
    windows: i64,
}

#[derive(Debug, Deserialize)]
struct MonitorReply {
    focused: bool,
    #[serde(rename = "activeWorkspace")]
    active_workspace: WorkspaceRef,
}

#[derive(Debug, Deserialize)]
struct ClientReply {
    #[serde(default)]
    address: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    class: String,
    #[serde(default = "default_true")]
    mapped: bool,
    #[serde(default)]
    urgent: bool,
    workspace: WorkspaceRef,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ActiveWindowReply {
    address: String,
    title: String,
    class: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DevicesReply {
    keyboards: Vec<KeyboardReply>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct KeyboardReply {
    main: bool,
    active_keymap: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceRef {
    id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_workspace_events_only() {
        assert!(event_needs_refresh("workspacev2>>2,2"));
        assert!(event_needs_refresh("urgent>>abc"));
        assert!(event_needs_refresh("activewindow>>foot,Terminal"));
        assert!(event_needs_refresh("submap>>resize"));
        assert!(!event_needs_refresh("malformed"));
    }

    #[test]
    fn selects_named_workspaces_explicitly() {
        assert_eq!(workspace_selector("3"), "3");
        assert_eq!(workspace_selector("web"), "name:web");
        assert_eq!(workspace_selector("special:magic"), "special:magic");
    }

    #[test]
    fn quotes_workspace_names_for_lua_dispatchers() {
        assert_eq!(lua_string("web"), "\"web\"");
        assert_eq!(lua_string("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn only_accepts_an_exact_success_response() {
        assert!(response_is_ok(b"ok\n"));
        assert!(!response_is_ok(b"error: nope"));
    }

    #[test]
    fn sorts_numbers_before_names() {
        let workspace = |name: &str| Workspace {
            id: 0,
            name: name.into(),
            reference: name.into(),
            monitor: String::new(),
            windows: 0,
            active: false,
            visible: false,
            urgent: false,
            special: false,
        };
        let mut values = [workspace("web"), workspace("10"), workspace("2")];
        values.sort_by(workspace_order);
        assert_eq!(
            values
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["2", "10", "web"]
        );
    }
}
