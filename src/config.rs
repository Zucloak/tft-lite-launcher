use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

pub const DEFAULT_AVD_NAME: &str = "TFT_Device";
pub const MIN_CPU_CORES: u32 = 2;
pub const MAX_CPU_CORES: u32 = 8;
pub const MIN_SDK_FREE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageDrive {
    pub mount_point: PathBuf,
    pub display_name: String,
    pub available_bytes: u64,
    pub total_bytes: u64,
    pub sdk_dir: PathBuf,
}

pub fn format_bytes_gb(bytes: u64) -> String {
    let gb = (bytes as f64) / (1024.0 * 1024.0 * 1024.0);
    format!("{gb:.1} GB")
}

fn os_default_sdk_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .map(|base| base.join("tft-lite-launcher").join("android-sdk"))
}

fn format_drive_label(name: &OsStr, mount_point: &Path) -> String {
    let vol_name = name.to_string_lossy().trim().to_owned();
    let mount_str = mount_point.to_string_lossy();
    let clean_mount = mount_str.trim_end_matches('\\');
    let short_mount = if clean_mount.is_empty() {
        mount_str.as_ref()
    } else {
        clean_mount
    };

    if vol_name.is_empty() || vol_name == short_mount {
        if cfg!(windows) {
            format!("Drive ({short_mount})")
        } else {
            short_mount.to_owned()
        }
    } else {
        format!("{vol_name} ({short_mount})")
    }
}

fn should_skip_mount(mount_point: &Path) -> bool {
    #[cfg(unix)]
    {
        let s = mount_point.to_string_lossy();
        if s.starts_with("/boot")
            || s.starts_with("/snap")
            || s.starts_with("/sys")
            || s.starts_with("/proc")
            || s.starts_with("/dev")
            || s.starts_with("/run")
            || s.starts_with("/System/Volumes")
            || s.starts_with("/private/var")
        {
            return true;
        }
    }
    #[cfg(not(unix))]
    let _ = mount_point;
    false
}

/// Enumerates writable host storage drives along with free space and the
/// recommended Android SDK directory on each drive.
pub fn detect_storage_drives() -> Vec<StorageDrive> {
    let disks = Disks::new_with_refreshed_list();
    let os_default = os_default_sdk_dir();
    let mut seen_mounts = HashSet::new();
    let mut raw_mounts: Vec<(PathBuf, String, u64, u64)> = Vec::new();

    for disk in disks.list() {
        if disk.is_read_only() || disk.total_space() == 0 {
            continue;
        }
        let mount = disk.mount_point().to_path_buf();
        if should_skip_mount(&mount) || !seen_mounts.insert(mount.clone()) {
            continue;
        }
        raw_mounts.push((
            mount.clone(),
            format_drive_label(disk.name(), &mount),
            disk.available_space(),
            disk.total_space(),
        ));
    }

    // Match the longest mount prefix for `os_default` so Linux/macOS split mounts
    // (e.g. `/` vs `/home`) place the OS default folder on the actual owning volume.
    let os_default_mount = os_default.as_ref().and_then(|def| {
        raw_mounts
            .iter()
            .filter(|(m, _, _, _)| def.starts_with(m))
            .max_by_key(|(m, _, _, _)| m.as_os_str().len())
            .map(|(m, _, _, _)| m.clone())
    });

    let mut drives: Vec<StorageDrive> = raw_mounts
        .into_iter()
        .map(|(mount_point, display_name, available_bytes, total_bytes)| {
            let sdk_dir = match (&os_default, &os_default_mount) {
                (Some(def), Some(owner)) if owner == &mount_point => def.clone(),
                _ => mount_point.join("TFT-Lite-Launcher").join("android-sdk"),
            };
            StorageDrive {
                mount_point,
                display_name,
                available_bytes,
                total_bytes,
                sdk_dir,
            }
        })
        .collect();

    drives.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    drives
}

/// Finds the storage drive hosting `path` by matching the longest mount-point prefix.
pub fn find_drive_for_path<'a>(
    path: &Path,
    drives: &'a [StorageDrive],
) -> Option<&'a StorageDrive> {
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    drives
        .iter()
        .filter(|drive| resolved.starts_with(&drive.mount_point))
        .max_by_key(|drive| drive.mount_point.as_os_str().len())
}

