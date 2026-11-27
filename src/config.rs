use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

pub const DEFAULT_AVD_NAME: &str = "TFT_Device";
pub const MIN_CPU_CORES: u32 = 2;
pub const MAX_CPU_CORES: u32 = 8;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsBackend {
    #[default]
    Host,
    AngleIndirect,
    SwiftshaderIndirect,
}

impl GraphicsBackend {
    pub fn as_flag(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::AngleIndirect => "angle_indirect",
            Self::SwiftshaderIndirect => "swiftshader_indirect",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Host => "Host / Native GPU",
            Self::AngleIndirect => "ANGLE / DirectX 11",
            Self::SwiftshaderIndirect => "SwiftShader",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Host => "Host GPU",
            Self::AngleIndirect => "ANGLE DX11",
            Self::SwiftshaderIndirect => "SwiftShader",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    #[default]
    BorderlessFullscreen,
    Fullscreen,
    Windowed1600x900,
}

impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::BorderlessFullscreen => "Borderless Fullscreen",
            Self::Fullscreen => "Fullscreen",
            Self::Windowed1600x900 => "Windowed 1600x900",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostHardware {
    pub logical_threads: usize,
    pub total_ram_mb: u64,
    pub recommended_cores: u32,
    pub recommended_ram_mb: u32,
}

/// Inspects host logical CPU threads and physical memory via sysinfo so
/// first-run AVD defaults scale safely to the user's machine.
pub fn detect_host_hardware() -> HostHardware {
    let sys = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(CpuRefreshKind::new())
            .with_memory(MemoryRefreshKind::everything()),
    );

    let logical_threads = sys.cpus().len().max(2);
    let total_ram_mb = sys.total_memory() / (1024 * 1024);

    // Allocate half of logical threads, clamped to 2..=6 to keep host OS scheduling responsive.
    let recommended_cores = ((logical_threads / 2) as u32).clamp(2, 6);

    // Reserve roughly one quarter of system memory in 512 MB increments, clamped to 2048..=4096 MB.
    let quarter_mb = ((total_ram_mb / 4) / 512) * 512;
    let recommended_ram_mb = (quarter_mb as u32).clamp(2048, 4096);

    HostHardware {
        logical_threads,
        total_ram_mb,
        recommended_cores,
        recommended_ram_mb,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeybindTarget {
    pub action: String,
    pub key: String,
    pub x_pct: f32,
    pub y_pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub sdk_dir: PathBuf,
    pub avd_name: String,
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub graphics_backend: GraphicsBackend,
    #[serde(default)]
    pub display_mode: DisplayMode,
    pub keymapper_enabled: bool,
    pub keybinds: Vec<KeybindTarget>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let hw = detect_host_hardware();
        Self {
            sdk_dir: PathBuf::from("./android-sdk"),
            avd_name: DEFAULT_AVD_NAME.to_owned(),
            cpu_cores: hw.recommended_cores,
            ram_mb: hw.recommended_ram_mb,
            graphics_backend: GraphicsBackend::Host,
            display_mode: DisplayMode::BorderlessFullscreen,
            keymapper_enabled: true,
            keybinds: vec![
                KeybindTarget {
                    action: "Refresh Shop".to_owned(),
                    key: "D".to_owned(),
                    x_pct: 12.5,
                    y_pct: 91.5,
                },
                KeybindTarget {
                    action: "Buy XP".to_owned(),
                    key: "F".to_owned(),
                    x_pct: 12.5,
                    y_pct: 83.0,
                },
                KeybindTarget {
                    action: "Sell Unit".to_owned(),
                    key: "E".to_owned(),
                    x_pct: 50.0,
                    y_pct: 89.0,
                },
            ],
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir()
            .context("Unable to resolve OS-standard user configuration directory")?;
        Ok(base.join("tft-lite-launcher").join("config.toml"))
    }

    pub fn load_or_default() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let defaults = Self::default();
            defaults.save()?;
            return Ok(defaults);
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {}", path.display()))?;
        let mut parsed: Self = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse TOML config at {}", path.display()))?;

        parsed.sanitize();
        Ok(parsed)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }

        let serialized = toml::to_string_pretty(self).context("Failed to serialize config TOML")?;
        fs::write(&path, serialized)
            .with_context(|| format!("Failed to write config file at {}", path.display()))?;
        Ok(())
    }

    pub fn sanitize(&mut self) {
        self.cpu_cores = self.cpu_cores.clamp(MIN_CPU_CORES, MAX_CPU_CORES);
        self.ram_mb = self.ram_mb.clamp(2048, 16384);
        for bind in &mut self.keybinds {
            bind.x_pct = bind.x_pct.clamp(0.0, 100.0);
            bind.y_pct = bind.y_pct.clamp(0.0, 100.0);
            bind.key = bind.key.trim().to_uppercase();
            if bind.key.is_empty() {
                bind.key = "D".to_owned();
            }
        }
    }
}
