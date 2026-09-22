use gtk::glib;
use gtk::prelude::*;

use crate::services::desktop::{DesktopService, DesktopSnapshot};

pub fn build(service: DesktopService) -> gtk::Widget {
    let label = gtk::Label::new(None);
    label.set_widget_name("privacy");
    label.add_css_class("module");
    label.add_css_class("privacy");
    let mut receiver = service.subscribe();
    render(&label, &receiver.borrow());
    let weak_label = label.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(label) = weak_label.upgrade() else {
                break;
            };
            render(&label, &receiver.borrow());
        }
    });
    label.upcast()
}

fn render(label: &gtk::Label, snapshot: &DesktopSnapshot) {
    let mut active = Vec::new();
    if snapshot.microphone_active {
        active.push("MIC");
    }
    if snapshot.camera_active {
        active.push("CAM");
    }
    if snapshot.screen_active {
        active.push("SCREEN");
    }
    label.set_label(&active.join(" · "));
    label.set_tooltip_text(
        (!active.is_empty()).then_some("An application is using a capture device"),
    );
    label.set_visible(!active.is_empty());
}
