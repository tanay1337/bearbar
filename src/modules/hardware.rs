use std::cell::Cell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use crate::config::{BatteryConfig, HardwareConfig};
use crate::services::battery::BatteryService;
use crate::services::desktop::DesktopService;
use crate::services::system::{SystemService, SystemSnapshot};

#[derive(Clone)]
struct Metric {
    button: gtk::MenuButton,
    summary_icon: gtk::Image,
    summary_glyph: gtk::Label,
    summary_value: gtk::Label,
    value: gtk::Label,
    meter: gtk::ProgressBar,
    primary_detail: gtk::Label,
    secondary_detail: gtk::Label,
}

struct WeakMetric {
    button: glib::WeakRef<gtk::MenuButton>,
    summary_icon: glib::WeakRef<gtk::Image>,
    summary_glyph: glib::WeakRef<gtk::Label>,
    summary_value: glib::WeakRef<gtk::Label>,
    value: glib::WeakRef<gtk::Label>,
    meter: glib::WeakRef<gtk::ProgressBar>,
    primary_detail: glib::WeakRef<gtk::Label>,
    secondary_detail: glib::WeakRef<gtk::Label>,
}

impl Metric {
    fn downgrade(&self) -> WeakMetric {
        WeakMetric {
            button: self.button.downgrade(),
            summary_icon: self.summary_icon.downgrade(),
            summary_glyph: self.summary_glyph.downgrade(),
            summary_value: self.summary_value.downgrade(),
            value: self.value.downgrade(),
            meter: self.meter.downgrade(),
            primary_detail: self.primary_detail.downgrade(),
            secondary_detail: self.secondary_detail.downgrade(),
        }
    }
}

impl WeakMetric {
    fn upgrade(&self) -> Option<Metric> {
        Some(Metric {
            button: self.button.upgrade()?,
            summary_icon: self.summary_icon.upgrade()?,
            summary_glyph: self.summary_glyph.upgrade()?,
            summary_value: self.summary_value.upgrade()?,
            value: self.value.upgrade()?,
            meter: self.meter.upgrade()?,
            primary_detail: self.primary_detail.upgrade()?,
            secondary_detail: self.secondary_detail.upgrade()?,
        })
    }
}

pub fn build(
    config: HardwareConfig,
    battery_config: BatteryConfig,
    battery_service: BatteryService,
    desktop_service: DesktopService,
    system_service: SystemService,
    orientation: gtk::Orientation,
) -> gtk::Widget {
    let group = gtk::Box::new(orientation, 0);
    group.set_widget_name("hardware");
    group.add_css_class("hardware-drawer");

    let details = gtk::Box::new(orientation, 0);
    details.add_css_class("hardware-details");
    let temperature = metric(
        "temperature",
        "THERMAL",
        "CPU package",
        "bearbar-temperature-symbolic",
        orientation,
    );
    let memory = metric(
        "memory",
        "MEMORY",
        "Physical memory",
        "bearbar-memory-symbolic",
        orientation,
    );
    let cpu = metric(
        "cpu",
        "PROCESSOR",
        "System load",
        "bearbar-cpu-symbolic",
        orientation,
    );
    details.append(&temperature.button);
    details.append(&memory.button);
    details.append(&cpu.button);

    let revealer = gtk::Revealer::new();
    revealer.set_transition_type(if orientation == gtk::Orientation::Vertical {
        gtk::RevealerTransitionType::SlideUp
    } else {
        gtk::RevealerTransitionType::SlideLeft
    });
    revealer.set_transition_duration(config.transition_ms);
    revealer.set_reveal_child(config.enabled && !config.reveal_on_hover);
    revealer.set_child(Some(&details));
    revealer.set_visible(config.enabled);
    group.append(&revealer);
    group.append(&crate::modules::battery::build(
        battery_config,
        battery_service,
        desktop_service,
        orientation,
    ));

    if config.enabled && config.reveal_on_hover {
        configure_hover_drawer(
            &group,
            &revealer,
            [&temperature.button, &memory.button, &cpu.button],
        );
    }

    let mut receiver = system_service.subscribe();
    render(
        &temperature,
        &memory,
        &cpu,
        &receiver.borrow().clone(),
        &config,
    );

    let weak_temperature = temperature.downgrade();
    let weak_memory = memory.downgrade();
    let weak_cpu = cpu.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let (Some(temperature), Some(memory), Some(cpu)) = (
                weak_temperature.upgrade(),
                weak_memory.upgrade(),
                weak_cpu.upgrade(),
            ) else {
                break;
            };
            render(
                &temperature,
                &memory,
                &cpu,
                &receiver.borrow().clone(),
                &config,
            );
        }
    });

    group.upcast()
}

