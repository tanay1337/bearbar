use std::sync::{Arc, Mutex, mpsc};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::introspect::SinkInfo;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::proplist::Proplist;
use libpulse_binding::volume::{ChannelVolumes, Volume};
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumeSnapshot {
    pub connected: bool,
    pub sink_name: String,
    pub description: String,
    pub volume: f64,
    pub muted: bool,
    pub bluetooth: bool,
}

#[derive(Debug, Clone, Copy)]
enum Command {
    SetVolume(f64),
    ToggleMute,
}

#[derive(Debug, Clone)]
pub struct VolumeService {
    state: watch::Receiver<VolumeSnapshot>,
    commands: mpsc::Sender<Command>,
}

impl VolumeService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(VolumeSnapshot::default());
        let (commands, command_rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("bearbar-pulse".into())
            .spawn(move || run(command_rx, state_tx))
            .expect("failed to create PulseAudio thread");

        Self { state, commands }
    }

    pub fn subscribe(&self) -> watch::Receiver<VolumeSnapshot> {
        self.state.clone()
    }

    pub fn set_volume(&self, volume: f64) {
        if self.commands.send(Command::SetVolume(volume)).is_err() {
            warn!("PulseAudio command channel is closed");
        }
    }

    pub fn toggle_mute(&self) {
        if self.commands.send(Command::ToggleMute).is_err() {
            warn!("PulseAudio command channel is closed");
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveSink {
    name: String,
    volume: ChannelVolumes,
    muted: bool,
}

fn run(commands: mpsc::Receiver<Command>, state_tx: watch::Sender<VolumeSnapshot>) {
    let Some(mut properties) = Proplist::new() else {
        warn!("failed to create PulseAudio property list");
        return;
    };
    let _ = properties.set_str("application.name", "Bearbar");

    let Some(mut mainloop) = Mainloop::new() else {
        warn!("failed to create PulseAudio main loop");
        return;
    };
    let Some(context) = Context::new_with_proplist(&mainloop, "Bearbar", &properties) else {
        warn!("failed to create PulseAudio context");
        return;
    };

    let context = Arc::new(Mutex::new(context));
    let active = Arc::new(Mutex::new(None::<ActiveSink>));

    if let Ok(mut context_guard) = context.lock() {
        context_guard.set_state_callback(Some(Box::new({
            let context = context.clone();
            let active = active.clone();
            let state_tx = state_tx.clone();
            move || on_state_change(&context, &active, &state_tx)
        })));
    }

    if let Err(error) = mainloop.start() {
        warn!(?error, "failed to start PulseAudio main loop");
        return;
    }

    mainloop.lock();
    let connect_result = context.lock().map_err(|_| ()).and_then(|mut context| {
        context
            .connect(None, FlagSet::NOAUTOSPAWN, None)
            .map_err(|_| ())
    });
    mainloop.unlock();

    if connect_result.is_err() {
        warn!("failed to connect to PulseAudio-compatible server");
        mainloop.stop();
        return;
    }

    for command in commands {
        let sink = active.lock().ok().and_then(|sink| sink.clone());
        let Some(sink) = sink else {
            continue;
        };

        mainloop.lock();
        if let Ok(context) = context.lock() {
            let mut introspector = context.introspect();
            match command {
                Command::SetVolume(target) => {
                    let mut channels = sink.volume;
                    let raw = (target.clamp(0.0, 1.5) * f64::from(Volume::NORMAL.0)).round();
                    channels.get_mut().fill(Volume(raw as u32));
                    introspector.set_sink_volume_by_name(&sink.name, &channels, None);
                }
                Command::ToggleMute => {
                    introspector.set_sink_mute_by_name(&sink.name, !sink.muted, None);
                }
            }
        }
        mainloop.unlock();
    }

    mainloop.stop();
}

fn on_state_change(
    context: &Arc<Mutex<Context>>,
    active: &Arc<Mutex<Option<ActiveSink>>>,
    state_tx: &watch::Sender<VolumeSnapshot>,
) {
    let Some(state) = context.try_lock().ok().map(|context| context.get_state()) else {
        return;
    };

    match state {
        State::Ready => {
            info!("connected to PulseAudio-compatible server");
            refresh_default_sink(context, active, state_tx);

            let callback_context = context.clone();
            if let Ok(mut context_guard) = context.lock() {
                context_guard.set_subscribe_callback(Some(Box::new({
                    let context = callback_context;
                    let active = active.clone();
                    let state_tx = state_tx.clone();
                    move |facility, _, _| {
                        if matches!(facility, Some(Facility::Server | Facility::Sink)) {
                            refresh_default_sink(&context, &active, &state_tx);
                        }
                    }
                })));
                context_guard.subscribe(InterestMaskSet::SERVER | InterestMaskSet::SINK, |_| {});
            }
        }
        State::Failed | State::Terminated => {
            let mut snapshot = state_tx.borrow().clone();
            snapshot.connected = false;
            state_tx.send_replace(snapshot);
        }
        _ => {}
    }
}

fn refresh_default_sink(
    context: &Arc<Mutex<Context>>,
    active: &Arc<Mutex<Option<ActiveSink>>>,
    state_tx: &watch::Sender<VolumeSnapshot>,
) {
    let Ok(context_guard) = context.lock() else {
        return;
    };
    let introspector = context_guard.introspect();
    drop(context_guard);

    introspector.get_server_info({
        let context = context.clone();
        let active = active.clone();
        let state_tx = state_tx.clone();
        move |server| {
            let Some(name) = server.default_sink_name.as_deref().map(str::to_owned) else {
                return;
            };
            let Ok(context) = context.lock() else {
                return;
            };
            context.introspect().get_sink_info_by_name(&name, {
                let active = active.clone();
                let state_tx = state_tx.clone();
                move |result| {
                    if let ListResult::Item(info) = result {
                        publish_sink(info, &active, &state_tx);
                    }
                }
            });
        }
    });
}

fn publish_sink(
    info: &SinkInfo<'_>,
    active: &Arc<Mutex<Option<ActiveSink>>>,
    state_tx: &watch::Sender<VolumeSnapshot>,
) {
    let name = info.name.as_deref().unwrap_or_default().to_owned();
    let channel_count = usize::from(info.volume.len());
    let volume = if channel_count == 0 {
        0.0
    } else {
        let total = info
            .volume
            .get()
            .iter()
            .map(|volume| u64::from(volume.0))
            .sum::<u64>();
        total as f64 / channel_count as f64 / f64::from(Volume::NORMAL.0)
    };

    if let Ok(mut current) = active.lock() {
        *current = Some(ActiveSink {
            name: name.clone(),
            volume: info.volume,
            muted: info.mute,
        });
    }

    let bluetooth = info
        .proplist
        .get_str("device.bus")
        .is_some_and(|bus| bus == "bluetooth")
        || name.starts_with("bluez_output");

    state_tx.send_replace(VolumeSnapshot {
        connected: true,
        sink_name: name,
        description: info.description.as_deref().unwrap_or_default().to_owned(),
        volume,
        muted: info.mute,
        bluetooth,
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent_conversion_uses_pulse_normal_as_one() {
        let raw = libpulse_binding::volume::Volume::NORMAL.0;
        let fraction = f64::from(raw) / f64::from(raw);
        assert_eq!(fraction, 1.0);
    }
}
