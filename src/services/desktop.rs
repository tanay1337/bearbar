use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command as Process};
use tokio::sync::{mpsc, watch};
use tracing::warn;

use crate::runtime;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopSnapshot {
    pub network_connected: bool,
    pub network_name: String,
    pub network_strength: u8,
    pub wifi_enabled: bool,
    pub wifi_networks: Vec<WifiNetwork>,
    pub brightness: Option<u8>,
    pub power_profile: String,
    pub bluetooth_available: bool,
    pub bluetooth_powered: bool,
    pub bluetooth_devices: Vec<BluetoothDevice>,
    pub media: Option<MediaSnapshot>,
    pub inhibited: bool,
    pub microphone_active: bool,
    pub camera_active: bool,
    pub screen_active: bool,
    pub notifications_available: bool,
    pub notification_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub address: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
    pub battery: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub strength: u8,
    pub security: String,
    pub active: bool,
    pub saved: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaSnapshot {
    pub player: String,
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    Previous,
    PlayPause,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopCommand {
    SetBrightness(u8),
    SetPowerProfile(String),
    ToggleBluetooth,
    ToggleBluetoothDevice {
        address: String,
        connected: bool,
        paired: bool,
    },
    ToggleWifi,
    ConnectWifi {
        ssid: String,
        password: Option<String>,
        saved: bool,
    },
    RefreshConnectivity,
    Media(MediaCommand),
    ToggleInhibit,
    ToggleNotifications,
}

#[derive(Debug, Clone)]
pub struct DesktopService {
    state: watch::Receiver<DesktopSnapshot>,
    commands: mpsc::Sender<DesktopCommand>,
}

impl DesktopService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(DesktopSnapshot::default());
        let (commands, command_rx) = mpsc::channel(32);
        runtime::spawn(run(state_tx, command_rx));
        Self { state, commands }
    }

    pub fn subscribe(&self) -> watch::Receiver<DesktopSnapshot> {
        self.state.clone()
    }

    pub fn send(&self, command: DesktopCommand) {
        if self.commands.try_send(command).is_err() {
            warn!("desktop command queue is full or closed");
        }
    }
}

async fn run(
    state_tx: watch::Sender<DesktopSnapshot>,
    mut commands: mpsc::Receiver<DesktopCommand>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    let mut inhibitor: Option<Child> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let inhibited = inhibitor.as_mut().is_some_and(|child| {
                    child.try_wait().ok().flatten().is_none()
                });
                if !inhibited {
                    inhibitor = None;
                }
                state_tx.send_replace(read_snapshot(inhibited).await);
            }
            command = commands.recv() => {
                let Some(command) = command else { break };
                let brightness = match &command {
                    DesktopCommand::SetBrightness(value) => Some(*value),
                    _ => None,
                };
                let refresh = !matches!(&command, DesktopCommand::RefreshConnectivity);
                handle_command(command, &state_tx, &mut inhibitor).await;
                if let Some(brightness) = brightness {
                    let mut snapshot = state_tx.borrow().clone();
                    snapshot.brightness = Some(brightness.clamp(1, 100));
                    state_tx.send_replace(snapshot);
                } else if refresh {
                    let inhibited = inhibitor.is_some();
                    state_tx.send_replace(read_snapshot(inhibited).await);
                }
            }
        }
    }
}

