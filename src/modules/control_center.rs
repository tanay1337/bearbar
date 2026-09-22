use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::config::{ControlCenterConfig, VolumeConfig};
use crate::services::desktop::{DesktopCommand, DesktopService, DesktopSnapshot, WifiNetwork};
use crate::services::volume::{VolumeService, VolumeSnapshot};

pub fn build_inhibit(service: DesktopService) -> gtk::Widget {
    let button = gtk::ToggleButton::with_label("AWAKE");
    button.set_widget_name("inhibit");
    button.add_css_class("module");
    button.add_css_class("inhibit");
    button.set_focusable(false);
    let applying = Rc::new(Cell::new(false));
    let mut receiver = service.subscribe();
    button.set_active(receiver.borrow().inhibited);
    {
        let service = service.clone();
        let applying = applying.clone();
        button.connect_toggled(move |_| {
            if !applying.get() {
                service.send(DesktopCommand::ToggleInhibit);
            }
        });
    }
    let weak_button = button.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(button) = weak_button.upgrade() else {
                break;
            };
            applying.set(true);
            let active = receiver.borrow().inhibited;
            button.set_active(active);
            button.set_label(if active { "AWAKE ON" } else { "AWAKE" });
            applying.set(false);
        }
    });
    button.upcast()
}

pub fn build(
    config: ControlCenterConfig,
    volume_config: VolumeConfig,
    desktop: DesktopService,
    volume: VolumeService,
) -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.set_widget_name("control-center");
    button.add_css_class("module");
    button.add_css_class("control-center-button");
    button.set_focusable(false);
    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let summary_icon = crate::icons::image("bearbar-control-center-symbolic", 16);
    let summary_label = gtk::Label::new(None);
    summary.append(&summary_icon);
    summary.append(&summary_label);
    button.set_child(Some(&summary));

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 9);
    contents.add_css_class("popover-contents");
    contents.add_css_class("control-center-panel");
    let kicker = gtk::Label::new(Some("CONTROL CENTER"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    contents.append(&kicker);

    let wifi = connectivity_section("WI-FI", "bearbar-wifi-high-symbolic");
    let bluetooth = connectivity_section("BLUETOOTH", "bearbar-bluetooth-symbolic");
    wifi.revealer.connect_reveal_child_notify({
        let other = bluetooth.revealer.clone();
        move |revealer| {
            if revealer.reveals_child() {
                other.set_reveal_child(false);
            }
        }
    });
    bluetooth.revealer.connect_reveal_child_notify({
        let other = wifi.revealer.clone();
        move |revealer| {
            if revealer.reveals_child() {
                other.set_reveal_child(false);
            }
        }
    });
    contents.append(&wifi.container);
    contents.append(&bluetooth.container);

    let password = password_row();
    let selected_wifi = Rc::new(RefCell::new(None::<WifiNetwork>));
    {
        let selected = selected_wifi.clone();
        let entry = password.entry.clone();
        let service = desktop.clone();
        let submit = Rc::new(move || {
            let Some(network) = selected.borrow().clone() else {
                return;
            };
            let password = entry.text().to_string();
            if password.is_empty() {
                return;
            }
            service.send(DesktopCommand::ConnectWifi {
                ssid: network.ssid,
                password: Some(password),
                saved: false,
            });
            entry.set_text("");
        });
        password.connect.connect_clicked({
            let submit = submit.clone();
            move |_| submit()
        });
        password.entry.connect_activate(move |_| submit());
    }

    let sliders = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    sliders.set_homogeneous(true);
    let brightness = control_slider("DISPLAY", 1.0, 100.0);
    let volume_control = control_slider("SOUND", 0.0, volume_config.max_volume * 100.0);
    sliders.append(&brightness.container);
    sliders.append(&volume_control.container);
    contents.append(&sliders);

    let inhibit = gtk::ToggleButton::with_label("AWAKE");
    inhibit.add_css_class("wide-control");
    inhibit.set_focusable(false);
    inhibit.set_hexpand(true);
    contents.append(&inhibit);

    let popover = gtk::Popover::new();
    popover.add_css_class("control-center-popover");
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));
    button.connect_active_notify({
        let service = desktop.clone();
        let wifi_revealer = wifi.revealer.clone();
        let bluetooth_revealer = bluetooth.revealer.clone();
        move |button| {
            if button.is_active() {
                service.send(DesktopCommand::RefreshConnectivity);
            } else {
                wifi_revealer.set_reveal_child(false);
                bluetooth_revealer.set_reveal_child(false);
            }
        }
    });

    let applying_desktop = Rc::new(Cell::new(false));
    wifi.toggle.connect_active_notify({
        let service = desktop.clone();
        let applying = applying_desktop.clone();
        move |_| {
            if !applying.get() {
                service.send(DesktopCommand::ToggleWifi);
            }
        }
    });
    bluetooth.toggle.connect_active_notify({
        let service = desktop.clone();
        let applying = applying_desktop.clone();
        move |_| {
            if !applying.get() {
                service.send(DesktopCommand::ToggleBluetooth);
            }
        }
    });
    inhibit.connect_toggled({
        let service = desktop.clone();
        let applying = applying_desktop.clone();
        move |_| {
            if !applying.get() {
                service.send(DesktopCommand::ToggleInhibit);
            }
        }
    });
    let brightness_interacting = Rc::new(Cell::new(false));
    debounce_brightness(
        &brightness,
        &desktop,
        &applying_desktop,
        &brightness_interacting,
    );

    let applying_volume = Rc::new(Cell::new(false));
    volume_control.scale.connect_value_changed({
        let service = volume.clone();
        let applying = applying_volume.clone();
        move |scale| {
            if !applying.get() {
                service.set_volume((scale.value() / 100.0).clamp(0.0, volume_config.max_volume));
            }
        }
    });

    let mut desktop_receiver = desktop.subscribe();
    render_desktop(
        &summary_icon,
        &summary_label,
        &wifi,
        &bluetooth,
        &password,
        &selected_wifi,
        &brightness,
        &inhibit,
        &desktop_receiver.borrow(),
        &config,
        &desktop,
        &applying_desktop,
        &brightness_interacting,
    );
    let weak_summary_icon = summary_icon.downgrade();
    let weak_summary_label = summary_label.downgrade();
    let weak_wifi_toggle = wifi.toggle.downgrade();
    let weak_wifi_value = wifi.value.downgrade();
    let weak_wifi_details = wifi.details.downgrade();
    let weak_bluetooth_toggle = bluetooth.toggle.downgrade();
    let weak_bluetooth_value = bluetooth.value.downgrade();
    let weak_bluetooth_details = bluetooth.details.downgrade();
    let weak_password_container = password.container.downgrade();
    let weak_password_label = password.label.downgrade();
    let weak_password_entry = password.entry.downgrade();
    let weak_password_connect = password.connect.downgrade();
    let weak_brightness_scale = brightness.scale.downgrade();
    let weak_brightness_value = brightness.value.downgrade();
    let weak_inhibit = inhibit.downgrade();
    let desktop_for_render = desktop.clone();
    glib::spawn_future_local(async move {
        while desktop_receiver.changed().await.is_ok() {
            let (
                Some(summary_icon),
                Some(summary_label),
                Some(wifi_toggle),
                Some(wifi_value),
                Some(wifi_details),
                Some(bluetooth_toggle),
                Some(bluetooth_value),
                Some(bluetooth_details),
                Some(password_container),
                Some(password_label),
                Some(password_entry),
                Some(password_connect),
                Some(brightness_scale),
                Some(brightness_value),
                Some(inhibit),
            ) = (
                weak_summary_icon.upgrade(),
                weak_summary_label.upgrade(),
                weak_wifi_toggle.upgrade(),
                weak_wifi_value.upgrade(),
                weak_wifi_details.upgrade(),
                weak_bluetooth_toggle.upgrade(),
                weak_bluetooth_value.upgrade(),
                weak_bluetooth_details.upgrade(),
                weak_password_container.upgrade(),
                weak_password_label.upgrade(),
                weak_password_entry.upgrade(),
                weak_password_connect.upgrade(),
                weak_brightness_scale.upgrade(),
                weak_brightness_value.upgrade(),
                weak_inhibit.upgrade(),
            )
            else {
                break;
            };
            render_desktop(
                &summary_icon,
                &summary_label,
                &ConnectivityWidgets::render_only(wifi_toggle, wifi_value, wifi_details),
                &ConnectivityWidgets::render_only(
                    bluetooth_toggle,
                    bluetooth_value,
                    bluetooth_details,
                ),
                &PasswordWidgets {
                    container: password_container,
                    label: password_label,
                    entry: password_entry,
                    connect: password_connect,
                },
                &selected_wifi,
                &SliderWidgets::render_only(brightness_scale, brightness_value),
                &inhibit,
                &desktop_receiver.borrow(),
                &config,
                &desktop_for_render,
                &applying_desktop,
                &brightness_interacting,
            );
        }
    });

    let mut volume_receiver = volume.subscribe();
    render_volume(&volume_control, &volume_receiver.borrow(), &applying_volume);
    let weak_volume_scale = volume_control.scale.downgrade();
    let weak_volume_value = volume_control.value.downgrade();
    glib::spawn_future_local(async move {
        while volume_receiver.changed().await.is_ok() {
            let (Some(scale), Some(value)) =
                (weak_volume_scale.upgrade(), weak_volume_value.upgrade())
            else {
                break;
            };
            render_volume(
                &SliderWidgets::render_only(scale, value),
                &volume_receiver.borrow(),
                &applying_volume,
            );
        }
    });

    button.upcast()
}

