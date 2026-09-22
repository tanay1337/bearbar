use std::time::Duration;

use chrono::Local;
use gtk::glib;
use gtk::prelude::*;

use crate::config::ClockConfig;

pub fn build(config: ClockConfig, orientation: gtk::Orientation) -> gtk::Widget {
    let vertical = orientation == gtk::Orientation::Vertical;
    let label = gtk::Label::new(None);
    label.add_css_class("clock-label");
    if vertical {
        label.add_css_class("vertical-clock");
        label.set_justify(gtk::Justification::Center);
    }
    update_label(&label, &config, vertical);

    if config.calendar {
        let button = gtk::MenuButton::new();
        button.set_widget_name("clock");
        button.add_css_class("module");
        button.add_css_class("clock");
        button.set_focusable(false);
        button.set_child(Some(&label));

        let calendar = gtk::Calendar::new();
        calendar.add_css_class("calendar");
        let contents = gtk::Box::new(gtk::Orientation::Vertical, 0);
        contents.add_css_class("popover-contents");
        contents.add_css_class("calendar-panel");
        contents.append(&calendar);
        let popover = gtk::Popover::new();
        popover.add_css_class("clock-popover");
        popover.set_child(Some(&contents));
        button.set_popover(Some(&popover));

        schedule_updates(&label, config, vertical);
        button.upcast()
    } else {
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        container.set_widget_name("clock");
        container.add_css_class("module");
        container.add_css_class("clock");
        container.append(&label);
        schedule_updates(&label, config, vertical);
        container.upcast()
    }
}

fn schedule_updates(label: &gtk::Label, config: ClockConfig, vertical: bool) {
    let weak_label = label.downgrade();
    let has_seconds = config.format.contains("%S") || config.tooltip_format.contains("%S");
    let interval = if has_seconds { 1 } else { 30 };

    glib::timeout_add_local(Duration::from_secs(interval), move || {
        let Some(label) = weak_label.upgrade() else {
            return glib::ControlFlow::Break;
        };
        update_label(&label, &config, vertical);
        glib::ControlFlow::Continue
    });
}

fn update_label(label: &gtk::Label, config: &ClockConfig, vertical: bool) {
    let now = Local::now();
    let display = if vertical {
        let format = if config.format.contains("%I") {
            "%I\n%M"
        } else {
            "%H\n%M"
        };
        now.format(format).to_string()
    } else {
        now.format(&config.format).to_string()
    };
    label.set_label(&display);
    label.set_tooltip_text(Some(&now.format(&config.tooltip_format).to_string()));
}
