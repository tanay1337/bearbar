use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config::{BarPosition, Config};
use crate::services::Services;

#[derive(Clone)]
pub struct BarManager {
    application: gtk::Application,
    config: Rc<RefCell<Config>>,
    services: Services,
    windows: Rc<RefCell<HashMap<String, gtk::ApplicationWindow>>>,
}

impl BarManager {
    pub fn new(application: &gtk::Application, config: Config, services: Services) -> Self {
        Self {
            application: application.clone(),
            config: Rc::new(RefCell::new(config)),
            services,
            windows: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn start(&self) {
        let Some(display) = gtk::gdk::Display::default() else {
            return;
        };
        let monitors = display.monitors();
        self.reconcile(&monitors);

        let manager = self.clone();
        monitors.connect_items_changed(move |monitors, _, _, _| {
            manager.reconcile(monitors);
        });
    }

    pub fn reload(&self, config: Config) {
        self.config.replace(config);
        for (_, window) in self.windows.borrow_mut().drain() {
            window.close();
        }
        if let Some(display) = gtk::gdk::Display::default() {
            self.reconcile(&display.monitors());
        }
    }

    fn reconcile(&self, monitors: &gio::ListModel) {
        let mut present = HashSet::new();

        for index in 0..monitors.n_items() {
            let Some(monitor) = monitors
                .item(index)
                .and_then(|item| item.downcast::<gtk::gdk::Monitor>().ok())
            else {
                continue;
            };

            let connector = monitor
                .connector()
                .map(|connector| connector.to_string())
                .unwrap_or_else(|| format!("monitor-{index}"));
            if !self.config.borrow().bar.includes_output(&connector) {
                continue;
            }

            present.insert(connector.clone());
            if !self.windows.borrow().contains_key(&connector) {
                let window = self.build_window(&monitor, &connector);
                window.present();
                self.windows.borrow_mut().insert(connector, window);
            }
        }

        let removed = self
            .windows
            .borrow()
            .keys()
            .filter(|connector| !present.contains(*connector))
            .cloned()
            .collect::<Vec<_>>();
        for connector in removed {
            if let Some(window) = self.windows.borrow_mut().remove(&connector) {
                window.close();
            }
        }
    }

    fn build_window(&self, monitor: &gtk::gdk::Monitor, connector: &str) -> gtk::ApplicationWindow {
        let config = self.config.borrow().clone();
        let window = gtk::ApplicationWindow::builder()
            .application(&self.application)
            .decorated(false)
            .resizable(true)
            .build();
        window.set_widget_name("bearbar-window");
        window.add_css_class("bearbar-window");

        window.init_layer_shell();
        window.set_namespace(Some("bearbar"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_monitor(Some(monitor));
        match config.bar.position {
            BarPosition::Top => {
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Right, true);
            }
            BarPosition::Bottom => {
                window.set_anchor(Edge::Bottom, true);
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Right, true);
            }
            BarPosition::Left => {
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Bottom, true);
            }
            BarPosition::Right => {
                window.set_anchor(Edge::Right, true);
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Bottom, true);
            }
        }
        window.set_exclusive_zone(config.bar.height as i32);

        let orientation = if config.bar.position.is_vertical() {
            window.set_size_request(config.bar.height as i32, -1);
            gtk::Orientation::Vertical
        } else {
            window.set_size_request(-1, config.bar.height as i32);
            gtk::Orientation::Horizontal
        };

        let start = self.build_section(
            "start",
            &config.modules.start,
            connector,
            &config,
            orientation,
        );
        let center = self.build_section(
            "center",
            &config.modules.center,
            connector,
            &config,
            orientation,
        );
        let end = self.build_section("end", &config.modules.end, connector, &config, orientation);
        if orientation == gtk::Orientation::Vertical {
            start.set_valign(gtk::Align::Start);
            center.set_valign(gtk::Align::Center);
            end.set_valign(gtk::Align::End);
        } else {
            start.set_halign(gtk::Align::Start);
            center.set_halign(gtk::Align::Center);
            end.set_halign(gtk::Align::End);
        }

        let layout = gtk::CenterBox::new();
        layout.set_widget_name("bar");
        layout.add_css_class("bar");
        layout.add_css_class(config.bar.position.css_class());
        layout.set_orientation(orientation);
        layout.set_shrink_center_last(true);
        layout.set_start_widget(Some(&start));
        layout.set_center_widget(Some(&center));
        layout.set_end_widget(Some(&end));
        window.set_child(Some(&layout));

        window
    }

