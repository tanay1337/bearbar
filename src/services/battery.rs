use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::watch;
use tracing::{info, warn};
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::runtime;

const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChargeState {
    #[default]
    Unknown,
    Charging,
    Discharging,
    Empty,
    Full,
    PendingCharge,
    PendingDischarge,
}

impl From<u32> for ChargeState {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Charging,
            2 => Self::Discharging,
            3 => Self::Empty,
            4 => Self::Full,
            5 => Self::PendingCharge,
            6 => Self::PendingDischarge,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BatterySnapshot {
    pub connected: bool,
    pub present: bool,
    pub percentage: f64,
    pub state: ChargeState,
    pub seconds_remaining: i64,
    pub capacity: f64,
}

#[derive(Debug, Clone)]
pub struct BatteryService {
    state: watch::Receiver<BatterySnapshot>,
}

impl BatteryService {
    pub fn start() -> Self {
        let (state_tx, state) = watch::channel(BatterySnapshot::default());
        runtime::spawn(run(state_tx));
        Self { state }
    }

    pub fn subscribe(&self) -> watch::Receiver<BatterySnapshot> {
        self.state.clone()
    }
}

async fn run(state_tx: watch::Sender<BatterySnapshot>) {
    let mut retry = Duration::from_secs(1);

    loop {
        match listen(&state_tx).await {
            Ok(()) => warn!("UPower signal stream ended"),
            Err(error) => warn!(%error, "UPower connection failed"),
        }

        let mut snapshot = state_tx.borrow().clone();
        snapshot.connected = false;
        state_tx.send_replace(snapshot);
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(30));
    }
}

async fn listen(state_tx: &watch::Sender<BatterySnapshot>) -> zbus::Result<()> {
    let connection = zbus::Connection::system().await?;
    let upower = UPowerProxy::new(&connection).await?;
    let path = upower.get_display_device().await?;
    let interface = InterfaceName::from_static_str(DEVICE_INTERFACE)
        .expect("the UPower device interface name is valid");

    let properties = PropertiesProxy::builder(&connection)
        .destination("org.freedesktop.UPower")?
        .path(path)?
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    let physical_battery = physical_battery(&connection, &upower, &interface).await?;

    publish(&properties, physical_battery.as_ref(), &interface, state_tx).await?;
    info!("connected to UPower");

    let mut changes = properties.receive_properties_changed().await?;
    while let Some(signal) = changes.next().await {
        let args = signal.args()?;
        if args.interface_name == interface {
            publish(&properties, physical_battery.as_ref(), &interface, state_tx).await?;
        }
    }

    Ok(())
}

async fn publish(
    proxy: &PropertiesProxy<'_>,
    physical_battery: Option<&PropertiesProxy<'_>>,
    interface: &InterfaceName<'_>,
    state_tx: &watch::Sender<BatterySnapshot>,
) -> zbus::Result<()> {
    let properties = proxy.get_all(interface.clone()).await?;
    let physical_properties = if let Some(proxy) = physical_battery {
        Some(proxy.get_all(interface.clone()).await?)
    } else {
        None
    };
    state_tx.send_replace(snapshot_from_properties(
        &properties,
        physical_properties.as_ref(),
    ));
    Ok(())
}

fn snapshot_from_properties(
    properties: &std::collections::HashMap<String, OwnedValue>,
    physical_properties: Option<&std::collections::HashMap<String, OwnedValue>>,
) -> BatterySnapshot {
    let percentage = properties
        .get("Percentage")
        .and_then(|value| value.downcast_ref::<f64>().ok())
        .unwrap_or_default();
    let raw_state = properties
        .get("State")
        .and_then(|value| value.downcast_ref::<u32>().ok())
        .unwrap_or_default();
    let state = ChargeState::from(raw_state);
    let time_to_empty = properties
        .get("TimeToEmpty")
        .and_then(|value| value.downcast_ref::<i64>().ok())
        .unwrap_or_default();
    let time_to_full = properties
        .get("TimeToFull")
        .and_then(|value| value.downcast_ref::<i64>().ok())
        .unwrap_or_default();

    BatterySnapshot {
        connected: true,
        present: properties
            .get("IsPresent")
            .and_then(|value| value.downcast_ref::<bool>().ok())
            .unwrap_or(false),
        percentage,
        state,
        seconds_remaining: if matches!(state, ChargeState::Charging | ChargeState::PendingCharge) {
            time_to_full
        } else {
            time_to_empty
        },
        capacity: properties
            .get("Capacity")
            .and_then(|value| value.downcast_ref::<f64>().ok())
            .filter(|capacity| *capacity > 0.0)
            .or_else(|| {
                physical_properties?
                    .get("Capacity")?
                    .downcast_ref::<f64>()
                    .ok()
            })
            .unwrap_or_default(),
    }
}

async fn physical_battery<'a>(
    connection: &'a zbus::Connection,
    upower: &UPowerProxy<'a>,
    interface: &InterfaceName<'_>,
) -> zbus::Result<Option<PropertiesProxy<'a>>> {
    for path in upower.enumerate_devices().await? {
        let proxy = PropertiesProxy::builder(connection)
            .destination("org.freedesktop.UPower")?
            .path(path)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let properties = proxy.get_all(interface.clone()).await?;
        let is_battery = properties
            .get("Type")
            .and_then(|value| value.downcast_ref::<u32>().ok())
            == Some(2);
        let is_power_supply = properties
            .get("PowerSupply")
            .and_then(|value| value.downcast_ref::<bool>().ok())
            .unwrap_or(false);
        if is_battery && is_power_supply {
            return Ok(Some(proxy));
        }
    }
    Ok(None)
}

#[zbus::proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
trait UPower {
    fn get_display_device(&self) -> zbus::Result<OwnedObjectPath>;
    fn enumerate_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_upower_states() {
        assert_eq!(ChargeState::from(1), ChargeState::Charging);
        assert_eq!(ChargeState::from(2), ChargeState::Discharging);
        assert_eq!(ChargeState::from(4), ChargeState::Full);
        assert_eq!(ChargeState::from(99), ChargeState::Unknown);
    }

    #[test]
    fn reads_capacity_from_the_physical_battery() {
        let display =
            std::collections::HashMap::from([("Capacity".to_owned(), OwnedValue::from(0.0_f64))]);
        let physical =
            std::collections::HashMap::from([("Capacity".to_owned(), OwnedValue::from(88.2_f64))]);
        let snapshot = snapshot_from_properties(&display, Some(&physical));
        assert_eq!(snapshot.capacity, 88.2);
    }
}
