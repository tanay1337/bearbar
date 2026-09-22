use gtk::glib;
use gtk::prelude::*;

use crate::config::FocusedConfig;
use crate::services::compositor::{CompositorService, WorkspaceSnapshot};

pub fn build(config: FocusedConfig, service: CompositorService) -> gtk::Widget {
    let label = gtk::Label::new(None);
    label.set_widget_name("focused");
    label.add_css_class("module");
    label.add_css_class("focused");
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(config.max_chars);

    let mut receiver = service.subscribe();
    render(&label, &receiver.borrow().clone(), &config);
    let weak_label = label.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(label) = weak_label.upgrade() else {
                break;
            };
            render(&label, &receiver.borrow().clone(), &config);
        }
    });

    label.upcast()
}

pub fn build_submap(service: CompositorService) -> gtk::Widget {
    let label = gtk::Label::new(None);
    label.set_widget_name("submap");
    label.add_css_class("module");
    label.add_css_class("submap");

    let mut receiver = service.subscribe();
    render_submap(&label, &receiver.borrow());
    let weak_label = label.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(label) = weak_label.upgrade() else {
                break;
            };
            render_submap(&label, &receiver.borrow());
        }
    });
    label.upcast()
}

pub fn build_keyboard(service: CompositorService) -> gtk::Widget {
    let label = gtk::Label::new(None);
    label.set_widget_name("keyboard");
    label.add_css_class("module");
    label.add_css_class("keyboard");
    let mut receiver = service.subscribe();
    let render = |label: &gtk::Label, layout: &str| {
        label.set_label(layout);
        label.set_visible(!layout.is_empty());
    };
    render(&label, &receiver.borrow().keyboard_layout);
    let weak_label = label.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(label) = weak_label.upgrade() else {
                break;
            };
            render(&label, &receiver.borrow().keyboard_layout);
        }
    });
    label.upcast()
}

fn render(label: &gtk::Label, snapshot: &WorkspaceSnapshot, config: &FocusedConfig) {
    let text = match (config.show_class, config.show_title) {
        (true, true) if !snapshot.active_class.is_empty() && !snapshot.active_title.is_empty() => {
            format!("{}  ·  {}", snapshot.active_class, snapshot.active_title)
        }
        (true, _) => snapshot.active_class.clone(),
        (_, true) => snapshot.active_title.clone(),
        _ => String::new(),
    };
    label.set_label(&text);
    label.set_visible(snapshot.connected && !text.is_empty());
    label.set_tooltip_text((!snapshot.active_title.is_empty()).then_some(&snapshot.active_title));
}

fn render_submap(label: &gtk::Label, snapshot: &WorkspaceSnapshot) {
    label.set_label(&snapshot.submap.to_uppercase());
    label.set_visible(
        snapshot.connected
            && !snapshot.submap.is_empty()
            && !snapshot.submap.eq_ignore_ascii_case("default"),
    );
}
