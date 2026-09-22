use gtk::glib;
use gtk::prelude::*;

use crate::config::BatteryConfig;
use crate::services::battery::{BatteryService, BatterySnapshot, ChargeState};
use crate::services::desktop::{DesktopCommand, DesktopService};

pub fn build(
    mut config: BatteryConfig,
    service: BatteryService,
    desktop: DesktopService,
    orientation: gtk::Orientation,
) -> gtk::Widget {
    if orientation == gtk::Orientation::Vertical {
        config.show_percentage = false;
    }
    let button = gtk::MenuButton::new();
    button.set_widget_name("battery");
    button.add_css_class("module");
    button.add_css_class("battery");
    button.set_focusable(false);

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    let icon = gtk::Image::new();
    icon.set_pixel_size(16);
    let glyph = gtk::Label::new(None);
    glyph.add_css_class("module-icon");
    let label = gtk::Label::new(None);
    summary.append(&icon);
    summary.append(&glyph);
    summary.append(&label);
    button.set_child(Some(&summary));

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 9);
    contents.add_css_class("popover-contents");
    contents.add_css_class("power-panel");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("panel-header");
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.set_hexpand(true);
    let kicker = gtk::Label::new(Some("POWER"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    let state = gtk::Label::new(None);
    state.set_xalign(0.0);
    state.add_css_class("panel-title");
    heading.append(&kicker);
    heading.append(&state);
    let popup_percentage = gtk::Label::new(None);
    popup_percentage.add_css_class("panel-value");
    header.append(&heading);
    header.append(&popup_percentage);
    let charge = gtk::ProgressBar::new();
    charge.add_css_class("charge-meter");
    let stats = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    stats.add_css_class("power-stats");
    let remaining = gtk::Label::new(None);
    remaining.set_xalign(0.0);
    remaining.set_hexpand(true);
    let health = gtk::Label::new(None);
    health.set_xalign(1.0);
    stats.append(&remaining);
    stats.append(&health);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("control-card");
    card.append(&charge);
    card.append(&stats);
    contents.append(&header);
    contents.append(&card);

    let profile_section = gtk::Box::new(gtk::Orientation::Vertical, 5);
    let profile_label = gtk::Label::new(Some("POWER MODE"));
    profile_label.set_xalign(0.0);
    profile_label.add_css_class("control-caption");
    let profiles = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    profiles.set_homogeneous(true);
    profiles.add_css_class("profile-group");
    let saver = profile_button("LOW", "power-saver", &desktop);
    let balanced = profile_button("AUTO", "balanced", &desktop);
    let performance = profile_button("HIGH", "performance", &desktop);
    profiles.append(&saver);
    profiles.append(&balanced);
    profiles.append(&performance);
    profile_section.append(&profile_label);
    profile_section.append(&profiles);
    contents.append(&profile_section);
    let popover = gtk::Popover::new();
    popover.add_css_class("battery-popover");
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));

    let mut receiver = service.subscribe();
    render(
        &button,
        &icon,
        &glyph,
        &label,
        &state,
        &popup_percentage,
        &charge,
        &remaining,
        &health,
        &receiver.borrow().clone(),
        &config,
    );

    let weak_button = button.downgrade();
    let weak_icon = icon.downgrade();
    let weak_glyph = glyph.downgrade();
    let weak_label = label.downgrade();
    let weak_state = state.downgrade();
    let weak_popup_percentage = popup_percentage.downgrade();
    let weak_charge = charge.downgrade();
    let weak_remaining = remaining.downgrade();
    let weak_health = health.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let (
                Some(button),
                Some(icon),
                Some(glyph),
                Some(label),
                Some(state),
                Some(popup_percentage),
                Some(charge),
                Some(remaining),
                Some(health),
            ) = (
                weak_button.upgrade(),
                weak_icon.upgrade(),
                weak_glyph.upgrade(),
                weak_label.upgrade(),
                weak_state.upgrade(),
                weak_popup_percentage.upgrade(),
                weak_charge.upgrade(),
                weak_remaining.upgrade(),
                weak_health.upgrade(),
            )
            else {
                break;
            };
            render(
                &button,
                &icon,
                &glyph,
                &label,
                &state,
                &popup_percentage,
                &charge,
                &remaining,
                &health,
                &receiver.borrow().clone(),
                &config,
            );
        }
    });

    let mut desktop_receiver = desktop.subscribe();
    render_profiles(
        [&saver, &balanced, &performance],
        &desktop_receiver.borrow().power_profile,
    );
    let weak_saver = saver.downgrade();
    let weak_balanced = balanced.downgrade();
    let weak_performance = performance.downgrade();
    glib::spawn_future_local(async move {
        while desktop_receiver.changed().await.is_ok() {
            let (Some(saver), Some(balanced), Some(performance)) = (
                weak_saver.upgrade(),
                weak_balanced.upgrade(),
                weak_performance.upgrade(),
            ) else {
                break;
            };
            render_profiles(
                [&saver, &balanced, &performance],
                &desktop_receiver.borrow().power_profile,
            );
        }
    });

    button.upcast()
}

fn profile_button(label: &str, profile: &str, service: &DesktopService) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::with_label(label);
    button.add_css_class("profile-control");
    button.set_focusable(false);
    let service = service.clone();
    let profile = profile.to_owned();
    button.connect_clicked(move |_| service.send(DesktopCommand::SetPowerProfile(profile.clone())));
    button
}

