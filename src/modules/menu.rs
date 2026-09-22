use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

pub fn build() -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.set_widget_name("menu");
    button.add_css_class("module");
    button.add_css_class("menu-button");
    button.set_focusable(false);
    button.set_child(Some(&crate::icons::image("bearbar-logo-symbolic", 16)));

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 10);
    contents.add_css_class("popover-contents");
    contents.add_css_class("app-menu-panel");
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search applications"));
    contents.append(&search);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 3);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_min_content_height(360);
    scroller.set_child(Some(&list));
    contents.append(&scroller);

    let mut applications = gio::AppInfo::all()
        .into_iter()
        .filter(|app| app.should_show())
        .collect::<Vec<_>>();
    applications.sort_by_key(|app| app.display_name().to_ascii_lowercase());
    let mut rows = Vec::new();
    for app in applications {
        let row = gtk::Button::new();
        row.add_css_class("app-row");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        if let Some(icon) = app.icon() {
            let image = gtk::Image::from_gicon(&icon);
            image.set_pixel_size(22);
            content.append(&image);
        }
        let label = gtk::Label::new(Some(&app.display_name()));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        content.append(&label);
        row.set_child(Some(&content));
        let launched = app.clone();
        row.connect_clicked(move |_| {
            if let Err(error) = launched.launch(&[], None::<&gio::AppLaunchContext>) {
                tracing::warn!(%error, "failed to launch application");
            }
        });
        let terms = format!("{} {}", app.display_name(), app.name()).to_ascii_lowercase();
        rows.push((terms, row.clone()));
        list.append(&row);
    }
    let rows = Rc::new(rows);
    search.connect_search_changed(move |search| {
        let query = search.text().to_ascii_lowercase();
        for (terms, row) in rows.iter() {
            row.set_visible(query.is_empty() || terms.contains(&query));
        }
    });

    let popover = gtk::Popover::new();
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));
    button.upcast()
}
