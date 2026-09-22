use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::services::compositor::{Command, Window, Workspace, WorkspaceSnapshot};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NiriWorkspace {
    id: u64,
    idx: u8,
    name: Option<String>,
    output: Option<String>,
    is_urgent: bool,
    is_active: bool,
    is_focused: bool,
    active_window_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NiriWindow {
    id: u64,
    title: Option<String>,
    app_id: Option<String>,
    workspace_id: Option<u64>,
    is_focused: bool,
    is_urgent: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct KeyboardLayouts {
    names: Vec<String>,
    current_idx: u8,
}

#[derive(Default)]
struct NiriState {
    workspaces: Vec<NiriWorkspace>,
    windows: HashMap<u64, NiriWindow>,
    keyboard: KeyboardLayouts,
}

pub async fn run_events(state_tx: watch::Sender<WorkspaceSnapshot>) {
    let mut retry = Duration::from_millis(250);
    loop {
        let result = async {
            let path = socket_path()?;
            let mut stream = UnixStream::connect(&path)
                .await
                .map_err(|error| error.to_string())?;
            retry = Duration::from_millis(250);
            stream
                .write_all(b"\"EventStream\"\n")
                .await
                .map_err(|error| error.to_string())?;
            info!(path = %path.display(), "connected to Niri event stream");
            let mut lines = BufReader::new(stream).lines();
            let mut state = NiriState::default();
            while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
                let value =
                    serde_json::from_str::<Value>(&line).map_err(|error| error.to_string())?;
                if value.get("Ok").is_some() {
                    continue;
                }
                if apply_event(&mut state, &value) {
                    state_tx.send_replace(snapshot(&state));
                }
            }
            Err::<(), String>("Niri event stream closed".into())
        }
        .await;

        if let Err(error) = result {
            warn!(%error, "Niri IPC disconnected");
        }
        let mut disconnected = state_tx.borrow().clone();
        disconnected.connected = false;
        state_tx.send_replace(disconnected);
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(5));
    }
}

pub async fn run_commands(mut commands: mpsc::Receiver<Command>) {
    while let Some(command) = commands.recv().await {
        let Some(request) = command_request(&command) else {
            continue;
        };
        if let Err(error) = send_request(&request).await {
            warn!(%error, "Niri command failed");
        }
    }
}

fn command_request(command: &Command) -> Option<Value> {
    Some(match command {
        Command::Focus { id, .. } => {
            let id = u64::try_from(*id).ok()?;
            json!({"Action": {"FocusWorkspace": {"reference": {"Id": id}}}})
        }
        Command::Relative(offset) if *offset < 0 => {
            json!({"Action": {"FocusWorkspaceUp": {}}})
        }
        Command::Relative(_) => json!({"Action": {"FocusWorkspaceDown": {}}}),
        Command::FocusWindow(id) => {
            let id = id.parse::<u64>().ok()?;
            json!({"Action": {"FocusWindow": {"id": id}}})
        }
    })
}

fn apply_event(state: &mut NiriState, event: &Value) -> bool {
    let Some((kind, data)) = event.as_object().and_then(|event| event.iter().next()) else {
        return false;
    };
    match kind.as_str() {
        "WorkspacesChanged" => {
            let Some(value) = data.get("workspaces") else {
                return false;
            };
            let Ok(workspaces) = serde_json::from_value(value.clone()) else {
                return false;
            };
            state.workspaces = workspaces;
        }
        "WorkspaceUrgencyChanged" => {
            let (Some(id), Some(urgent)) = (
                data.get("id").and_then(Value::as_u64),
                data.get("urgent").and_then(Value::as_bool),
            ) else {
                return false;
            };
            if let Some(workspace) = state
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == id)
            {
                workspace.is_urgent = urgent;
            }
        }
        "WorkspaceActivated" => {
            let (Some(id), Some(focused)) = (
                data.get("id").and_then(Value::as_u64),
                data.get("focused").and_then(Value::as_bool),
            ) else {
                return false;
            };
            let output = state
                .workspaces
                .iter()
                .find(|workspace| workspace.id == id)
                .and_then(|workspace| workspace.output.clone());
            for workspace in &mut state.workspaces {
                if workspace.output == output {
                    workspace.is_active = workspace.id == id;
                }
                if focused {
                    workspace.is_focused = workspace.id == id;
                }
            }
        }
        "WorkspaceActiveWindowChanged" => {
            let Some(id) = data.get("workspace_id").and_then(Value::as_u64) else {
                return false;
            };
            let active = data.get("active_window_id").and_then(Value::as_u64);
            if let Some(workspace) = state
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.id == id)
            {
                workspace.active_window_id = active;
            }
        }
        "WindowsChanged" => {
            let Some(value) = data.get("windows") else {
                return false;
            };
            let Ok(windows) = serde_json::from_value::<Vec<NiriWindow>>(value.clone()) else {
                return false;
            };
            state.windows = windows
                .into_iter()
                .map(|window| (window.id, window))
                .collect();
        }
        "WindowOpenedOrChanged" => {
            let Some(value) = data.get("window") else {
                return false;
            };
            let Ok(window) = serde_json::from_value::<NiriWindow>(value.clone()) else {
                return false;
            };
            if window.is_focused {
                for current in state.windows.values_mut() {
                    current.is_focused = false;
                }
            }
            state.windows.insert(window.id, window);
        }
        "WindowClosed" => {
            let Some(id) = data.get("id").and_then(Value::as_u64) else {
                return false;
            };
            state.windows.remove(&id);
        }
        "WindowFocusChanged" => {
            let focused = data.get("id").and_then(Value::as_u64);
            for window in state.windows.values_mut() {
                window.is_focused = Some(window.id) == focused;
            }
        }
        "WindowUrgencyChanged" => {
            let (Some(id), Some(urgent)) = (
                data.get("id").and_then(Value::as_u64),
                data.get("urgent").and_then(Value::as_bool),
            ) else {
                return false;
            };
            if let Some(window) = state.windows.get_mut(&id) {
                window.is_urgent = urgent;
            }
        }
        "KeyboardLayoutsChanged" => {
            let Some(value) = data.get("keyboard_layouts") else {
                return false;
            };
            let Ok(keyboard) = serde_json::from_value(value.clone()) else {
                return false;
            };
            state.keyboard = keyboard;
        }
        "KeyboardLayoutSwitched" => {
            let Some(idx) = data.get("idx").and_then(Value::as_u64) else {
                return false;
            };
            state.keyboard.current_idx = idx as u8;
        }
        _ => return false,
    }
    true
}