async fn handle_command(
    command: DesktopCommand,
    state: &watch::Sender<DesktopSnapshot>,
    inhibitor: &mut Option<Child>,
) {
    match command {
        DesktopCommand::SetBrightness(percent) => {
            run_quiet(
                "brightnessctl",
                &["-q", "set", &format!("{}%", percent.clamp(1, 100))],
            )
            .await;
        }
        DesktopCommand::SetPowerProfile(profile) => {
            run_quiet("powerprofilesctl", &["set", &profile]).await;
        }
        DesktopCommand::ToggleBluetooth => {
            let powered = state.borrow().bluetooth_powered;
            run_quiet(
                "bluetoothctl",
                &["power", if powered { "off" } else { "on" }],
            )
            .await;
        }
        DesktopCommand::ToggleBluetoothDevice {
            address,
            connected,
            paired,
        } => {
            if !connected && !paired {
                run_quiet_timeout("bluetoothctl", &["pair", &address], Duration::from_secs(30))
                    .await;
                run_quiet_timeout("bluetoothctl", &["trust", &address], Duration::from_secs(5))
                    .await;
            }
            run_quiet_timeout(
                "bluetoothctl",
                &[if connected { "disconnect" } else { "connect" }, &address],
                Duration::from_secs(20),
            )
            .await;
        }
        DesktopCommand::ToggleWifi => {
            let enabled = state.borrow().wifi_enabled;
            run_quiet(
                "nmcli",
                &["radio", "wifi", if enabled { "off" } else { "on" }],
            )
            .await;
        }
        DesktopCommand::ConnectWifi {
            ssid,
            password,
            saved,
        } => {
            if saved {
                run_quiet_timeout(
                    "nmcli",
                    &["--wait", "20", "connection", "up", "id", &ssid],
                    Duration::from_secs(25),
                )
                .await;
            } else {
                let mut args = vec!["--wait", "20", "device", "wifi", "connect", ssid.as_str()];
                if let Some(password) = password.as_deref() {
                    args.extend(["password", password]);
                }
                run_quiet_timeout("nmcli", &args, Duration::from_secs(25)).await;
            }
        }
        DesktopCommand::RefreshConnectivity => {
            tokio::spawn(async {
                run_quiet("nmcli", &["device", "wifi", "rescan"]).await;
            });
            tokio::spawn(async {
                let _ = tokio::time::timeout(
                    Duration::from_secs(7),
                    Process::new("bluetoothctl")
                        .args(["--timeout", "5", "scan", "on"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .output(),
                )
                .await;
            });
        }
        DesktopCommand::Media(action) => {
            let action = match action {
                MediaCommand::Previous => "previous",
                MediaCommand::PlayPause => "play-pause",
                MediaCommand::Next => "next",
            };
            run_quiet("playerctl", &[action]).await;
        }
        DesktopCommand::ToggleInhibit => {
            if let Some(mut child) = inhibitor.take() {
                let _ = child.kill().await;
            } else {
                match Process::new("systemd-inhibit")
                    .args([
                        "--what=idle:sleep",
                        "--who=Bearbar",
                        "--why=Enabled from the Bearbar top panel",
                        "sleep",
                        "infinity",
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => *inhibitor = Some(child),
                    Err(error) => warn!(%error, "failed to start idle inhibitor"),
                }
            }
        }
        DesktopCommand::ToggleNotifications => {
            run_quiet("swaync-client", &["-t"]).await;
        }
    }
}

async fn read_snapshot(inhibited: bool) -> DesktopSnapshot {
    let (network, brightness, power_profile, bluetooth, media, privacy, notifications) = tokio::join!(
        read_network(),
        read_brightness(),
        read_power_profile(),
        read_bluetooth(),
        read_media(),
        read_privacy(),
        read_notifications(),
    );
    DesktopSnapshot {
        network_connected: network.connected,
        network_name: network.name,
        network_strength: network.strength,
        wifi_enabled: network.wifi_enabled,
        wifi_networks: network.networks,
        brightness,
        power_profile,
        bluetooth_available: bluetooth.0,
        bluetooth_powered: bluetooth.1,
        bluetooth_devices: bluetooth.2,
        media,
        inhibited,
        microphone_active: privacy.0,
        camera_active: privacy.1,
        screen_active: privacy.2,
        notifications_available: notifications.is_some(),
        notification_count: notifications.unwrap_or_default(),
    }
}

async fn read_notifications() -> Option<u32> {
    output("swaync-client", &["-c"]).await?.trim().parse().ok()
}

async fn read_privacy() -> (bool, bool, bool) {
    let Some(dump) = output("pw-dump", &[]).await else {
        return (false, false, false);
    };
    let Ok(objects) = serde_json::from_str::<Vec<serde_json::Value>>(&dump) else {
        return (false, false, false);
    };
    let mut microphone = false;
    let mut camera = false;
    let mut screen = false;
    for object in objects {
        if object.get("type").and_then(|value| value.as_str()) != Some("PipeWire:Interface:Node") {
            continue;
        }
        let Some(info) = object.get("info") else {
            continue;
        };
        if info.get("state").and_then(|value| value.as_str()) != Some("running") {
            continue;
        }
        let Some(props) = info.get("props") else {
            continue;
        };
        let media_class = props
            .get("media.class")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let media_role = props
            .get("media.role")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let node_name = props
            .get("node.name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if media_class == "Stream/Input/Audio" {
            microphone = true;
        } else if media_class == "Stream/Input/Video" {
            if media_role.eq_ignore_ascii_case("Screen") || node_name.contains("screen") {
                screen = true;
            } else {
                camera = true;
            }
        }
    }
    (microphone, camera, screen)
}

#[derive(Default)]
struct NetworkState {
    connected: bool,
    name: String,
    strength: u8,
    wifi_enabled: bool,
    networks: Vec<WifiNetwork>,
}

async fn read_network() -> NetworkState {
    let general = output("nmcli", &["-t", "-f", "STATE", "general"])
        .await
        .unwrap_or_default();
    let connected = general.lines().next().is_some_and(|line| {
        matches!(
            line.trim(),
            "connected" | "connected (site only)" | "connected (local only)"
        )
    });
    let wifi_enabled = output("nmcli", &["-t", "-f", "WIFI", "radio"])
        .await
        .is_some_and(|value| value.trim() == "enabled");
    let saved = output(
        "nmcli",
        &[
            "-t",
            "--escape",
            "yes",
            "-f",
            "NAME,TYPE",
            "connection",
            "show",
        ],
    )
    .await
    .unwrap_or_default()
    .lines()
    .filter_map(|line| {
        let fields = split_nmcli_fields(line);
        let [name, kind, ..] = fields.as_slice() else {
            return None;
        };
        matches!(kind.as_str(), "802-11-wireless" | "wifi").then(|| name.to_owned())
    })
    .collect::<HashSet<_>>();
    let wifi = output(
        "nmcli",
        &[
            "-t",
            "--escape",
            "yes",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "no",
        ],
    )
    .await
    .unwrap_or_default();
    let mut by_name = std::collections::HashMap::<String, WifiNetwork>::new();
    for line in wifi.lines() {
        let fields = split_nmcli_fields(line);
        let in_use = fields
            .first()
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let ssid = fields.get(1).map(String::as_str).unwrap_or_default().trim();
        if ssid.is_empty() {
            continue;
        }
        let strength = fields
            .get(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or_default();
        let security = fields.get(3).cloned().unwrap_or_default();
        let network = WifiNetwork {
            ssid: ssid.to_owned(),
            strength,
            security,
            active: matches!(in_use, "yes" | "*"),
            saved: saved.contains(ssid),
        };
        let current = by_name
            .entry(network.ssid.clone())
            .or_insert_with(|| network.clone());
        if network.active || network.strength > current.strength {
            *current = network;
        }
    }
    let mut networks = by_name.into_values().collect::<Vec<_>>();
    networks.sort_by_key(|network| (!network.active, std::cmp::Reverse(network.strength)));
    let active = networks.iter().find(|network| network.active);
    NetworkState {
        connected,
        name: active
            .map(|network| network.ssid.clone())
            .unwrap_or_default(),
        strength: active.map_or(0, |network| network.strength),
        wifi_enabled,
        networks,
    }
}

fn split_nmcli_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            fields.last_mut().expect("one field").push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().expect("one field").push(character);
        }
    }
    if escaped {
        fields.last_mut().expect("one field").push('\\');
    }
    fields
}

async fn read_brightness() -> Option<u8> {
    let value = output("brightnessctl", &["-m"]).await?;
    value
        .lines()
        .next()?
        .split(',')
        .find_map(|field| field.strip_suffix('%')?.parse().ok())
}

async fn read_power_profile() -> String {
    output("powerprofilesctl", &["get"])
        .await
        .unwrap_or_default()
        .trim()
        .to_owned()
}

async fn read_bluetooth() -> (bool, bool, Vec<BluetoothDevice>) {
    let Some(show) = output("bluetoothctl", &["show"]).await else {
        return (false, false, Vec::new());
    };
    let powered = show.lines().any(|line| line.trim() == "Powered: yes");
    let paired = output("bluetoothctl", &["devices"])
        .await
        .unwrap_or_default();
    let candidates = paired.lines().filter_map(|line| {
        let mut fields = line.splitn(3, ' ');
        if fields.next() != Some("Device") {
            return None;
        }
        let address = fields.next().unwrap_or_default().to_owned();
        let name = fields.next().unwrap_or("Bluetooth device").to_owned();
        Some((address, name))
    });
    let devices =
        futures_util::future::join_all(candidates.map(|(address, listed_name)| async move {
            let info = output("bluetoothctl", &["info", &address])
                .await
                .unwrap_or_default();
            let connected = info.lines().any(|line| line.trim() == "Connected: yes");
            let paired = info.lines().any(|line| line.trim() == "Paired: yes");
            let battery = parse_bluetooth_battery(&info);
            let name = bluetooth_display_name(&address, &listed_name, &info);
            BluetoothDevice {
                address,
                name,
                connected,
                paired,
                battery,
            }
        }))
        .await;
    (true, powered, devices)
}

fn bluetooth_display_name(address: &str, listed_name: &str, info: &str) -> String {
    ["Alias:", "Name:"]
        .into_iter()
        .filter_map(|property| {
            info.lines()
                .find_map(|line| line.trim().strip_prefix(property).map(str::trim))
        })
        .chain(std::iter::once(listed_name.trim()))
        .find(|name| !name.is_empty() && !same_bluetooth_address(name, address))
        .map(str::to_owned)
        .unwrap_or_else(|| bluetooth_type_name(info).to_owned())
}

fn same_bluetooth_address(value: &str, address: &str) -> bool {
    let normalize = |text: &str| {
        text.chars()
            .filter(|character| character.is_ascii_hexdigit())
            .map(|character| character.to_ascii_uppercase())
            .collect::<String>()
    };
    let address = normalize(address);
    address.len() == 12 && normalize(value) == address
}

fn bluetooth_type_name(info: &str) -> &'static str {
    let icon = info
        .lines()
        .find_map(|line| line.trim().strip_prefix("Icon:").map(str::trim))
        .unwrap_or_default();
    if icon.contains("headset") || icon.contains("headphones") || icon.contains("audio") {
        "Audio device"
    } else if icon.contains("keyboard") {
        "Keyboard"
    } else if icon.contains("mouse") {
        "Mouse"
    } else if icon.contains("phone") {
        "Phone"
    } else if icon.contains("computer") {
        "Computer"
    } else {
        "Bluetooth device"
    }
}

