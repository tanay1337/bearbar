use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::prelude::*;

const DEFAULT_STYLE: &str = include_str!("../assets/default.css");

pub struct StyleManager {
    _monitor: Option<gio::FileMonitor>,
}

pub fn load(display: &gtk::gdk::Display, explicit_path: Option<&Path>) -> StyleManager {
    let defaults = gtk::CssProvider::new();
    defaults.connect_parsing_error(|_, section, error| {
        tracing::error!(%section, %error, "embedded stylesheet error");
    });
    defaults.load_from_string(DEFAULT_STYLE);
    gtk::style_context_add_provider_for_display(
        display,
        &defaults,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let path = style_path(explicit_path);
    let overrides = gtk::CssProvider::new();
    overrides.connect_parsing_error(|_, section, error| {
        tracing::error!(%section, %error, "user stylesheet error");
    });
    reload(&overrides, &path);
    gtk::style_context_add_provider_for_display(
        display,
        &overrides,
        gtk::STYLE_PROVIDER_PRIORITY_USER,
    );

    let monitor = monitor_parent(&path);
    if let Some(monitor) = &monitor {
        let generation = Rc::new(Cell::new(0_u64));
        monitor.connect_changed(move |_, file, other, _| {
            if !matches_path(file, &path) && !other.is_some_and(|file| matches_path(file, &path)) {
                return;
            }
            let next = generation.get().wrapping_add(1);
            generation.set(next);
            let generation = generation.clone();
            let provider = overrides.clone();
            let path = path.clone();
            gtk::glib::timeout_add_local_once(Duration::from_millis(180), move || {
                if generation.get() == next {
                    reload(&provider, &path);
                }
            });
        });
    }
    StyleManager { _monitor: monitor }
}

fn reload(provider: &gtk::CssProvider, path: &Path) {
    if path.exists() {
        provider.load_from_path(path);
        tracing::info!(path = %path.display(), "loaded user stylesheet");
    } else {
        provider.load_from_string("");
    }
}

fn monitor_parent(path: &Path) -> Option<gio::FileMonitor> {
    let parent = path.parent()?;
    gio::File::for_path(parent)
        .monitor_directory(
            gio::FileMonitorFlags::WATCH_MOVES,
            None::<&gio::Cancellable>,
        )
        .map_err(
            |error| tracing::warn!(%error, path = %parent.display(), "cannot watch stylesheet"),
        )
        .ok()
}

fn matches_path(file: &gio::File, target: &Path) -> bool {
    file.path().as_deref() == Some(target)
}

fn style_path(explicit_path: Option<&Path>) -> PathBuf {
    explicit_path.map(Path::to_path_buf).unwrap_or_else(|| {
        crate::config::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("style.css")
    })
}
