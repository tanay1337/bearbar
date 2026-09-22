pub mod battery;
pub mod compositor;
pub mod desktop;
pub mod hyprland;
pub mod kde;
pub mod niri;
pub mod sway;
pub mod system;
pub mod tray;
pub mod volume;

use battery::BatteryService;
use compositor::CompositorService;
use desktop::DesktopService;
use system::SystemService;
use tray::TrayService;
use volume::VolumeService;

#[derive(Clone)]
pub struct Services {
    pub compositor: CompositorService,
    pub battery: BatteryService,
    pub volume: VolumeService,
    pub system: SystemService,
    pub desktop: DesktopService,
    pub tray: TrayService,
}

impl Services {
    pub fn start() -> Self {
        Self {
            compositor: CompositorService::start(),
            battery: BatteryService::start(),
            volume: VolumeService::start(),
            system: SystemService::start(),
            desktop: DesktopService::start(),
            tray: TrayService::start(),
        }
    }
}