fn parse_bluetooth_battery(info: &str) -> Option<u8> {
    let line = info
        .lines()
        .find(|line| line.trim_start().starts_with("Battery Percentage:"))?;
    let value = line.split_once("0x")?.1.split_whitespace().next()?;
    u8::from_str_radix(value, 16).ok()
}

async fn read_media() -> Option<MediaSnapshot> {
    const FORMAT: &str =
        "{{playerName}}\x1f{{status}}\x1f{{title}}\x1f{{artist}}\x1f{{album}}\x1f{{mpris:artUrl}}";
    let metadata = output(
        "playerctl",
        &["--all-players", "--format", FORMAT, "metadata"],
    )
    .await?;
    let mut candidates = metadata.lines().filter_map(parse_media).collect::<Vec<_>>();
    candidates.sort_by_key(|media| media.status != "Playing");
    candidates.into_iter().next()
}

fn parse_media(line: &str) -> Option<MediaSnapshot> {
    let mut fields = line.split('\x1f');
    let media = MediaSnapshot {
        player: fields.next()?.to_owned(),
        status: fields.next()?.to_owned(),
        title: fields.next()?.to_owned(),
        artist: fields.next()?.to_owned(),
        album: fields.next()?.to_owned(),
        art_url: fields.next().unwrap_or_default().to_owned(),
    };
    (!media.title.is_empty()).then_some(media)
}

