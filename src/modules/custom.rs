use std::process::{Command, Stdio};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use tokio::sync::watch;

use crate::config::CustomModuleConfig;
use crate::runtime;

pub fn build(name: &str, config: CustomModuleConfig) -> gtk::Widget {
    let button = gtk::Button::new();
    button.set_widget_name(&format!("custom-{name}"));
    button.add_css_class("module");
    button.add_css_class("custom");
    button.set_focusable(false);
    button.set_label(&config.label);
    if let Some(command) = config.on_click {
        button.connect_clicked(move |_| {
            if let Err(error) = Command::new("/bin/sh")
                .args(["-c", &command])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                tracing::warn!(%error, "failed to run custom click command");
            }
        });
    }
    if config.command.is_empty() {
        return button.upcast();
    }
    let (sender, mut receiver) = watch::channel(config.label);
    runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(config.interval_secs.max(1)));
        loop {
            tokio::select! {
                _ = sender.closed() => break,
                _ = interval.tick() => {}
            }
            let result = tokio::time::timeout(
                Duration::from_secs(10),
                tokio::process::Command::new("/bin/sh")
                    .args(["-c", &config.command])
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .output(),
            )
            .await;
            if let Ok(Ok(output)) = result
                && output.status.success()
            {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                sender.send_replace(value);
            }
        }
    });
    let weak_button = button.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let Some(button) = weak_button.upgrade() else {
                break;
            };
            button.set_label(&receiver.borrow());
        }
    });
    button.upcast()
}
