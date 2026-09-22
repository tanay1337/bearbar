use gtk::glib;
use gtk::prelude::*;

use crate::services::desktop::{DesktopCommand, DesktopService, DesktopSnapshot};

pub fn build(service: DesktopService) -> gtk::Widget {
    let button = gtk::Button::new();
    button.set_widget_name("notifications");
    button.add_css_class("module");
    button.set_focusable(false);
    button.connect_clicked({
        let service = service.clone();
        move |_| service.send(DesktopCommand::ToggleNotifications)
    });
    let mut receiver = service.subscribe();
    render(&button, &receiver.borrow());
    let weak_button = button.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(button) = weak_button.upgrade() else {
                break;
            };
            render(&button, &receiver.borrow());
        }
    });
    button.upcast()
}

fn render(button: &gtk::Button, snapshot: &DesktopSnapshot) {
    button.set_visible(snapshot.notifications_available);
    button.set_label(
        if snapshot.notification_count == 0 {
            "NOTIFY".into()
        } else {
            format!("NOTIFY {}", snapshot.notification_count)
        }
        .as_str(),
    );
}