struct ConnectivityWidgets {
    container: gtk::Box,
    toggle: gtk::ToggleButton,
    value: gtk::Label,
    details: gtk::Box,
    revealer: gtk::Revealer,
}

impl ConnectivityWidgets {
    fn render_only(toggle: gtk::ToggleButton, value: gtk::Label, details: gtk::Box) -> Self {
        Self {
            container: gtk::Box::new(gtk::Orientation::Vertical, 0),
            toggle,
            value,
            details,
            revealer: gtk::Revealer::new(),
        }
    }
}

fn connectivity_section(title: &str, icon_name: &str) -> ConnectivityWidgets {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
    container.add_css_class("connectivity-card");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let expand = gtk::Button::new();
    expand.add_css_class("connectivity-header");
    expand.set_focusable(false);
    expand.set_hexpand(true);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let caption_row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let section_icon = crate::icons::image(icon_name, 12);
    section_icon.add_css_class("connection-icon");
    let caption = gtk::Label::new(Some(title));
    caption.set_xalign(0.0);
    caption.add_css_class("control-caption");
    caption_row.append(&section_icon);
    caption_row.append(&caption);
    let value = gtk::Label::new(None);
    value.set_xalign(0.0);
    value.set_ellipsize(gtk::pango::EllipsizeMode::End);
    value.set_max_width_chars(24);
    value.add_css_class("tile-value");
    text.append(&caption_row);
    text.append(&value);
    expand.set_child(Some(&text));
    let toggle = gtk::ToggleButton::new();
    toggle.add_css_class("connectivity-switch");
    toggle.set_has_frame(false);
    toggle.set_focusable(false);
    toggle.set_valign(gtk::Align::Center);
    toggle.set_margin_start(6);
    toggle.set_margin_end(9);
    let knob = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    knob.add_css_class("connectivity-switch-knob");
    toggle.set_child(Some(&knob));
    header.append(&expand);
    header.append(&toggle);
    let details = gtk::Box::new(gtk::Orientation::Vertical, 2);
    details.add_css_class("connection-list");
    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_transition_duration(160);
    revealer.set_child(Some(&details));
    let reveal_control = revealer.clone();
    expand.connect_clicked(move |_| {
        reveal_control.set_reveal_child(!reveal_control.reveals_child());
    });
    container.append(&header);
    container.append(&revealer);
    ConnectivityWidgets {
        container,
        toggle,
        value,
        details,
        revealer,
    }
}

