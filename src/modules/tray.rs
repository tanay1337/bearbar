use gtk::glib;
use gtk::prelude::*;
use system_tray::item::{IconPixmap, Status};
use system_tray::menu::{MenuItem, MenuType, ToggleState};

use crate::services::tray::{TrayCommand, TrayItem, TrayService, TraySnapshot};

pub fn build(service: TrayService, orientation: gtk::Orientation) -> gtk::Widget {
    let tray = gtk::Box::new(orientation, 1);
    tray.set_widget_name("tray");
    tray.add_css_class("tray");
    let mut receiver = service.subscribe();
    render(&tray, &receiver.borrow(), &service);
    let weak_tray = tray.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(tray) = weak_tray.upgrade() else {
                break;
            };
            render(&tray, &receiver.borrow(), &service);
        }
    });
    tray.upcast()
}

fn render(tray: &gtk::Box, snapshot: &TraySnapshot, service: &TrayService) {
    while let Some(child) = tray.first_child() {
        tray.remove(&child);
    }
    for item in snapshot
        .items
        .iter()
        .filter(|item| item.status != Status::Passive)
    {
        tray.append(&build_item(item, service));
    }
    tray.set_visible(
        snapshot
            .items
            .iter()
            .any(|item| item.status != Status::Passive),
    );
}

fn build_item(item: &TrayItem, service: &TrayService) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("tray-item");
    button.set_focusable(false);
    button.set_tooltip_text(Some(&item.title));
    if item.status == Status::NeedsAttention {
        button.add_css_class("attention");
    }
    if let Some(image) = item_image(item) {
        button.set_child(Some(&image));
    } else {
        button.set_label(&item.title.chars().next().unwrap_or('•').to_string());
    }
    let address = item.address.clone();
    let activate = service.clone();
    button.connect_clicked(move |_| activate.send(TrayCommand::Activate(address.clone())));

    if let (Some(menu), Some(path)) = (&item.menu, &item.menu_path) {
        let popover = gtk::Popover::new();
        popover.add_css_class("tray-menu-popover");
        let contents = gtk::Box::new(gtk::Orientation::Vertical, 2);
        contents.add_css_class("tray-menu");
        for entry in &menu.submenus {
            append_menu_item(&contents, entry, &item.address, path, service, 0);
        }
        popover.set_child(Some(&contents));
        popover.set_parent(&button);
        let click = gtk::GestureClick::new();
        click.set_button(3);
        let weak_popover = popover.downgrade();
        click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(popover) = weak_popover.upgrade() {
                popover.popup();
            }
        });
        button.add_controller(click);
        button.connect_destroy(move |_| popover.unparent());
    } else {
        let address = item.address.clone();
        let secondary = service.clone();
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.connect_pressed(move |gesture, _, _, _| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            secondary.send(TrayCommand::Secondary(address.clone()));
        });
        button.add_controller(click);
    }
    button
}

fn append_menu_item(
    parent: &gtk::Box,
    item: &MenuItem,
    address: &str,
    path: &str,
    service: &TrayService,
    depth: u8,
) {
    if !item.visible {
        return;
    }
    if item.menu_type == MenuType::Separator {
        parent.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        return;
    }
    let prefix = if item.toggle_state == ToggleState::On {
        "✓ "
    } else {
        ""
    };
    let label = item.label.as_deref().unwrap_or_default().replace('_', "");
    let button =
        gtk::Button::with_label(&format!("{}{prefix}{label}", "  ".repeat(depth as usize)));
    button.add_css_class("tray-menu-item");
    button.set_sensitive(item.enabled);
    let command = TrayCommand::MenuItem {
        address: address.to_owned(),
        path: path.to_owned(),
        id: item.id,
    };
    let activation_service = service.clone();
    button.connect_clicked(move |_| activation_service.send(command.clone()));
    parent.append(&button);
    for child in &item.submenu {
        append_menu_item(
            parent,
            child,
            address,
            path,
            service,
            depth.saturating_add(1),
        );
    }
}

fn item_image(item: &TrayItem) -> Option<gtk::Image> {
    if let Some(name) = item.icon_name.as_deref().filter(|name| !name.is_empty()) {
        let image = if name.starts_with('/') {
            gtk::Image::from_file(name)
        } else {
            gtk::Image::from_icon_name(name)
        };
        image.set_pixel_size(17);
        return Some(image);
    }
    let pixmap = best_pixmap(item.icon_pixmap.as_deref()?)?;
    if pixmap.width <= 0
        || pixmap.height <= 0
        || pixmap.pixels.len() < (pixmap.width * pixmap.height * 4) as usize
    {
        return None;
    }
    let bytes = glib::Bytes::from_owned(pixmap.pixels.clone());
    let texture = gtk::gdk::MemoryTexture::new(
        pixmap.width,
        pixmap.height,
        gtk::gdk::MemoryFormat::A8r8g8b8,
        &bytes,
        pixmap.width as usize * 4,
    );
    let image = gtk::Image::from_paintable(Some(&texture));
    image.set_pixel_size(17);
    Some(image)
}

fn best_pixmap(pixmaps: &[IconPixmap]) -> Option<&IconPixmap> {
    pixmaps
        .iter()
        .min_by_key(|pixmap| (pixmap.width - 20).unsigned_abs())
}
