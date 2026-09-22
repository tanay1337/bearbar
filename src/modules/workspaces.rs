use gtk::glib;
use gtk::prelude::*;

use crate::config::WorkspacesConfig;
use crate::services::compositor::{Command, CompositorService, Workspace, WorkspaceSnapshot};

pub fn build(
    output: String,
    config: WorkspacesConfig,
    service: CompositorService,
    orientation: gtk::Orientation,
) -> gtk::Widget {
    let container = gtk::Box::new(orientation, 0);
    container.set_widget_name("workspaces");
    container.add_css_class("module");
    container.add_css_class("workspaces");
    container.set_tooltip_text(Some("Compositor workspaces"));

    if config.scroll {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let scroll_service = service.clone();
        scroll.connect_scroll(move |_, _, delta_y| {
            if delta_y.abs() >= 0.01 {
                scroll_service.send(Command::Relative(if delta_y < 0.0 { -1 } else { 1 }));
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        container.add_controller(scroll);
    }

    let mut receiver = service.subscribe();
    render(
        &container,
        &receiver.borrow().clone(),
        &output,
        &config,
        &service,
    );

    let weak_container = container.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(container) = weak_container.upgrade() else {
                break;
            };
            render(
                &container,
                &receiver.borrow().clone(),
                &output,
                &config,
                &service,
            );
        }
    });

    container.upcast()
}

fn render(
    container: &gtk::Box,
    snapshot: &WorkspaceSnapshot,
    output: &str,
    config: &WorkspacesConfig,
    service: &CompositorService,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    if snapshot.connected {
        container.remove_css_class("disconnected");
    } else {
        container.add_css_class("disconnected");
    }

    let mut workspaces = snapshot
        .workspaces
        .iter()
        .filter(|workspace| {
            config.all_outputs || workspace.monitor.is_empty() || workspace.monitor == output
        })
        .filter(|workspace| config.show_special || !workspace.special)
        .cloned()
        .collect::<Vec<_>>();

    for id in config
        .persistent
        .iter()
        .filter(|_| snapshot.supports_persistent)
    {
        if !workspaces.iter().any(|workspace| workspace.id == *id) {
            workspaces.push(Workspace {
                id: *id,
                name: id.to_string(),
                reference: id.to_string(),
                monitor: output.to_owned(),
                windows: 0,
                active: false,
                visible: false,
                urgent: false,
                special: false,
            });
        }
    }

    workspaces.sort_by(
        |left, right| match (left.name.parse::<i64>(), right.name.parse::<i64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => left.name.cmp(&right.name),
        },
    );

    for workspace in workspaces {
        let label = if workspace.active {
            config
                .active_icon
                .as_deref()
                .or(config.inactive_icon.as_deref())
        } else {
            config.inactive_icon.as_deref()
        }
        .unwrap_or(&workspace.name);
        let button = gtk::Button::with_label(label);
        button.add_css_class("workspace");
        button.set_focusable(false);
        button.set_tooltip_text(Some(&workspace_tooltip(&workspace)));

        if workspace.active {
            button.add_css_class("active");
        } else if workspace.visible {
            button.add_css_class("visible");
        }
        if workspace.windows == 0 {
            button.add_css_class("empty");
        } else {
            button.add_css_class("occupied");
        }
        if workspace.urgent {
            button.add_css_class("urgent");
        }
        if workspace.special {
            button.add_css_class("special");
        }

        let service = service.clone();
        let workspace_reference = workspace.reference.clone();
        let current_monitor = config.move_to_current_monitor;
        button.connect_clicked(move |_| {
            service.send(Command::Focus {
                id: workspace.id,
                workspace: workspace_reference.clone(),
                current_monitor,
            });
        });

        container.append(&button);
    }

    if container.first_child().is_none() && !snapshot.connected {
        let status = gtk::Label::new(Some("Compositor unavailable"));
        status.add_css_class("status-error");
        container.append(&status);
    }
}

fn workspace_tooltip(workspace: &Workspace) -> String {
    let windows = match workspace.windows {
        0 => "empty".to_owned(),
        1 => "1 window".to_owned(),
        count => format!("{count} windows"),
    };
    format!("Workspace {} · {windows}", workspace.name)
}
