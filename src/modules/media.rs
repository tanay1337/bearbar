use gtk::glib;
use gtk::prelude::*;

use crate::config::MediaConfig;
use crate::services::desktop::{DesktopCommand, DesktopService, DesktopSnapshot, MediaCommand};

pub fn build(config: MediaConfig, service: DesktopService) -> gtk::Widget {
    let button = gtk::MenuButton::new();
    button.set_widget_name("media");
    button.add_css_class("module");
    button.add_css_class("media");
    button.set_focusable(false);

    let summary = gtk::Label::new(None);
    summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
    summary.set_max_width_chars(config.max_chars);
    button.set_child(Some(&summary));

    let contents = gtk::Box::new(gtk::Orientation::Vertical, 9);
    contents.add_css_class("popover-contents");
    contents.add_css_class("media-panel");
    let kicker = gtk::Label::new(Some("NOW PLAYING"));
    kicker.set_xalign(0.0);
    kicker.add_css_class("panel-kicker");
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("media-title");
    let artist = gtk::Label::new(None);
    artist.set_xalign(0.0);
    artist.set_ellipsize(gtk::pango::EllipsizeMode::End);
    artist.add_css_class("media-artist");
    let album = gtk::Label::new(None);
    album.set_xalign(0.0);
    album.set_ellipsize(gtk::pango::EllipsizeMode::End);
    album.add_css_class("media-album");

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.set_halign(gtk::Align::Center);
    controls.add_css_class("media-controls");
    let previous = control_button("PREVIOUS");
    let play_pause = control_button("PLAY");
    play_pause.add_css_class("primary");
    let next = control_button("NEXT");
    controls.append(&previous);
    controls.append(&play_pause);
    controls.append(&next);
    contents.append(&kicker);
    contents.append(&title);
    contents.append(&artist);
    contents.append(&album);
    contents.append(&controls);

    let popover = gtk::Popover::new();
    popover.add_css_class("media-popover");
    popover.set_child(Some(&contents));
    button.set_popover(Some(&popover));

    for (control, command) in [
        (&previous, MediaCommand::Previous),
        (&play_pause, MediaCommand::PlayPause),
        (&next, MediaCommand::Next),
    ] {
        let service = service.clone();
        control.connect_clicked(move |_| service.send(DesktopCommand::Media(command)));
    }

    let mut receiver = service.subscribe();
    render(
        &button,
        &summary,
        &title,
        &artist,
        &album,
        &play_pause,
        &receiver.borrow(),
        &config,
    );
    let weak_button = button.downgrade();
    let weak_summary = summary.downgrade();
    let weak_title = title.downgrade();
    let weak_artist = artist.downgrade();
    let weak_album = album.downgrade();
    let weak_play_pause = play_pause.downgrade();
    glib::spawn_future_local(async move {
        while receiver.changed().await.is_ok() {
            let (
                Some(button),
                Some(summary),
                Some(title),
                Some(artist),
                Some(album),
                Some(play_pause),
            ) = (
                weak_button.upgrade(),
                weak_summary.upgrade(),
                weak_title.upgrade(),
                weak_artist.upgrade(),
                weak_album.upgrade(),
                weak_play_pause.upgrade(),
            )
            else {
                break;
            };
            render(
                &button,
                &summary,
                &title,
                &artist,
                &album,
                &play_pause,
                &receiver.borrow(),
                &config,
            );
        }
    });
    button.upcast()
}

fn control_button(text: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("media-control");
    button.set_focusable(false);
    button
}

#[allow(clippy::too_many_arguments)]
fn render(
    button: &gtk::MenuButton,
    summary: &gtk::Label,
    title: &gtk::Label,
    artist: &gtk::Label,
    album: &gtk::Label,
    play_pause: &gtk::Button,
    snapshot: &DesktopSnapshot,
    config: &MediaConfig,
) {
    let Some(media) = &snapshot.media else {
        button.set_visible(false);
        return;
    };
    button.set_visible(true);
    let summary_text = if config.show_artist && !media.artist.is_empty() {
        format!("{} — {}", media.artist, media.title)
    } else {
        media.title.clone()
    };
    summary.set_label(&summary_text);
    title.set_label(&media.title);
    artist.set_label(&media.artist);
    artist.set_visible(!media.artist.is_empty());
    album.set_label(&media.album);
    album.set_visible(!media.album.is_empty());
    play_pause.set_label(if media.status == "Playing" {
        "PAUSE"
    } else {
        "PLAY"
    });
    button.set_tooltip_text(Some(&format!("{} · {}", media.player, summary_text)));
}