    fn build_section(
        &self,
        name: &str,
        modules: &[String],
        connector: &str,
        config: &Config,
        orientation: gtk::Orientation,
    ) -> gtk::Box {
        let section = gtk::Box::new(orientation, 0);
        section.set_widget_name(name);
        for module in modules {
            if let Some(widget) = self.build_module(module, connector, config, orientation) {
                if config.bar.position.is_vertical() {
                    compact_vertical_label(&widget);
                }
                section.append(&widget);
            }
        }
        section
    }

    fn build_module(
        &self,
        module: &str,
        connector: &str,
        config: &Config,
        orientation: gtk::Orientation,
    ) -> Option<gtk::Widget> {
        match module {
            "workspaces" => Some(crate::modules::workspaces::build(
                connector.to_owned(),
                config.workspaces.clone(),
                self.services.compositor.clone(),
                orientation,
            )),
            "focused" => Some(crate::modules::focused::build(
                config.focused.clone(),
                self.services.compositor.clone(),
            )),
            "media" => Some(crate::modules::media::build(
                config.media.clone(),
                self.services.desktop.clone(),
            )),
            "tray" => Some(crate::modules::tray::build(
                self.services.tray.clone(),
                orientation,
            )),
            "control_center" => Some(crate::modules::control_center::build(
                config.control_center.clone(),
                config.volume.clone(),
                self.services.desktop.clone(),
                self.services.volume.clone(),
            )),
            "inhibit" => Some(crate::modules::control_center::build_inhibit(
                self.services.desktop.clone(),
            )),
            "submap" => Some(crate::modules::focused::build_submap(
                self.services.compositor.clone(),
            )),
            "keyboard" => Some(crate::modules::focused::build_keyboard(
                self.services.compositor.clone(),
            )),
            "launcher" => Some(crate::modules::launcher::build(
                self.services.compositor.clone(),
            )),
            "menu" => Some(crate::modules::menu::build()),
            "clipboard" => crate::modules::clipboard::build(),
            "notifications" => Some(crate::modules::notifications::build(
                self.services.desktop.clone(),
            )),
            "privacy" => Some(crate::modules::privacy::build(
                self.services.desktop.clone(),
            )),
            "hardware" => Some(crate::modules::hardware::build(
                config.hardware.clone(),
                config.battery.clone(),
                self.services.battery.clone(),
                self.services.desktop.clone(),
                self.services.system.clone(),
                orientation,
            )),
            "volume" => Some(crate::modules::volume::build(
                config.volume.clone(),
                self.services.volume.clone(),
                orientation,
            )),
            "battery" => Some(crate::modules::battery::build(
                config.battery.clone(),
                self.services.battery.clone(),
                self.services.desktop.clone(),
                orientation,
            )),
            "clock" => Some(crate::modules::clock::build(
                config.clock.clone(),
                orientation,
            )),
            module if module.starts_with("custom:") => config
                .custom
                .get(module.trim_start_matches("custom:"))
                .cloned()
                .map(|config| {
                    crate::modules::custom::build(module.trim_start_matches("custom:"), config)
                }),
            _ => {
                tracing::warn!(module, "configured module is not available in this build");
                None
            }
        }
    }
}

fn compact_vertical_label(widget: &gtk::Widget) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(3);
    } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        if let Some(label) = button.child().and_downcast::<gtk::Label>() {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(3);
        }
    } else if let Some(button) = widget.downcast_ref::<gtk::MenuButton>()
        && let Some(label) = button.child().and_downcast::<gtk::Label>()
    {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(3);
    }
}