fn configure_hover_drawer(
    group: &gtk::Box,
    revealer: &gtk::Revealer,
    buttons: [&gtk::MenuButton; 3],
) {
    let pointer_inside = Rc::new(Cell::new(false));
    let motion = gtk::EventControllerMotion::new();
    let reveal = revealer.clone();
    let inside = pointer_inside.clone();
    motion.connect_enter(move |_, _, _| {
        inside.set(true);
        reveal.set_reveal_child(true);
    });
    let reveal = revealer.clone();
    let inside = pointer_inside.clone();
    let menu_buttons = buttons
        .iter()
        .map(|button| (*button).clone())
        .collect::<Vec<_>>();
    motion.connect_leave(move |_| {
        inside.set(false);
        if !menu_buttons.iter().any(gtk::MenuButton::is_active) {
            reveal.set_reveal_child(false);
        }
    });
    group.add_controller(motion);

    for button in buttons {
        let reveal = revealer.clone();
        let inside = pointer_inside.clone();
        button.connect_active_notify(move |button| {
            if !button.is_active() && !inside.get() {
                reveal.set_reveal_child(false);
            }
        });
    }
}

fn metric(
    name: &str,
    kicker_text: &str,
    title_text: &str,
    icon_name: &str,
    orientation: gtk::Orientation,
) -> Metric {
    let button = gtk::MenuButton::new();
    button.set_widget_name(name);
    button.add_css_class("module");
    button.add_css_class("hardware-metric");
    button.set_focusable(false);

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let summary_icon = crate::icons::image(icon_name, 14);
    let summary_glyph = gtk::Label::new(None);
    summary_glyph.add_css_class("module-icon");
    let summary_value = gtk::Label::new(None);
    summary_value.set_visible(orientation != gtk::Orientation::Vertical);
    summary.append(&summary_icon);
    summary.append(&summary_glyph);
    summary.append(&summary_value);
    button.set_child(Some(&summary));

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 9);
    contents.add_css_class("popover-contents");
    contents.add_css_class("metric-panel");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("panel-header");
    let heading = gtk::Box::new(gtk::Orientation::Vertical, 2);
    heading.set_hexpand(true);
    let kicker = gtk::Label::new(Some(kicker_text));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    let title = gtk::Label::new(Some(title_text));
    title.set_xalign(0.0);
    title.add_css_class("panel-title");
    heading.append(&kicker);
    heading.append(&title);
    let value = gtk::Label::new(None);
    value.add_css_class("panel-value");
    header.append(&heading);
    header.append(&value);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("control-card");
    let meter = gtk::ProgressBar::new();
    meter.add_css_class("metric-meter");
    let details = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    details.add_css_class("panel-details");
    let primary_detail = gtk::Label::new(None);
    primary_detail.set_xalign(0.0);
    primary_detail.set_hexpand(true);
    primary_detail.add_css_class("detail-primary");
    let secondary_detail = gtk::Label::new(None);
    secondary_detail.set_xalign(1.0);
    secondary_detail.add_css_class("detail-secondary");
    details.append(&primary_detail);
    details.append(&secondary_detail);
    card.append(&meter);
    card.append(&details);
    contents.append(&header);
    contents.append(&card);

    let popover = gtk::Popover::new();
    popover.add_css_class("system-popover");
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));

    Metric {
        button,
        summary_icon,
        summary_glyph,
        summary_value,
        value,
        meter,
        primary_detail,
        secondary_detail,
    }
}