fn snapshot(state: &NiriState) -> WorkspaceSnapshot {
    let focused = state.windows.values().find(|window| window.is_focused);
    let mut workspaces = state
        .workspaces
        .iter()
        .map(|workspace| Workspace {
            id: i64::try_from(workspace.id).unwrap_or(i64::MAX),
            reference: workspace.id.to_string(),
            name: workspace
                .name
                .clone()
                .unwrap_or_else(|| workspace.idx.to_string()),
            monitor: workspace.output.clone().unwrap_or_default(),
            windows: state
                .windows
                .values()
                .filter(|window| window.workspace_id == Some(workspace.id))
                .count() as u32,
            active: workspace.is_focused,
            visible: workspace.is_active,
            urgent: workspace.is_urgent,
            special: false,
        })
        .collect::<Vec<_>>();
    workspaces.sort_by_key(|workspace| {
        state
            .workspaces
            .iter()
            .find(|item| i64::try_from(item.id).ok() == Some(workspace.id))
            .map_or(u8::MAX, |item| item.idx)
    });
    let windows = state
        .windows
        .values()
        .map(|window| Window {
            address: window.id.to_string(),
            title: window.title.clone().unwrap_or_default(),
            class: window.app_id.clone().unwrap_or_default(),
            workspace_id: window
                .workspace_id
                .and_then(|id| i64::try_from(id).ok())
                .unwrap_or_default(),
            active: window.is_focused,
        })
        .collect();
    let keyboard_layout = state
        .keyboard
        .names
        .get(state.keyboard.current_idx as usize)
        .cloned()
        .unwrap_or_default();
    WorkspaceSnapshot {
        connected: true,
        supports_persistent: false,
        workspaces,
        active_title: focused
            .and_then(|window| window.title.clone())
            .unwrap_or_default(),
        active_class: focused
            .and_then(|window| window.app_id.clone())
            .unwrap_or_default(),
        submap: String::new(),
        keyboard_layout,
        windows,
    }
}

async fn send_request(request: &Value) -> Result<(), String> {
    let mut stream = UnixStream::connect(socket_path()?)
        .await
        .map_err(|error| error.to_string())?;
    let mut encoded = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .await
        .map_err(|error| error.to_string())?;
    stream.shutdown().await.map_err(|error| error.to_string())?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .await
        .map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&response).map_err(|error| error.to_string())?;
    if value.get("Ok").is_some() {
        Ok(())
    } else {
        Err(response.trim().to_owned())
    }
}

fn socket_path() -> Result<PathBuf, String> {
    env::var_os("NIRI_SOCKET")
        .map(PathBuf::from)
        .ok_or_else(|| "NIRI_SOCKET is not set".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_stream_builds_niri_workspace_state() {
        let mut state = NiriState::default();
        let event = json!({"WorkspacesChanged": {"workspaces": [{
            "id": 8, "idx": 2, "name": "code", "output": "eDP-1",
            "is_urgent": false, "is_active": true, "is_focused": true,
            "active_window_id": 12
        }]}});
        assert!(apply_event(&mut state, &event));
        let event = json!({"WindowsChanged": {"windows": [{
            "id": 12, "title": "Editor", "app_id": "code", "workspace_id": 8,
            "is_focused": true, "is_urgent": false
        }]}});
        assert!(apply_event(&mut state, &event));
        let snapshot = snapshot(&state);
        assert_eq!(snapshot.workspaces[0].name, "code");
        assert_eq!(snapshot.workspaces[0].windows, 1);
        assert_eq!(snapshot.active_title, "Editor");
        assert!(!snapshot.supports_persistent);
    }

    #[test]
    fn commands_use_niris_stable_object_ids() {
        assert_eq!(
            command_request(&Command::Focus {
                id: 42,
                workspace: "named-workspace".into(),
                current_monitor: false,
            }),
            Some(json!({"Action": {"FocusWorkspace": {
                "reference": {"Id": 42}
            }}}))
        );
        assert_eq!(
            command_request(&Command::FocusWindow("73".into())),
            Some(json!({"Action": {"FocusWindow": {"id": 73}}}))
        );
        assert_eq!(
            command_request(&Command::Relative(-1)),
            Some(json!({"Action": {"FocusWorkspaceUp": {}}}))
        );
    }
}
