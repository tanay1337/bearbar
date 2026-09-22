use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

pub fn build() -> Option<gtk::Widget> {
    let clipboard = gtk::gdk::Display::default()?.clipboard();
    let button = gtk::MenuButton::new();
    button.set_widget_name("clipboard");
    button.add_css_class("module");
    button.set_focusable(false);
    button.set_child(Some(&gtk::Label::new(Some("CLIP"))));
    let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let contents = gtk::Box::new(gtk::Orientation::Vertical, 10);
    contents.add_css_class("popover-contents");
    contents.add_css_class("clipboard-panel");
    let kicker = gtk::Label::new(Some("CLIPBOARD HISTORY"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    contents.append(&kicker);
    contents.append(&list);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));

    let history = Rc::new(RefCell::new(Vec::<String>::new()));
    let capture = {
        let clipboard = clipboard.clone();
        let history = history.clone();
        let list = list.clone();
        move || {
            let clipboard = clipboard.clone();
            let history = history.clone();
            let list = list.clone();
            gtk::glib::spawn_future_local(async move {
                if let Ok(Some(text)) = clipboard.read_text_future().await {
                    let text = text.trim().to_owned();
                    if text.is_empty() {
                        return;
                    }
                    let mut values = history.borrow_mut();
                    values.retain(|value| value != &text);
                    values.insert(0, text);
                    values.truncate(10);
                    render(&list, &values, &clipboard);
                }
            });
        }
    };
    capture();
    clipboard.connect_changed(move |_| capture());
    Some(button.upcast())
}

fn render(list: &gtk::Box, values: &[String], clipboard: &gtk::gdk::Clipboard) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for value in values {
        let row = gtk::Button::with_label(&value.replace('\n', " "));
        row.add_css_class("clipboard-row");
        row.set_tooltip_text(Some(value));
        let value = value.clone();
        let clipboard = clipboard.clone();
        row.connect_clicked(move |_| clipboard.set_text(&value));
        list.append(&row);
    }
}