fn render(
    temperature: &Metric,
    memory: &Metric,
    cpu: &Metric,
    snapshot: &SystemSnapshot,
    config: &HardwareConfig,
) {
    let temperature_c = snapshot.temperature_c.unwrap_or_default();
    let temperature_icon = temperature_icon(temperature_c, &config.temperature_icons);
    let temperature_value = snapshot
        .temperature_c
        .map_or_else(|| "--°".to_owned(), |value| format!("{value:.0}°"));
    set_summary(
        temperature,
        temperature_icon,
        &format!("{temperature_value}C"),
    );
    temperature.value.set_label(&temperature_value);
    temperature
        .meter
        .set_fraction((temperature_c / 100.0).clamp(0.0, 1.0));
    temperature
        .primary_detail
        .set_label(temperature_status(snapshot.temperature_c));
    temperature.secondary_detail.set_label("CPU SENSOR");
    temperature
        .button
        .set_tooltip_text(Some(&snapshot.temperature_c.map_or_else(
            || "CPU temperature unavailable".to_owned(),
            |value| format!("CPU temperature: {value:.1}°C"),
        )));

    set_summary(
        memory,
        non_empty(&config.memory_icon),
        &format!("{}%", snapshot.memory_percent),
    );
    memory
        .value
        .set_label(&format!("{}%", snapshot.memory_percent));
    memory
        .meter
        .set_fraction(f64::from(snapshot.memory_percent) / 100.0);
    memory.primary_detail.set_label(&format!(
        "{:.1} / {:.1} GiB",
        snapshot.memory_used_gib, snapshot.memory_total_gib
    ));
    memory
        .secondary_detail
        .set_label(&format!("{:.1} GiB FREE", snapshot.memory_available_gib));
    memory.button.set_tooltip_text(Some(&format!(
        "Memory: {:.1} of {:.1} GiB used",
        snapshot.memory_used_gib, snapshot.memory_total_gib
    )));

    set_summary(
        cpu,
        non_empty(&config.cpu_icon),
        &format!("{}%", snapshot.cpu_percent),
    );
    cpu.value.set_label(&format!("{}%", snapshot.cpu_percent));
    cpu.meter
        .set_fraction(f64::from(snapshot.cpu_percent) / 100.0);
    cpu.primary_detail
        .set_label(&format!("LOAD {:.2}", snapshot.load_one));
    cpu.secondary_detail
        .set_label(&format!("{} THREADS", snapshot.logical_cpus));
    cpu.button.set_tooltip_text(Some(&format!(
        "CPU usage: {}% · load {:.2}",
        snapshot.cpu_percent, snapshot.load_one
    )));

    for metric in [temperature, memory, cpu] {
        if snapshot.connected {
            metric.button.remove_css_class("unavailable");
        } else {
            metric.button.add_css_class("unavailable");
        }
    }
}

fn temperature_status(temperature: Option<f64>) -> &'static str {
    match temperature {
        None => "NO SENSOR",
        Some(value) if value < 55.0 => "COOL",
        Some(value) if value < 75.0 => "NOMINAL",
        Some(value) if value < 90.0 => "WARM",
        Some(_) => "HOT",
    }
}

fn set_summary(metric: &Metric, custom_icon: Option<&str>, value: &str) {
    metric.summary_value.set_label(value);
    if let Some(icon) = custom_icon {
        metric.summary_glyph.set_label(icon);
        metric.summary_glyph.set_visible(true);
        metric.summary_icon.set_visible(false);
    } else {
        metric.summary_glyph.set_visible(false);
        metric.summary_icon.set_visible(true);
    }
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn temperature_icon(temperature: f64, icons: &[String]) -> Option<&str> {
    if icons.is_empty() {
        None
    } else if temperature < 55.0 {
        Some(&icons[0])
    } else if temperature < 75.0 {
        Some(icons.get(1).unwrap_or(&icons[0]))
    } else {
        Some(icons.get(2).unwrap_or_else(|| icons.last().unwrap()))
    }
}
