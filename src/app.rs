use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio;
use gtk::prelude::*;

use crate::bar::BarManager;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::runtime;
use crate::services::Services;

#[derive(Default)]
struct UiState {
    manager: Option<BarManager>,
    _config_monitor: Option<gio::FileMonitor>,
    _style: Option<crate::style::StyleManager>,
}

pub fn run(
    config: Config,
    config_path: Option<PathBuf>,
    style_path: Option<PathBuf>,
) -> Result<()> {
    runtime::initialize()?;

    let application = gtk::Application::builder()
        .application_id("dev.bearbar.Bearbar")
        .build();
    let initial_config = Rc::new(RefCell::new(Some(config)));
    let state = Rc::new(RefCell::new(UiState::default()));

    application.connect_activate(move |application| {
        if state.borrow().manager.is_some() {
            return;
        }
        let Some(display) = gtk::gdk::Display::default() else {
            tracing::error!("GTK could not open a display");
            application.quit();
            return;
        };
        let Some(config) = initial_config.borrow_mut().take() else {
            return;
        };

        crate::icons::register(&display);
        let style = crate::style::load(&display, style_path.as_deref());
        let services = Services::start();
        let manager = BarManager::new(application, config, services);
        manager.start();
        let config_monitor = Config::path(config_path.as_deref())
            .as_deref()
            .and_then(|path| watch_config(path, &manager));
        state.replace(UiState {
            manager: Some(manager),
            _config_monitor: config_monitor,
            _style: Some(style),
        });
    });

    let exit_code = application.run_with_args::<&str>(&[]);
    if exit_code == gtk::glib::ExitCode::SUCCESS {
        Ok(())
    } else {
        Err(Error::NoDisplay)
    }
}

fn watch_config(path: &Path, manager: &BarManager) -> Option<gio::FileMonitor> {
    let parent = path.parent()?;
    let monitor = gio::File::for_path(parent)
        .monitor_directory(
            gio::FileMonitorFlags::WATCH_MOVES,
            None::<&gio::Cancellable>,
        )
        .map_err(|error| tracing::warn!(%error, path = %parent.display(), "cannot watch config"))
        .ok()?;
    let target = path.to_owned();
    let manager = manager.clone();
    let generation = Rc::new(Cell::new(0_u64));
    monitor.connect_changed(move |_, file, other, _| {
        if !matches_path(file, &target) && !other.is_some_and(|file| matches_path(file, &target)) {
            return;
        }
        let next = generation.get().wrapping_add(1);
        generation.set(next);
        let generation = generation.clone();
        let target = target.clone();
        let manager = manager.clone();
        gtk::glib::timeout_add_local_once(Duration::from_millis(180), move || {
            if generation.get() != next {
                return;
            }
            match Config::load(Some(&target)) {
                Ok(config) => {
                    manager.reload(config);
                    tracing::info!(path = %target.display(), "reloaded configuration");
                }
                Err(error) => tracing::error!(%error, "configuration reload rejected"),
            }
        });
    });
    Some(monitor)
}

fn matches_path(file: &gio::File, target: &Path) -> bool {
    file.path().as_deref() == Some(target)
}