struct PasswordWidgets {
    container: gtk::Box,
    label: gtk::Label,
    entry: gtk::PasswordEntry,
    connect: gtk::Button,
}

fn password_row() -> PasswordWidgets {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 5);
    container.add_css_class("wifi-password");
    container.set_visible(false);
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.add_css_class("row-meta");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let entry = gtk::PasswordEntry::new();
    entry.set_hexpand(true);
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("Network password"));
    let connect = gtk::Button::with_label("JOIN");
    connect.add_css_class("connect-button");
    row.append(&entry);
    row.append(&connect);
    container.append(&label);
    container.append(&row);
    PasswordWidgets {
        container,
        label,
        entry,
        connect,
    }
}

struct SliderWidgets {
    container: gtk::Box,
    scale: gtk::Scale,
    value: gtk::Label,
}

impl SliderWidgets {
    fn render_only(scale: gtk::Scale, value: gtk::Label) -> Self {
        Self {
            container: gtk::Box::new(gtk::Orientation::Vertical, 0),
            scale,
            value,
        }
    }
}

fn control_slider(title: &str, minimum: f64, maximum: f64) -> SliderWidgets {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 5);
    container.add_css_class("control-card");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let caption = gtk::Label::new(Some(title));
    caption.set_xalign(0.0);
    caption.set_hexpand(true);
    caption.add_css_class("control-caption");
    let value = gtk::Label::new(None);
    value.add_css_class("control-reading");
    header.append(&caption);
    header.append(&value);
    let adjustment = gtk::Adjustment::new(minimum, minimum, maximum, 1.0, 5.0, 0.0);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_focusable(false);
    container.append(&header);
    container.append(&scale);
    SliderWidgets {
        container,
        scale,
        value,
    }
}