fn render_profiles(buttons: [&gtk::ToggleButton; 3], active: &str) {
    for (button, name) in buttons
        .into_iter()
        .zip(["power-saver", "balanced", "performance"])
    {
        button.set_active(active == name);
        button.set_sensitive(!active.is_empty());
    }
}

#[allow(clippy::too_many_arguments)]
fn render(
    button: &gtk::MenuButton,
    icon: &gtk::Image,
    glyph: &gtk::Label,
    label: &gtk::Label,
    state: &gtk::Label,
    popup_percentage: &gtk::Label,
    charge: &gtk::ProgressBar,
    remaining: &gtk::Label,
    health: &gtk::Label,
    snapshot: &BatterySnapshot,
    config: &BatteryConfig,
) {
    let should_show = snapshot.present || !config.hide_when_absent;
    button.set_visible(should_show);
    if !should_show {
        return;
    }

    for class in ["charging", "warning", "critical", "unavailable"] {
        button.remove_css_class(class);
    }
    if !snapshot.connected {
        button.add_css_class("unavailable");
    }
    if matches!(
        snapshot.state,
        ChargeState::Charging | ChargeState::PendingCharge
    ) {
        button.add_css_class("charging");
    }

    let percentage = snapshot.percentage.clamp(0.0, 100.0).round() as u8;
    if percentage <= config.critical {
        button.add_css_class("critical");
    } else if percentage <= config.warning {
        button.add_css_class("warning");
    }

    if let Some(custom_icon) = battery_custom_icon(percentage, snapshot.state, config) {
        glyph.set_label(custom_icon);
        glyph.set_visible(true);
        icon.set_visible(false);
    } else {
        icon.set_icon_name(Some(battery_icon(percentage, snapshot.state)));
        icon.set_visible(true);
        glyph.set_visible(false);
    }
    label.set_visible(config.show_percentage);
    label.set_label(&format!("{percentage}%"));
    button.set_tooltip_text(Some(&battery_tooltip(snapshot, percentage)));

    state.set_label(battery_state_label(snapshot.state));
    popup_percentage.set_label(&format!("{percentage}%"));
    charge.set_fraction(f64::from(percentage) / 100.0);
    remaining.set_label(&remaining_label(snapshot));
    let health_text = if snapshot.capacity > 0.0 {
        format!("HEALTH {:.0}%", snapshot.capacity)
    } else {
        "HEALTH --".into()
    };
    health.set_label(&health_text);
}

fn battery_state_label(state: ChargeState) -> &'static str {
    match state {
        ChargeState::Charging | ChargeState::PendingCharge => "CHARGING",
        ChargeState::Discharging | ChargeState::PendingDischarge => "ON BATTERY",
        ChargeState::Empty => "EMPTY",
        ChargeState::Full => "CHARGED",
        ChargeState::Unknown => "POWER",
    }
}

fn remaining_label(snapshot: &BatterySnapshot) -> String {
    if snapshot.seconds_remaining <= 0 {
        return "TIME --".into();
    }
    let hours = snapshot.seconds_remaining / 3600;
    let minutes = (snapshot.seconds_remaining % 3600) / 60;
    if matches!(
        snapshot.state,
        ChargeState::Charging | ChargeState::PendingCharge
    ) {
        format!("FULL {hours:02}:{minutes:02}")
    } else {
        format!("LEFT {hours:02}:{minutes:02}")
    }
}

fn battery_custom_icon(percentage: u8, state: ChargeState, config: &BatteryConfig) -> Option<&str> {
    if matches!(state, ChargeState::Charging | ChargeState::PendingCharge)
        && let Some(icon) = config.charging_icon.as_deref()
    {
        return Some(icon);
    }
    if config.icons.len() == 5 {
        let index = usize::from(percentage.saturating_sub(1) / 20).min(4);
        return config.icons.get(index).map(String::as_str);
    }
    None
}

fn battery_icon(percentage: u8, state: ChargeState) -> &'static str {
    let charging = matches!(state, ChargeState::Charging | ChargeState::PendingCharge);
    match (percentage, charging) {
        (_, true) => "bearbar-battery-charging-symbolic",
        (0..=10, false) => "bearbar-battery-empty-symbolic",
        (11..=30, false) => "bearbar-battery-low-symbolic",
        (31..=50, false) => "bearbar-battery-half-symbolic",
        (51..=80, false) => "bearbar-battery-good-symbolic",
        (_, false) => "bearbar-battery-full-symbolic",
    }
}

fn battery_tooltip(snapshot: &BatterySnapshot, percentage: u8) -> String {
    let state = match snapshot.state {
        ChargeState::Charging | ChargeState::PendingCharge => "Charging",
        ChargeState::Discharging | ChargeState::PendingDischarge => "On battery",
        ChargeState::Empty => "Empty",
        ChargeState::Full => "Fully charged",
        ChargeState::Unknown => "Battery",
    };
    let remaining = if snapshot.seconds_remaining > 0 {
        let hours = snapshot.seconds_remaining / 3600;
        let minutes = (snapshot.seconds_remaining % 3600) / 60;
        format!(" · {hours}h {minutes:02}m remaining")
    } else {
        String::new()
    };
    let health = if snapshot.capacity > 0.0 {
        format!("\nBattery health: {:.0}%", snapshot.capacity)
    } else {
        String::new()
    };
    format!("{state} · {percentage}%{remaining}{health}")
}