/// Returns the default user-writable Android SDK storage directory, preferring
/// the drive with sufficient free space when the OS default volume is tight.
pub fn default_sdk_dir() -> PathBuf {
    let drives = detect_storage_drives();
    if let Some(os_def) = os_default_sdk_dir() {
        if let Some(current_drive) = find_drive_for_path(&os_def, &drives) {
            if current_drive.available_bytes < MIN_SDK_FREE_BYTES {
                if let Some(best) = drives
                    .iter()
                    .filter(|d| d.available_bytes >= MIN_SDK_FREE_BYTES)
                    .max_by_key(|d| d.available_bytes)
                {
                    return best.sdk_dir.clone();
                }
            }
        }
        return os_def;
    }

    if let Some(best) = drives.iter().max_by_key(|d| d.available_bytes) {
        return best.sdk_dir.clone();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("android-sdk");
        }
    }
    PathBuf::from("./android-sdk")
}

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
            sdk_dir: default_sdk_dir(),
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

        // Migrate legacy relative `./android-sdk` defaults so launching from
        // protected or non-writable directories (such as System32 or Program Files)
        // does not fail with Access Denied unless a portable SDK was already provisioned there.
        let is_legacy_relative = parsed.sdk_dir == Path::new("./android-sdk")
            || parsed.sdk_dir == Path::new("android-sdk");
        if is_legacy_relative
            && !parsed.sdk_dir.join("jdk").exists()
            && !parsed.sdk_dir.join("cmdline-tools").exists()
            && !parsed.sdk_dir.join("avd").exists()
        {
            parsed.sdk_dir = default_sdk_dir();
            let _ = parsed.save();
        }

        // Redirect to a drive with sufficient free space when the configured SDK path
        // is on a volume that has less than 8 GB available and no SDK is provisioned there
        // yet. This handles the common case where Windows AppData is on a full C: drive.
        let sdk_not_provisioned = !parsed.sdk_dir.join("jdk").exists()
            && !parsed.sdk_dir.join("cmdline-tools").exists()
            && !parsed.sdk_dir.join("avd").exists();
        if sdk_not_provisioned {
            let drives = detect_storage_drives();
            let on_low_drive = find_drive_for_path(&parsed.sdk_dir, &drives)
                .map(|d| d.available_bytes < MIN_SDK_FREE_BYTES)
                .unwrap_or(false);
            if on_low_drive {
                let better = drives
                    .iter()
                    .filter(|d| d.available_bytes >= MIN_SDK_FREE_BYTES)
                    .max_by_key(|d| d.available_bytes);
                if let Some(best) = better {
                    parsed.sdk_dir = best.sdk_dir.clone();
                    let _ = parsed.save();
                }
            }
        }

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
        if self.sdk_dir.as_os_str().is_empty() {
            self.sdk_dir = default_sdk_dir();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sdk_dir_targets_app_folder() {
        let dir = default_sdk_dir();
        assert!(
            dir.ends_with(Path::new("tft-lite-launcher").join("android-sdk"))
                || dir.ends_with(Path::new("TFT-Lite-Launcher").join("android-sdk"))
                || dir.ends_with("android-sdk")
        );
    }

    #[test]
    fn format_drive_label_handles_windows_and_unix_styles() {
        let label = format_drive_label(OsStr::new("Datadisk"), Path::new("D:\\"));
        if cfg!(windows) {
            assert_eq!(label, "Datadisk (D:)");
        } else {
            assert!(label.contains("Datadisk"));
        }
        assert_eq!(format_bytes_gb(4 * 1024 * 1024 * 1024), "4.0 GB");
    }

    #[test]
    fn find_drive_for_path_picks_longest_mount_prefix() {
        #[cfg(windows)]
        let (root_a, root_b, query, expected) = (
            PathBuf::from("C:\\"),
            PathBuf::from("D:\\"),
            PathBuf::from("d:/TFT-Lite-Launcher/android-sdk"),
            "Datadisk (D:)",
        );
        #[cfg(not(windows))]
        let (root_a, root_b, query, expected) = (
            PathBuf::from("/"),
            PathBuf::from("/mnt/data"),
            PathBuf::from("/mnt/data/TFT-Lite-Launcher/android-sdk"),
            "Data (/mnt/data)",
        );

        let drives = vec![
            StorageDrive {
                mount_point: root_a.clone(),
                display_name: if cfg!(windows) {
                    "Local Disk (C:)".to_owned()
                } else {
                    "/".to_owned()
                },
                available_bytes: 5_000_000_000,
                total_bytes: 50_000_000_000,
                sdk_dir: root_a.join("android-sdk"),
            },
            StorageDrive {
                mount_point: root_b.clone(),
                display_name: expected.to_owned(),
                available_bytes: 400_000_000_000,
                total_bytes: 1_000_000_000_000,
                sdk_dir: root_b.join("TFT-Lite-Launcher").join("android-sdk"),
            },
        ];

        let found = find_drive_for_path(&query, &drives).expect("expected drive match");
        assert_eq!(found.display_name, expected);
    }

    #[test]
    fn sanitize_restores_empty_sdk_dir() {
        let mut cfg = AppConfig {
            sdk_dir: PathBuf::new(),
            ..AppConfig::default()
        };
        cfg.sanitize();
        assert!(!cfg.sdk_dir.as_os_str().is_empty());
    }
}
