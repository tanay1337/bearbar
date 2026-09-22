use std::env;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::services::compositor::{Command, Window, Workspace, WorkspaceSnapshot};

const RUN_COMMAND: u32 = 0;
const GET_WORKSPACES: u32 = 1;
const SUBSCRIBE: u32 = 2;
const GET_TREE: u32 = 4;
const GET_BINDING_STATE: u32 = 12;
const GET_INPUTS: u32 = 100;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SwayWorkspace {
    id: i64,
    num: i64,
    name: String,
    output: String,
    visible: bool,
    focused: bool,
    urgent: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SwayNode {
    id: i64,
    name: Option<String>,
    app_id: Option<String>,
    focused: bool,
    urgent: bool,
    #[serde(rename = "type")]
    node_type: String,
    window_properties: WindowProperties,
    nodes: Vec<SwayNode>,
    floating_nodes: Vec<SwayNode>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WindowProperties {
    class: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SwayInput {
    #[serde(rename = "type")]
    input_type: String,
    xkb_active_layout_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BindingState {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CommandResult {
    success: bool,
    error: Option<String>,
}

pub async fn run_events(state_tx: watch::Sender<WorkspaceSnapshot>) {
    let mut retry = Duration::from_millis(250);
    loop {
        let result = async {
            let path = socket_path()?;
            let mut stream = UnixStream::connect(&path)
                .await
                .map_err(|error| error.to_string())?;
            write_message(
                &mut stream,
                SUBSCRIBE,
                br#"["workspace","window","mode","input"]"#,
            )
            .await?;
            let (_, response) = read_message(&mut stream).await?;
            let subscribed = serde_json::from_slice::<serde_json::Value>(&response)
                .ok()
                .and_then(|value| value.get("success").and_then(serde_json::Value::as_bool))
                .unwrap_or(false);
            if !subscribed {
                return Err::<(), String>("Sway rejected the event subscription".into());
            }

            retry = Duration::from_millis(250);
            info!(path = %path.display(), "connected to Sway IPC");
            publish_snapshot(&path, &state_tx).await;
            loop {
                read_message(&mut stream).await?;
                publish_snapshot(&path, &state_tx).await;
            }
        }
        .await;

        if let Err(error) = result {
            warn!(%error, "Sway IPC disconnected");
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
        let text = command_text(&command);
        let result = async {
            let path = socket_path()?;
            let response = request(&path, RUN_COMMAND, text.as_bytes()).await?;
            let replies = serde_json::from_slice::<Vec<CommandResult>>(&response)
                .map_err(|error| error.to_string())?;
            if let Some(failed) = replies.iter().find(|reply| !reply.success) {
                return Err(failed
                    .error
                    .clone()
                    .unwrap_or_else(|| "Sway rejected the command".into()));
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            warn!(%error, command = %text, "Sway command failed");
        }
    }
}

async fn publish_snapshot(path: &PathBuf, state_tx: &watch::Sender<WorkspaceSnapshot>) {
    match load_snapshot(path).await {
        Ok(snapshot) => {
            state_tx.send_replace(snapshot);
        }
        Err(error) => warn!(%error, "failed to read Sway workspace snapshot"),
    }
}

async fn load_snapshot(path: &PathBuf) -> Result<WorkspaceSnapshot, String> {
    let (workspace_data, tree_data, binding_data, input_data) = tokio::try_join!(
        request(path, GET_WORKSPACES, b""),
        request(path, GET_TREE, b""),
        request(path, GET_BINDING_STATE, b""),
        request(path, GET_INPUTS, b""),
    )?;
    let sway_workspaces = serde_json::from_slice::<Vec<SwayWorkspace>>(&workspace_data)
        .map_err(|error| error.to_string())?;
    let tree = serde_json::from_slice::<SwayNode>(&tree_data).map_err(|error| error.to_string())?;
    let binding = serde_json::from_slice::<BindingState>(&binding_data).unwrap_or_default();
    let inputs = serde_json::from_slice::<Vec<SwayInput>>(&input_data).unwrap_or_default();

    let mut workspaces = sway_workspaces
        .iter()
        .map(|workspace| Workspace {
            id: if workspace.num >= 0 {
                workspace.num
            } else {
                workspace.id
            },
            name: workspace.name.clone(),
            reference: workspace.name.clone(),
            monitor: workspace.output.clone(),
            windows: 0,
            active: workspace.focused,
            visible: workspace.visible,
            urgent: workspace.urgent,
            special: workspace.num < 0,
        })
        .collect::<Vec<_>>();
    let mut windows = Vec::new();
    collect_windows(&tree, None, &sway_workspaces, &mut windows);
    for workspace in &mut workspaces {
        let source_id = sway_workspaces
            .iter()
            .find(|candidate| candidate.name == workspace.name)
            .map_or(workspace.id, |candidate| candidate.id);
        workspace.windows = windows
            .iter()
            .filter(|window| window.workspace_id == source_id)
            .count() as u32;
    }
    workspaces.sort_by_key(|workspace| workspace.id);
    let focused = windows.iter().find(|window| window.active);

    Ok(WorkspaceSnapshot {
        connected: true,
        supports_persistent: true,
        workspaces,
        active_title: focused.map_or_else(String::new, |window| window.title.clone()),
        active_class: focused.map_or_else(String::new, |window| window.class.clone()),
        submap: binding.name,
        keyboard_layout: inputs
            .into_iter()
            .find(|input| input.input_type == "keyboard")
            .and_then(|input| input.xkb_active_layout_name)
            .unwrap_or_default(),
        windows,
    })
}

fn collect_windows(
    node: &SwayNode,
    workspace_id: Option<i64>,
    workspaces: &[SwayWorkspace],
    windows: &mut Vec<Window>,
) {
    let workspace_id = if node.node_type == "workspace" {
        node.name
            .as_deref()
            .and_then(|name| workspaces.iter().find(|workspace| workspace.name == name))
            .map(|workspace| workspace.id)
            .or(workspace_id)
    } else {
        workspace_id
    };
    let class = node
        .app_id
        .as_deref()
        .or(node.window_properties.class.as_deref())
        .unwrap_or_default();
    if workspace_id.is_some() && node.nodes.is_empty() && !class.is_empty() {
        windows.push(Window {
            address: node.id.to_string(),
            title: node.name.clone().unwrap_or_default(),
            class: class.to_owned(),
            workspace_id: workspace_id.unwrap_or_default(),
            active: node.focused,
        });
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_windows(child, workspace_id, workspaces, windows);
    }
}

fn command_text(command: &Command) -> String {
    match command {
        Command::Focus { workspace, .. } => format!(
            "workspace --no-auto-back-and-forth \"{}\"",
            escape_argument(workspace)
        ),
        Command::Relative(offset) if *offset < 0 => "workspace prev_on_output".into(),
        Command::Relative(_) => "workspace next_on_output".into(),
        Command::FocusWindow(id) => format!("[con_id={id}] focus"),
    }
}

fn escape_argument(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn socket_path() -> Result<PathBuf, String> {
    env::var_os("SWAYSOCK")
        .or_else(|| env::var_os("I3SOCK"))
        .map(PathBuf::from)
        .ok_or_else(|| "SWAYSOCK and I3SOCK are not set".into())
}

async fn request(path: &PathBuf, message_type: u32, payload: &[u8]) -> Result<Vec<u8>, String> {
    let mut stream = UnixStream::connect(path)
        .await
        .map_err(|error| error.to_string())?;
    write_message(&mut stream, message_type, payload).await?;
    let (_, response) = read_message(&mut stream).await?;
    Ok(response)
}

async fn write_message(
    stream: &mut UnixStream,
    message_type: u32,
    payload: &[u8],
) -> Result<(), String> {
    let length = u32::try_from(payload.len()).map_err(|error| error.to_string())?;
    let mut header = [0_u8; 14];
    header[..6].copy_from_slice(b"i3-ipc");
    header[6..10].copy_from_slice(&length.to_le_bytes());
    header[10..14].copy_from_slice(&message_type.to_le_bytes());
    stream
        .write_all(&header)
        .await
        .map_err(|error| error.to_string())?;
    stream
        .write_all(payload)
        .await
        .map_err(|error| error.to_string())
}

async fn read_message(stream: &mut UnixStream) -> Result<(u32, Vec<u8>), String> {
    let mut header = [0_u8; 14];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|error| error.to_string())?;
    if &header[..6] != b"i3-ipc" {
        return Err("invalid Sway IPC response".into());
    }
    let length = u32::from_le_bytes(header[6..10].try_into().expect("four bytes"));
    let message_type = u32::from_le_bytes(header[10..14].try_into().expect("four bytes"));
    let mut payload = vec![0; length as usize];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|error| error.to_string())?;
    Ok((message_type, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sway_workspace_commands() {
        let command = Command::Focus {
            id: 2,
            workspace: "2: web".into(),
            current_monitor: false,
        };
        assert_eq!(
            command_text(&command),
            "workspace --no-auto-back-and-forth \"2: web\""
        );
        assert_eq!(
            command_text(&Command::Relative(-1)),
            "workspace prev_on_output"
        );
    }

    #[test]
    fn extracts_windows_from_the_sway_tree() {
        let tree: SwayNode = serde_json::from_str(
            r#"{"id":1,"type":"root","nodes":[{"id":2,"type":"workspace","name":"1","nodes":[{"id":9,"type":"con","name":"Editor","app_id":"code","focused":true}]}]}"#,
        )
        .unwrap();
        let workspaces = vec![SwayWorkspace {
            id: 2,
            num: 1,
            name: "1".into(),
            ..SwayWorkspace::default()
        }];
        let mut windows = Vec::new();
        collect_windows(&tree, None, &workspaces, &mut windows);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].class, "code");
        assert!(windows[0].active);
    }
}