fn debounce_brightness(
    widgets: &SliderWidgets,
    service: &DesktopService,
    applying: &Rc<Cell<bool>>,
    interacting: &Rc<Cell<bool>>,
) {
    let pending = Rc::new(RefCell::new(None::<glib::SourceId>));
    let settling = Rc::new(RefCell::new(None::<glib::SourceId>));
    widgets.scale.connect_value_changed({
        let service = service.clone();
        let applying = applying.clone();
        let interacting = interacting.clone();
        let value = widgets.value.clone();
        move |scale| {
            if applying.get() {
                return;
            }
            interacting.set(true);
            value.set_label(&format!("{:.0}%", scale.value()));
            if let Some(source) = pending.borrow_mut().take() {
                source.remove();
            }
            if let Some(source) = settling.borrow_mut().take() {
                source.remove();
            }
            let weak_scale = scale.downgrade();
            let service = service.clone();
            let pending_for_timeout = pending.clone();
            let settling_for_timeout = settling.clone();
            let interacting_for_timeout = interacting.clone();
            *pending.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(140),
                move || {
                    pending_for_timeout.borrow_mut().take();
                    if let Some(scale) = weak_scale.upgrade() {
                        service.send(DesktopCommand::SetBrightness(scale.value().round() as u8));
                    }
                    let settling_after = settling_for_timeout.clone();
                    *settling_for_timeout.borrow_mut() = Some(glib::timeout_add_local_once(
                        Duration::from_millis(450),
                        move || {
                            settling_after.borrow_mut().take();
                            interacting_for_timeout.set(false);
                        },
                    ));
                },
            ));
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_desktop(
    summary_icon: &gtk::Image,
    summary_label: &gtk::Label,
    wifi: &ConnectivityWidgets,
    bluetooth: &ConnectivityWidgets,
    password: &PasswordWidgets,
    selected_wifi: &Rc<RefCell<Option<WifiNetwork>>>,
    brightness: &SliderWidgets,
    inhibit: &gtk::ToggleButton,
    snapshot: &DesktopSnapshot,
    config: &ControlCenterConfig,
    service: &DesktopService,
    applying: &Cell<bool>,
    brightness_interacting: &Cell<bool>,
) {
    applying.set(true);
    let network_name = config.show_network_name.then_some(&snapshot.network_name);
    let custom_icon = config.icon.as_deref().filter(|icon| !icon.is_empty());
    if let Some(text) = network_name.filter(|name| !name.is_empty()) {
        summary_label.set_label(text);
        summary_label.set_visible(true);
        summary_icon.set_visible(false);
    } else if let Some(icon) = custom_icon {
        summary_label.set_label(icon);
        summary_label.set_visible(true);
        summary_icon.set_visible(false);
    } else {
        summary_label.set_visible(false);
        summary_icon.set_visible(true);
    }
    wifi.toggle.set_active(snapshot.wifi_enabled);
    wifi.value.set_label(if !snapshot.wifi_enabled {
        "Off"
    } else if snapshot.network_name.is_empty() {
        "Not connected"
    } else {
        &snapshot.network_name
    });
    render_wifi_list(
        &wifi.details,
        password,
        selected_wifi,
        &snapshot.wifi_networks,
        service,
    );

    bluetooth.toggle.set_sensitive(snapshot.bluetooth_available);
    bluetooth.toggle.set_active(snapshot.bluetooth_powered);
    bluetooth.value.set_label(if !snapshot.bluetooth_available {
        "Unavailable"
    } else if !snapshot.bluetooth_powered {
        "Off"
    } else if let Some(device) = snapshot
        .bluetooth_devices
        .iter()
        .find(|device| device.connected)
    {
        &device.name
    } else {
        "Not connected"
    });
    render_bluetooth_list(&bluetooth.details, snapshot, service);

    if let Some(value) = snapshot.brightness {
        brightness.scale.set_sensitive(true);
        if !brightness_interacting.get() {
            brightness.scale.set_value(f64::from(value));
            brightness.value.set_label(&format!("{value}%"));
        }
    } else {
        brightness.scale.set_sensitive(false);
        brightness.value.set_label("--");
    }
    inhibit.set_active(snapshot.inhibited);
    inhibit.set_label(if snapshot.inhibited {
        "AWAKE · ON"
    } else {
        "AWAKE"
    });
    applying.set(false);
}

fn render_wifi_list(
    list: &gtk::Box,
    password: &PasswordWidgets,
    selected: &Rc<RefCell<Option<WifiNetwork>>>,
    networks: &[WifiNetwork],
    service: &DesktopService,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if networks.is_empty() {
        list.append(&connection_empty("No networks found"));
    }
    for network in networks.iter().take(7) {
        let secured = !network.security.is_empty() && network.security != "--";
        let indicators = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        indicators.add_css_class("connection-indicators");
        if secured {
            indicators.append(&crate::icons::image("bearbar-lock-symbolic", 11));
        }
        indicators.append(&crate::icons::image(wifi_icon(network.strength), 14));
        if network.active {
            indicators.append(&crate::icons::image("bearbar-check-symbolic", 12));
        }
        let security = if secured {
            network.security.as_str()
        } else {
            "Open"
        };
        let tooltip = if network.active {
            format!("Connected · {}% · {security}", network.strength)
        } else {
            format!("{}% · {security}", network.strength)
        };
        let row = connection_row(&network.ssid, &indicators, network.active, &tooltip);
        let network = network.clone();
        let selected = selected.clone();
        let password_container = password.container.clone();
        let password_label = password.label.clone();
        let password_entry = password.entry.clone();
        let service = service.clone();
        row.connect_clicked(move |_| {
            if network.active {
                return;
            }
            let secured = !network.security.is_empty() && network.security != "--";
            if secured && !network.saved {
                password_label.set_label(&format!("JOIN {}", network.ssid));
                password_container.set_visible(true);
                password_entry.grab_focus();
                selected.replace(Some(network.clone()));
            } else {
                password_container.set_visible(false);
                service.send(DesktopCommand::ConnectWifi {
                    ssid: network.ssid.clone(),
                    password: None,
                    saved: network.saved,
                });
            }
        });
        list.append(&row);
    }
    list.append(&password.container);
}

fn render_bluetooth_list(list: &gtk::Box, snapshot: &DesktopSnapshot, service: &DesktopService) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if snapshot.bluetooth_devices.is_empty() {
        list.append(&connection_empty(if snapshot.bluetooth_powered {
            "No devices found · scanning"
        } else {
            "Bluetooth is off"
        }));
    }
    for device in snapshot.bluetooth_devices.iter().take(7) {
        let indicators = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        indicators.add_css_class("connection-indicators");
        if let Some(battery) = device.battery {
            indicators.append(&crate::icons::image(battery_level_icon(battery), 13));
            let battery_label = gtk::Label::new(Some(&format!("{battery}%")));
            battery_label.add_css_class("connection-battery");
            indicators.append(&battery_label);
        }
        let status = if device.connected {
            indicators.append(&crate::icons::image("bearbar-check-symbolic", 12));
            "Connected"
        } else if device.paired {
            indicators.append(&crate::icons::image("bearbar-paired-symbolic", 13));
            "Paired"
        } else {
            indicators.append(&crate::icons::image("bearbar-bluetooth-symbolic", 13));
            "Available"
        };
        let tooltip = device.battery.map_or_else(
            || status.to_owned(),
            |battery| format!("{status} · {battery}%"),
        );
        let row = connection_row(&device.name, &indicators, device.connected, &tooltip);
        let command = DesktopCommand::ToggleBluetoothDevice {
            address: device.address.clone(),
            connected: device.connected,
            paired: device.paired,
        };
        let service = service.clone();
        row.connect_clicked(move |_| service.send(command.clone()));
        list.append(&row);
    }
}

fn connection_empty(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("connection-empty");
    label
}

fn connection_row(name: &str, indicators: &gtk::Box, active: bool, tooltip: &str) -> gtk::Button {
    let row = gtk::Button::new();
    row.add_css_class("connection-row");
    row.set_focusable(false);
    if active {
        row.add_css_class("active");
    }
    row.set_tooltip_text(Some(tooltip));
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let name = gtk::Label::new(Some(name));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.set_max_width_chars(24);
    content.append(&name);
    content.append(indicators);
    row.set_child(Some(&content));
    row
}

fn wifi_icon(strength: u8) -> &'static str {
    match strength {
        0 => "bearbar-wifi-none-symbolic",
        1..=39 => "bearbar-wifi-low-symbolic",
        40..=69 => "bearbar-wifi-medium-symbolic",
        _ => "bearbar-wifi-high-symbolic",
    }
}

fn battery_level_icon(level: u8) -> &'static str {
    match level {
        0..=10 => "bearbar-battery-empty-symbolic",
        11..=30 => "bearbar-battery-low-symbolic",
        31..=50 => "bearbar-battery-half-symbolic",
        51..=80 => "bearbar-battery-good-symbolic",
        _ => "bearbar-battery-full-symbolic",
    }
}

fn render_volume(widgets: &SliderWidgets, snapshot: &VolumeSnapshot, applying: &Cell<bool>) {
    applying.set(true);
    let percent = (snapshot.volume * 100.0).round();
    widgets.scale.set_sensitive(snapshot.connected);
    widgets.scale.set_value(percent);
    let text = if snapshot.muted {
        "MUTED".into()
    } else {
        format!("{percent:.0}%")
    };
    widgets.value.set_label(&text);
    applying.set(false);
}