async fn output(program: &str, args: &[&str]) -> Option<String> {
    output_timeout(program, args, Duration::from_secs(2)).await
}

async fn output_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let result = tokio::time::timeout(
        timeout,
        Process::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).into_owned())
}

async fn run_quiet(program: &str, args: &[&str]) {
    let _ = output(program, args).await;
}

async fn run_quiet_timeout(program: &str, args: &[&str], timeout: Duration) {
    let _ = output_timeout(program, args, timeout).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bluetooth_percentage() {
        let info = "\tConnected: yes\n\tBattery Percentage: 0x4b (75)\n";
        assert_eq!(parse_bluetooth_battery(info), Some(75));
    }

    #[test]
    fn parses_media_fields() {
        let item =
            parse_media("spotify\x1fPlaying\x1fTrack\x1fArtist\x1fAlbum\x1ffile:///cover.jpg")
                .unwrap();
        assert_eq!(item.player, "spotify");
        assert_eq!(item.title, "Track");
        assert_eq!(item.artist, "Artist");
    }

    #[test]
    fn finds_brightness_percentage_in_machine_output() {
        let value = "intel_backlight,backlight,7707,4%,174545";
        let percent = value
            .split(',')
            .find_map(|field| field.strip_suffix('%')?.parse::<u8>().ok());
        assert_eq!(percent, Some(4));
    }

    #[test]
    fn prefers_bluetooth_alias_over_an_address() {
        let info = "Device AA:BB:CC:DD:EE:FF\n\tName: WH-1000XM5\n\tAlias: Studio Headphones\n";
        assert_eq!(
            bluetooth_display_name("AA:BB:CC:DD:EE:FF", "AA:BB:CC:DD:EE:FF", info),
            "Studio Headphones"
        );
    }

    #[test]
    fn falls_back_to_the_bluetooth_name() {
        let info = "Device AA:BB:CC:DD:EE:FF\n\tName: MX Keys\n";
        assert_eq!(
            bluetooth_display_name("AA:BB:CC:DD:EE:FF", "AA:BB:CC:DD:EE:FF", info),
            "MX Keys"
        );
    }

    #[test]
    fn rejects_dash_formatted_bluetooth_addresses() {
        let info = "Device AA:BB:CC:DD:EE:FF\n\tAlias: AA-BB-CC-DD-EE-FF\n\tName: Keychron K3\n";
        assert_eq!(
            bluetooth_display_name("AA:BB:CC:DD:EE:FF", "AA-BB-CC-DD-EE-FF", info),
            "Keychron K3"
        );
    }

    #[test]
    fn uses_device_type_when_bluez_has_no_name() {
        let info = "Device AA:BB:CC:DD:EE:FF\n\tAlias: AA-BB-CC-DD-EE-FF\n\tIcon: input-keyboard\n";
        assert_eq!(
            bluetooth_display_name("AA:BB:CC:DD:EE:FF", "AA-BB-CC-DD-EE-FF", info),
            "Keyboard"
        );
    }

    #[test]
    fn parses_escaped_nmcli_fields() {
        assert_eq!(
            split_nmcli_fields("*:Cafe\\: Upstairs:81:WPA2"),
            ["*", "Cafe: Upstairs", "81", "WPA2"]
        );
    }
}
