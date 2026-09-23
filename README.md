<p align="center">
  <img src="assets/bearbar-logo.svg" width="100" alt="Bearbar">
</p>

<h1 align="center">Bearbar</h1>

A compact, customizable GTK4 bar for Hyprland, Niri, Sway, and KDE Plasma.
Ships with native compositor integration, bundled icons, polished popups, and
live configuration reload.

[Installation](#installation) · [Configuration](#configuration) ·
[Modules](#modules) · [Starting Bearbar](#starting-bearbar)

## Screenshots

<table>
<tr>
<td width="50%">

**Control Center**

![Bearbar control center](https://i.imgur.com/Mn0HQT1.png)

</td>
<td width="50%">

**Battery**

![Bearbar battery popup](https://i.imgur.com/bIM6WhA.png)

</td>
</tr>
<tr>
<td width="50%">

**Volume**

![Bearbar volume popup](https://i.imgur.com/6cSIhmA.png)

</td>
<td width="50%">

**Calendar**

![Bearbar calendar popup](https://i.imgur.com/6rcodRx.png)

</td>
</tr>
</table>

## Features

- Native workspace integration for Hyprland, Niri, Sway, and KDE Plasma
- Top, bottom, left, and right screen positions
- Built-in control center, calendar, volume, battery, and hardware popups
- StatusNotifier system tray and application launcher
- Bundled symbolic SVG icons with optional custom glyphs
- Live TOML and CSS reload without restarting the bar

### Compositor support

| Compositor | Status |
|---|---|
| Hyprland | Primary |
| Niri | Supported |
| Sway | Supported |
| KDE Plasma | Supported |

Hyprland is the most thoroughly tested integration. Reports and patches for the
newer Niri, Sway, and KDE Plasma integrations are welcome.

## Installation

### Arch Linux

Install the [AUR package](https://aur.archlinux.org/packages/bearbar-git):

```sh
yay -S bearbar-git
```

### Building from source

Bearbar requires Rust 1.92+, GTK 4.12+, GTK4 Layer Shell 1+, GLib development
tools, PulseAudio client libraries, and UPower.

```sh
git clone https://github.com/tanay1337/bearbar.git
cd bearbar
cargo build --release --locked
install -Dm755 target/release/bearbar ~/.local/bin/bearbar
```

Ubuntu 24.04 does not package the required GTK4 Layer Shell development files;
build [GTK4 Layer Shell](https://github.com/wmww/gtk4-layer-shell) 1.x from
source first.

## Configuration

Bearbar works with its built-in defaults. To customize it, create:

- `~/.config/bearbar/config.toml` for layout and module options
- `~/.config/bearbar/style.css` for appearance

Example files live in [`examples/`](examples). AUR users can also find them in
`/usr/share/doc/bearbar-git/examples/`. Both files reload automatically when
saved; invalid TOML leaves the active layout untouched.

```toml
[bar]
position = "top" # top, bottom, left, or right
height = 28

[modules]
start = ["menu", "workspaces", "focused"]
center = ["media"]
end = ["tray", "control_center", "hardware", "volume", "clock"]
```

Bundled SVG icons are used by default. Configured text or glyph icons take
precedence; [`config.apple.toml`](examples/config.apple.toml) demonstrates the
optional Apple-style setup.

```sh
bearbar --check-config
bearbar --config /path/to/config.toml --style /path/to/style.css
```

## Modules

- **Compositor** — `workspaces`, `focused`, `keyboard`, `submap`
- **Desktop** — `menu`, `launcher`, `tray`, `clock`, `control_center`
- **System** — `battery`, `hardware`, `volume`, `privacy`, `inhibit`
- **Workflow** — `media`, `notifications`, `clipboard`, `custom:name`

Optional integrations use standard desktop tools when installed:

| Integration | Dependency |
|---|---|
| Wi-Fi | NetworkManager (`nmcli`) |
| Bluetooth | BlueZ (`bluetoothctl`) |
| Brightness | `brightnessctl` |
| Power modes | `power-profiles-daemon` |
| Media | `playerctl` |
| Privacy indicators | PipeWire (`pw-dump`) |
| Notifications | SwayNC (`swaync-client`) |
| Clipboard | `cliphist` and `wl-clipboard` |

Custom modules run periodic shell commands and can define a click action:

```toml
[custom.weather]
command = "weather-script"
interval_secs = 900
on_click = "xdg-open https://example.com/weather"
```

Use `custom:weather` in a module list. Custom commands are executable code, so
only use configurations you trust.

## Starting Bearbar

| Compositor | Configuration |
|---|---|
| Hyprland | `exec-once = bearbar` |
| Niri | `spawn-at-startup "bearbar"` |
| Sway | `exec bearbar` |
| KDE Plasma | Add `bearbar` in **System Settings → Autostart** |

The AUR package also installs a user service:

```sh
systemctl --user enable --now bearbar.service
```

Choose one startup method and stop any other panel reserving the same screen
edge. Set `BEARBAR_COMPOSITOR` to `hyprland`, `niri`, `sway`, or `kde` to
override automatic compositor detection.

## License

[MIT License](LICENSE)
