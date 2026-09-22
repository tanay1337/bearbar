use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::config::VolumeConfig;
use crate::services::volume::{VolumeService, VolumeSnapshot};

pub fn build(
    mut config: VolumeConfig,
    service: VolumeService,
    orientation: gtk::Orientation,
) -> gtk::Widget {
    if orientation == gtk::Orientation::Vertical {
        config.show_percentage = false;
    }
    let button = gtk::MenuButton::new();
    button.set_widget_name("volume");
    button.add_css_class("module");
    button.add_css_class("volume");
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

    let popover_contents = gtk::Box::new(gtk::Orientation::Vertical, 9);
    popover_contents.add_css_class("popover-contents");
    popover_contents.add_css_class("control-center");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("panel-header");
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.set_hexpand(true);
    let kicker = gtk::Label::new(Some("OUTPUT"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    let sink = gtk::Label::new(None);
    sink.set_xalign(0.0);
    sink.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sink.set_max_width_chars(18);
    sink.add_css_class("panel-title");
    heading.append(&kicker);
    heading.append(&sink);
    let popup_percentage = gtk::Label::new(None);
    popup_percentage.add_css_class("panel-value");
    header.append(&heading);
    header.append(&popup_percentage);

    let control_card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    control_card.add_css_class("control-card");
    let mute = gtk::ToggleButton::new();
    mute.add_css_class("mute-control");
    let mute_label = gtk::Label::new(Some("MUTE"));
    mute.set_child(Some(&mute_label));
    mute.set_tooltip_text(Some("Mute output"));
    let adjustment = gtk::Adjustment::new(0.0, 0.0, config.max_volume, 0.01, 0.1, 0.0);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_size_request(170, -1);
    control_card.append(&scale);
    control_card.append(&mute);
    popover_contents.append(&header);
    popover_contents.append(&control_card);
    let popover = gtk::Popover::new();
    popover.add_css_class("volume-popover");
    popover.set_child(Some(&popover_contents));
    button.set_popover(Some(&popover));

    let applying_state = Rc::new(Cell::new(false));
    {
        let service = service.clone();
        let applying_state = applying_state.clone();
        scale.connect_value_changed(move |scale| {
            if !applying_state.get() {
                service.set_volume(scale.value());
            }
        });
    }
    {
        let service = service.clone();
        let applying_state = applying_state.clone();
        mute.connect_toggled(move |_| {
            if !applying_state.get() {
                service.toggle_mute();
            }
        });
    }

    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let scroll_service = service.clone();
    let scroll_state = service.subscribe();
    let step = config.scroll_step;
    let max_volume = config.max_volume;
    scroll.connect_scroll(move |_, _, delta_y| {
        if delta_y.abs() >= 0.01 {
            let current = scroll_state.borrow().volume;
            let target = if delta_y < 0.0 {
                current + step
            } else {
                current - step
            };
            scroll_service.set_volume(target.clamp(0.0, max_volume));
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    button.add_controller(scroll);

    let mut receiver = service.subscribe();
    render(
        &button,
        &icon,
        &glyph,
        &label,
        &scale,
        &mute,
        &mute_label,
        &sink,
        &popup_percentage,
        &receiver.borrow().clone(),
        &config,
        &applying_state,
    );

    let weak_button = button.downgrade();
    let weak_icon = icon.downgrade();
    let weak_glyph = glyph.downgrade();
    let weak_label = label.downgrade();
    let weak_scale = scale.downgrade();
    let weak_mute = mute.downgrade();
    let weak_mute_label = mute_label.downgrade();
    let weak_sink = sink.downgrade();
    let weak_popup_percentage = popup_percentage.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let (
                Some(button),
                Some(icon),
                Some(glyph),
                Some(label),
                Some(scale),
                Some(mute),
                Some(mute_label),
                Some(sink),
                Some(popup_percentage),
            ) = (
                weak_button.upgrade(),
                weak_icon.upgrade(),
                weak_glyph.upgrade(),
                weak_label.upgrade(),
                weak_scale.upgrade(),
                weak_mute.upgrade(),
                weak_mute_label.upgrade(),
                weak_sink.upgrade(),
                weak_popup_percentage.upgrade(),
            )
            else {
                break;
            };
            render(
                &button,
                &icon,
                &glyph,
                &label,
                &scale,
                &mute,
                &mute_label,
                &sink,
                &popup_percentage,
                &receiver.borrow().clone(),
                &config,
                &applying_state,
            );
        }
    });

    button.upcast()
}

#[allow(clippy::too_many_arguments)]
fn render(
    button: &gtk::MenuButton,
    icon: &gtk::Image,
    glyph: &gtk::Label,
    label: &gtk::Label,
    scale: &gtk::Scale,
    mute: &gtk::ToggleButton,
    mute_label: &gtk::Label,
    sink: &gtk::Label,
    popup_percentage: &gtk::Label,
    snapshot: &VolumeSnapshot,
    config: &VolumeConfig,
    applying_state: &Cell<bool>,
) {
    applying_state.set(true);
    for class in ["muted", "unavailable"] {
        button.remove_css_class(class);
    }
    if !snapshot.connected {
        button.add_css_class("unavailable");
    }
    if snapshot.muted {
        button.add_css_class("muted");
    }

    if let Some(custom_icon) = custom_volume_icon(snapshot, config) {
        glyph.set_label(&custom_icon);
        glyph.set_visible(true);
        icon.set_visible(false);
    } else {
        let icon_name = volume_icon(snapshot);
        icon.set_icon_name(Some(icon_name));
        icon.set_visible(true);
        glyph.set_visible(false);
    }
    let percentage = (snapshot.volume * 100.0).round() as i32;
    label.set_visible(config.show_percentage);
    label.set_label(&format!("{percentage}%"));
    scale.set_value(snapshot.volume.min(config.max_volume));
    mute.set_active(snapshot.muted);
    mute_label.set_label(if snapshot.muted { "UNMUTE" } else { "MUTE" });
    scale.set_sensitive(snapshot.connected);
    mute.set_sensitive(snapshot.connected);
    popup_percentage.set_label(&format!("{percentage}%"));
    sink.set_label(if snapshot.description.is_empty() {
        "Audio service unavailable"
    } else {
        &snapshot.description
    });
    button.set_tooltip_text(Some(&format!(
        "{} · {percentage}%",
        if snapshot.description.is_empty() {
            "Volume"
        } else {
            &snapshot.description
        }
    )));
    applying_state.set(false);
}

fn custom_volume_icon(snapshot: &VolumeSnapshot, config: &VolumeConfig) -> Option<String> {
    let icon = if snapshot.muted || snapshot.volume <= 0.001 {
        config.muted_icon.as_deref()
    } else if config.icons.len() == 3 {
        let index = if snapshot.volume < 0.34 {
            0
        } else if snapshot.volume < 0.67 {
            1
        } else {
            2
        };
        config.icons.get(index).map(String::as_str)
    } else {
        None
    }?;

    let bluetooth = snapshot
        .bluetooth
        .then_some(config.bluetooth_icon.as_deref())
        .flatten()
        .unwrap_or_default();
    Some(format!("{icon}{bluetooth}"))
}

fn volume_icon(snapshot: &VolumeSnapshot) -> &'static str {
    if snapshot.muted || snapshot.volume <= 0.001 {
        "bearbar-volume-muted-symbolic"
    } else if snapshot.volume < 0.34 {
        "bearbar-volume-low-symbolic"
    } else if snapshot.volume < 0.67 {
        "bearbar-volume-medium-symbolic"
    } else {
        "bearbar-volume-high-symbolic"
    }
}
