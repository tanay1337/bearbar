use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bar: BarConfig,
    pub modules: ModulesConfig,
    pub workspaces: WorkspacesConfig,
    pub focused: FocusedConfig,
    pub media: MediaConfig,
    pub control_center: ControlCenterConfig,
    pub clock: ClockConfig,
    pub volume: VolumeConfig,
    pub battery: BatteryConfig,
    pub hardware: HardwareConfig,
    pub custom: HashMap<String, CustomModuleConfig>,
}

impl Config {
    pub fn load(explicit_path: Option<&Path>) -> Result<Self> {
        let path = Self::path(explicit_path);

        let Some(path) = path else {
            return Ok(Self::default());
        };

        if !path.exists() && explicit_path.is_none() {
            return Ok(Self::default());
        }

        let source = fs::read_to_string(&path).map_err(|source| Error::ReadConfig {
            path: path.clone(),
            source,
        })?;
        let config: Self = toml::from_str(&source).map_err(|source| Error::ParseConfig {
            path: path.clone(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn path(explicit_path: Option<&Path>) -> Option<PathBuf> {
        explicit_path
            .map(Path::to_path_buf)
            .or_else(default_config_path)
    }

    fn validate(&self) -> Result<()> {
        if !(20..=96).contains(&self.bar.height) {
            return Err(Error::InvalidConfig(
                "bar.height must be between 20 and 96 logical pixels".into(),
            ));
        }
        if self.volume.scroll_step <= 0.0 || self.volume.scroll_step > 0.25 {
            return Err(Error::InvalidConfig(
                "volume.scroll_step must be greater than 0 and at most 0.25".into(),
            ));
        }
        if !(0.0..=1.5).contains(&self.volume.max_volume) {
            return Err(Error::InvalidConfig(
                "volume.max_volume must be between 0 and 1.5".into(),
            ));
        }
        if !self.volume.icons.is_empty() && self.volume.icons.len() != 3 {
            return Err(Error::InvalidConfig(
                "volume.icons must contain exactly three values (low, medium, high)".into(),
            ));
        }
        if self.battery.critical > self.battery.warning || self.battery.warning > 100 {
            return Err(Error::InvalidConfig(
                "battery thresholds must satisfy critical <= warning <= 100".into(),
            ));
        }
        if !self.battery.icons.is_empty() && self.battery.icons.len() != 5 {
            return Err(Error::InvalidConfig(
                "battery.icons must contain exactly five charge-level values".into(),
            ));
        }
        if self.hardware.transition_ms > 5_000 {
            return Err(Error::InvalidConfig(
                "hardware.transition_ms must be at most 5000".into(),
            ));
        }
        for module in self
            .modules
            .start
            .iter()
            .chain(&self.modules.center)
            .chain(&self.modules.end)
        {
            if !KNOWN_MODULES.contains(&module.as_str())
                && !module
                    .strip_prefix("custom:")
                    .is_some_and(|name| self.custom.contains_key(name))
            {
                return Err(Error::InvalidConfig(format!(
                    "unknown module in layout: {module}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CustomModuleConfig {
    pub label: String,
    pub command: String,
    pub interval_secs: u64,
    pub on_click: Option<String>,
}

impl Default for CustomModuleConfig {
    fn default() -> Self {
        Self {
            label: String::new(),
            command: String::new(),
            interval_secs: 30,
            on_click: None,
        }
    }
}

pub const KNOWN_MODULES: &[&str] = &[
    "workspaces",
    "focused",
    "media",
    "tray",
    "control_center",
    "hardware",
    "volume",
    "battery",
    "clock",
    "submap",
    "keyboard",
    "inhibit",
    "clipboard",
    "notifications",
    "privacy",
    "menu",
    "launcher",
    "custom",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModulesConfig {
    pub start: Vec<String>,
    pub center: Vec<String>,
    pub end: Vec<String>,
}

impl Default for ModulesConfig {
    fn default() -> Self {
        Self {
            start: vec!["menu".into(), "workspaces".into(), "focused".into()],
            center: vec!["media".into()],
            end: vec![
                "privacy".into(),
                "tray".into(),
                "submap".into(),
                "control_center".into(),
                "hardware".into(),
                "clock".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BarConfig {
    pub height: u32,
    pub outputs: Vec<String>,
    pub position: BarPosition,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            height: 28,
            outputs: vec!["*".into()],
            position: BarPosition::Top,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl BarPosition {
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Top => "edge-top",
            Self::Bottom => "edge-bottom",
            Self::Left => "edge-left",
            Self::Right => "edge-right",
        }
    }
}

impl BarConfig {
    pub fn includes_output(&self, connector: &str) -> bool {
        self.outputs
            .iter()
            .any(|output| output == "*" || output == connector)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacesConfig {
    pub persistent: Vec<i64>,
    pub all_outputs: bool,
    pub show_special: bool,
    pub scroll: bool,
    pub move_to_current_monitor: bool,
    pub inactive_icon: Option<String>,
    pub active_icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FocusedConfig {
    pub show_class: bool,
    pub show_title: bool,
    pub max_chars: i32,
}

impl Default for FocusedConfig {
    fn default() -> Self {
        Self {
            show_class: true,
            show_title: true,
            max_chars: 56,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConfig {
    pub show_artist: bool,
    pub max_chars: i32,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            show_artist: true,
            max_chars: 42,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlCenterConfig {
    pub icon: Option<String>,
    pub show_network_name: bool,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            persistent: vec![1, 2, 3, 4],
            all_outputs: false,
            show_special: false,
            scroll: false,
            move_to_current_monitor: false,
            inactive_icon: None,
            active_icon: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClockConfig {
    pub format: String,
    pub tooltip_format: String,
    pub calendar: bool,
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            format: "%a %d. %b %H:%M".into(),
            tooltip_format: "%A, %d %B %Y · %H:%M:%S %Z".into(),
            calendar: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolumeConfig {
    pub show_percentage: bool,
    pub scroll_step: f64,
    pub max_volume: f64,
    pub icons: Vec<String>,
    pub muted_icon: Option<String>,
    pub bluetooth_icon: Option<String>,
}

impl Default for VolumeConfig {
    fn default() -> Self {
        Self {
            show_percentage: true,
            scroll_step: 0.02,
            max_volume: 1.0,
            icons: Vec::new(),
            muted_icon: None,
            bluetooth_icon: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatteryConfig {
    pub show_percentage: bool,
    pub warning: u8,
    pub critical: u8,
    pub hide_when_absent: bool,
    pub icons: Vec<String>,
    pub charging_icon: Option<String>,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        Self {
            show_percentage: true,
            warning: 30,
            critical: 15,
            hide_when_absent: true,
            icons: Vec::new(),
            charging_icon: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HardwareConfig {
    pub enabled: bool,
    pub reveal_on_hover: bool,
    pub transition_ms: u32,
    pub temperature_icons: Vec<String>,
    pub memory_icon: String,
    pub cpu_icon: String,
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            reveal_on_hover: true,
            transition_ms: 500,
            temperature_icons: Vec::new(),
            memory_icon: String::new(),
            cpu_icon: String::new(),
        }
    }
}

pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| directory.join("bearbar"))
}

fn default_config_path() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn rejects_unknown_fields() {
        let result = toml::from_str::<Config>("[bar]\nheigth = 32");
        assert!(result.is_err());
    }

    #[test]
    fn validates_threshold_order() {
        let mut config = Config::default();
        config.battery.warning = 10;
        config.battery.critical = 20;
        assert!(config.validate().is_err());
    }

    #[test]
    fn output_filter_supports_wildcard_and_exact_names() {
        assert!(BarConfig::default().includes_output("eDP-1"));
        let config = BarConfig {
            outputs: vec!["DP-1".into()],
            ..BarConfig::default()
        };
        assert!(config.includes_output("DP-1"));
        assert!(!config.includes_output("eDP-1"));
    }

    #[test]
    fn parses_every_bar_position() {
        for position in ["top", "bottom", "left", "right"] {
            let config =
                toml::from_str::<Config>(&format!("[bar]\nposition = \"{position}\"")).unwrap();
            assert_eq!(
                config.bar.position.is_vertical(),
                matches!(position, "left" | "right")
            );
        }
    }
}
