<p align="center">
  <img src="assets/bearbar-logo.svg" width="100" alt="Bearbar">
</p>

<h1 align="center">Bearbar</h1>

A compact GTK4 bar for Hyprland, Niri, Sway, and KDE Plasma. Bearbar includes a
small default theme, bundled symbolic icons, live configuration reload, and
native compositor, audio, battery, and tray services.

## Installation

Bearbar requires Rust 1.92+, GTK 4.12+, GTK4 Layer Shell, PulseAudio client
libraries, UPower, and the GLib development tools.

Install the build and runtime dependencies on Arch Linux:

```sh
sudo pacman -S --needed base-devel rust gtk4 gtk4-layer-shell libpulse upower
```

On Ubuntu 24.04 or newer:

```sh
sudo apt install build-essential libglib2.0-dev libgtk-4-dev \
  libgtk4-layer-shell-dev libpulse-dev pkg-config upower
```

Use [rustup](https://rustup.rs/) if your distribution does not provide Rust
1.92 or newer. Then build and install Bearbar:

```sh
git clone https://github.com/tanay1337/bearbar.git
cd bearbar
cargo build --release --locked
install -Dm755 target/release/bearbar ~/.local/bin/bearbar
install -Dm644 examples/config.toml ~/.config/bearbar/config.toml
install -Dm644 examples/style.css ~/.config/bearbar/style.css
```

Optional integrations are enabled when their command is installed:

| Feature | Dependency |
|---|---|
| Wi-Fi | NetworkManager (`nmcli`) |
| Bluetooth | BlueZ (`bluetoothctl`) |
| Brightness | `brightnessctl` |
| Power modes | `power-profiles-daemon` |
| Media | `playerctl` and an MPRIS player |
| Privacy indicators | PipeWire (`pw-dump`) |
| Notifications | SwayNC (`swaync-client`) |
| Clipboard | `cliphist` and `wl-clipboard` |

For PipeWire audio, its PulseAudio compatibility service must be running.
`glib-compile-resources` is required while building and normally ships with the
GLib development tools.

### Start Bearbar

Start Bearbar directly from your compositor using the appropriate line:

| Compositor | Configuration |
|---|---|
| Hyprland | `exec-once = bearbar` |
| Niri | `spawn-at-startup "bearbar"` |
| Sway | `exec bearbar` |
| KDE Plasma | Add `~/.local/bin/bearbar` in **System Settings → Autostart** |

Alternatively, install and enable the included user service:

```sh
install -Dm644 contrib/bearbar.service ~/.config/systemd/user/bearbar.service
systemctl --user daemon-reload
systemctl --user enable --now bearbar.service
```

Choose either compositor autostart or the user service, not both. Stop Waybar or
any other panel reserving the same screen edge before starting Bearbar.

Hyprland is the primary and most thoroughly tested compositor. Niri, Sway, and
KDE Plasma support is newer; bug reports from those sessions are welcome.

## Configure

Bearbar reads `~/.config/bearbar/config.toml` and `style.css`. Both files reload
when saved. Invalid TOML is rejected without replacing the active layout.

```sh
bearbar --check-config
bearbar --config /path/to/config.toml --style /path/to/style.css
```

Modules are placed in the three layout lists:

```toml
[bar]
position = "top" # top, bottom, left, or right
height = 28       # bar thickness in logical pixels

[modules]
start = ["menu", "workspaces", "focused"]
center = ["media"]
end = ["tray", "control_center", "hardware", "volume", "clock"]
```

Bearbar uses bundled SVG icons when an icon setting is omitted. A configured
glyph or text icon takes precedence, so Nerd Font and SF Symbol setups remain
supported. [`examples/config.apple.toml`](examples/config.apple.toml) contains
the optional Apple-style glyph configuration.

## Modules

| Module | Purpose | Extra dependency |
|---|---|---|
| `workspaces` | Hyprland, Niri, Sway, or KDE Plasma workspaces | — |
| `focused` | Focused application and title | — |
| `launcher` | Search and focus open windows | — |
| `menu` | Search installed desktop applications | — |
| `tray` | StatusNotifier items and menus | — |
| `clock` | Local time and calendar | — |
| `volume` | Output level, mute, scroll, and popup | PulseAudio/PipeWire Pulse |
| `battery` | Charge, health, remaining time, and power mode | UPower; power profiles optional |
| `hardware` | Battery plus hover drawer for CPU, memory, and temperature | Linux `/proc` and `/sys` |
| `control_center` | Wi-Fi, Bluetooth, brightness, sound, and idle inhibition | Integration commands above |
| `media` | Current track and playback controls | `playerctl` |
| `privacy` | Active microphone, camera, or screen capture | `pw-dump` |
| `notifications` | SwayNC count and panel toggle | `swaync-client` |
| `clipboard` | Search and restore clipboard history | `cliphist`, `wl-copy` |
| `keyboard` | Current compositor keyboard layout | — |
| `submap` | Current Hyprland submap | — |
| `inhibit` | Standalone idle inhibitor toggle | `systemd-inhibit` |
| `custom:name` | Periodic command output with an optional click action | User command |

Niri workspaces follow Niri's dynamic workspace model. KDE virtual desktops are
global rather than per-output. The `persistent` list is used by Hyprland and
Sway, and ignored by Niri and KDE.

The compositor is detected from its session environment. Set
`BEARBAR_COMPOSITOR` to `hyprland`, `niri`, `sway`, or `kde` to override
auto-detection.

Left and right bars use compact summaries: workspaces and tray items stack,
metric percentages collapse to icons, and the clock uses a two-line time.

## Custom modules

```toml
[custom.weather]
command = "weather-script"
interval_secs = 900
on_click = "xdg-open https://example.com/weather"
```

Use `custom:weather` in a layout list to display it.

Custom module commands and click actions run through the user's shell. Treat
configuration files as executable code and only use commands you trust.

## License

[MIT License](LICENSE)
