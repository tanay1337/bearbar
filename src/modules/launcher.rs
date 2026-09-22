use gtk::glib;
use gtk::prelude::*;

use crate::services::compositor::{Command, CompositorService, WorkspaceSnapshot};

pub fn build(service: CompositorService) -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.set_widget_name("launcher");
    button.add_css_class("module");
    button.add_css_class("launcher");
    button.set_focusable(false);
    button.set_child(Some(&gtk::Label::new(Some("APPS"))));

    let list = gtk::Box::new(gtk::Orientation::Vertical, 5);
    let contents = gtk::Box::new(gtk::Orientation::Vertical, 10);
    contents.add_css_class("popover-contents");
    contents.add_css_class("window-panel");
    let kicker = gtk::Label::new(Some("OPEN WINDOWS"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    contents.append(&kicker);
    contents.append(&list);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));

    let mut receiver = service.subscribe();
    render(&list, &receiver.borrow(), &service);
    let weak_list = list.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(list) = weak_list.upgrade() else {
                break;
            };
            render(&list, &receiver.borrow(), &service);
        }
    });
    button.upcast()
}

fn render(list: &gtk::Box, snapshot: &WorkspaceSnapshot, service: &CompositorService) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if snapshot.windows.is_empty() {
        let empty = gtk::Label::new(Some("No open windows"));
        empty.set_xalign(0.0);
        empty.add_css_class("empty");
        list.append(&empty);
        return;
    }
    for window in &snapshot.windows {
        let button = gtk::Button::new();
        button.add_css_class("window-row");
        if window.active {
            button.add_css_class("active");
        }
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let badge = gtk::Label::new(Some(
            &window.class.chars().next().unwrap_or('?').to_string(),
        ));
        badge.add_css_class("app-badge");
        let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
        text.set_hexpand(true);
        let title = gtk::Label::new(Some(if window.title.is_empty() {
            &window.class
        } else {
            &window.title
        }));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(36);
        let meta = gtk::Label::new(Some(&format!(
            "{} · SPACE {}",
            window.class, window.workspace_id
        )));
        meta.set_xalign(0.0);
        meta.add_css_class("row-meta");
        text.append(&title);
        text.append(&meta);
        content.append(&badge);
        content.append(&text);
        button.set_child(Some(&content));
        let address = window.address.clone();
        let service = service.clone();
        button.connect_clicked(move |_| service.send(Command::FocusWindow(address.clone())));
        list.append(&button);
    }
}
