use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;
use flate2::read::GzDecoder;
use tar::Archive as TarArchive;
use thiserror::Error;
use zip::ZipArchive;

use crate::config::{
    detect_storage_drives, find_drive_for_path, format_bytes_gb, AppConfig, MIN_SDK_FREE_BYTES,
};

/// Official Google Android Command Line Tools revision (no proprietary binaries bundled).
const CMDLINE_TOOLS_REV: &str = "11076708";

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("Unsupported host platform: {os}/{arch}")]
    UnsupportedPlatform { os: &'static str, arch: &'static str },

    #[error("Failed to download runtime asset from {url}: {source}")]
    DownloadFailed {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("HTTP status {status} while downloading {url}")]
    HttpStatus { url: String, status: u16 },

    #[error("Failed to extract ZIP archive: {0}")]
    ArchiveExtraction(#[from] zip::result::ZipError),

    #[error("Unrecognized OpenJDK archive format downloaded from {url}")]
    UnsupportedArchiveFormat { url: String },

    #[error("Portable OpenJDK 17 extraction completed but {expected} was not found")]
    JavaBinaryMissing { expected: PathBuf },

    #[error("Command '{program}' exited with status {code}: {stderr}")]
    SubprocessFailed {
        program: String,
        code: i32,
        stderr: String,
    },

    #[error("Insufficient disk space at '{}' ({} free, at least {} required). Select another drive in Launcher or Settings.", path.display(), format_bytes_gb(*available_bytes), format_bytes_gb(*required_bytes))]
    InsufficientDiskSpace {
        path: PathBuf,
        available_bytes: u64,
        required_bytes: u64,
    },

    #[error("I/O error while {action} '{}': {source}", path.display())]
    IoPath {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

fn io_ctx<T>(action: &'static str, path: &Path, res: io::Result<T>) -> Result<T, SdkError> {
    res.map_err(|source| SdkError::IoPath {
        action,
        path: path.to_path_buf(),
        source,
    })
}

fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTarget {
    pub os: &'static str,
    pub arch: &'static str,
}

impl HostTarget {
    pub fn detect() -> Result<Self, SdkError> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;

        match (os, arch) {
            ("windows" | "macos" | "linux", "x86_64" | "aarch64") => Ok(Self { os, arch }),
            _ => Err(SdkError::UnsupportedPlatform { os, arch }),
        }
    }

    pub fn cmdline_tools_url(&self) -> Result<String, SdkError> {
        let os_slug = match self.os {
            "windows" => "win",
            "macos" => "mac",
            "linux" => "linux",
            other => {
                return Err(SdkError::UnsupportedPlatform {
                    os: other,
                    arch: self.arch,
                })
            }
        };

        Ok(format!(
            "https://dl.google.com/android/repository/commandlinetools-{os_slug}-{CMDLINE_TOOLS_REV}_latest.zip"
        ))
    }

    /// Resolves the official Eclipse Temurin OpenJDK 17 JRE download endpoint
    /// from the Adoptium API for zero-dependency host execution.
    pub fn adoptium_jre_url(&self) -> Result<String, SdkError> {
        let os_slug = match self.os {
            "windows" => "windows",
            "macos" => "mac",
            "linux" => "linux",
            other => {
                return Err(SdkError::UnsupportedPlatform {
                    os: other,
                    arch: self.arch,
                })
            }
        };

        let arch_slug = match self.arch {
            "x86_64" => "x64",
            "aarch64" => "aarch64",
            other => {
                return Err(SdkError::UnsupportedPlatform {
                    os: self.os,
                    arch: other,
                })
            }
        };

        Ok(format!(
            "https://api.adoptium.net/v3/binary/latest/17/ga/{os_slug}/{arch_slug}/jre/hotspot/normal/eclipse"
        ))
    }

    /// Selects the unrooted Google Play system image matching the host CPU ABI.
    /// The google_apis_playstore variant is required so Play Integrity checks
    /// pass; on x86_64 it also includes Intel libhoudini ARM binary translation
    /// so ARM-compiled mobile binaries execute smoothly on desktop CPUs.
    pub fn system_image_package(&self) -> &'static str {
        match self.arch {
            "aarch64" => "system-images;android-34;google_apis_playstore;arm64-v8a",
            _ => "system-images;android-34;google_apis_playstore;x86_64",
        }
    }
}

pub struct SdkPaths {
    pub root: PathBuf,
    pub avd_home: PathBuf,
}

impl SdkPaths {
    pub fn from_config(config: &AppConfig) -> Self {
        let root = if config.sdk_dir.is_absolute() {
            config.sdk_dir.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(&config.sdk_dir)
        };
        let avd_home = root.join("avd");
        Self { root, avd_home }
    }

    /// Local portable OpenJDK 17 root (`./android-sdk/jdk`).
    pub fn jdk_dir(&self) -> PathBuf {
        self.root.join("jdk")
    }

    /// Canonicalized `JAVA_HOME` path injected into `sdkmanager` and `avdmanager`
    /// without Windows `\\?\` verbatim prefixes that break batch scripts.
    pub fn canonical_jdk_home(&self) -> PathBuf {
        let dir = self.jdk_dir();
        let resolved = fs::canonicalize(&dir).unwrap_or(dir);
        strip_windows_verbatim_prefix(resolved)
    }

    /// Predictable flattened Java executable (`./android-sdk/jdk/bin/java` or `java.exe`).
    pub fn java_bin(&self) -> PathBuf {
        let bin = if cfg!(windows) { "java.exe" } else { "java" };
        self.jdk_dir().join("bin").join(bin)
    }

    pub fn sdkmanager(&self) -> PathBuf {
        let bin = if cfg!(windows) {
            "sdkmanager.bat"
        } else {
            "sdkmanager"
        };
        self.root
            .join("cmdline-tools")
            .join("latest")
            .join("bin")
            .join(bin)
    }

    pub fn avdmanager(&self) -> PathBuf {
        let bin = if cfg!(windows) {
            "avdmanager.bat"
        } else {
            "avdmanager"
        };
        self.root
            .join("cmdline-tools")
            .join("latest")
            .join("bin")
            .join(bin)
    }

    pub fn emulator(&self) -> PathBuf {
        let bin = if cfg!(windows) {
            "emulator.exe"
        } else {
            "emulator"
        };
        self.root.join("emulator").join(bin)
    }

    pub fn adb(&self) -> PathBuf {
        let bin = if cfg!(windows) { "adb.exe" } else { "adb" };
        self.root.join("platform-tools").join(bin)
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn system_image_dir(&self, package: &str) -> PathBuf {
        package
            .split(';')
            .fold(self.root.clone(), |acc, part| acc.join(part))
    }

    pub fn has_system_image(&self, package: &str) -> bool {
        self.system_image_dir(package)
            .join("source.properties")
            .exists()
    }

    pub fn avd_config_ini(&self, avd_name: &str) -> PathBuf {
        self.avd_home
            .join(format!("{avd_name}.avd"))
            .join("config.ini")
    }

    pub fn is_provisioned(&self, avd_name: &str) -> bool {
        self.java_bin().exists()
            && self.adb().exists()
            && self.emulator().exists()
            && self.avd_config_ini(avd_name).exists()
    }
}

/// Ensures portable OpenJDK 17, Google's Android Command Line Tools,
/// platform-tools, emulator, Google Play system image, and the TFT_Device AVD
/// exist locally with zero host-system dependencies.
pub fn ensure_provisioned<F>(config: &AppConfig, mut report: F) -> Result<SdkPaths, SdkError>
where
    F: FnMut(f32, &str),
{
    let target = HostTarget::detect()?;
    let paths = SdkPaths::from_config(config);
    let image = target.system_image_package();
    let needs_packages =
        !paths.adb().exists() || !paths.emulator().exists() || !paths.has_system_image(image);

    if needs_packages {
        let drives = detect_storage_drives();
        if let Some(drive) = find_drive_for_path(&paths.root, &drives) {
            if drive.available_bytes < MIN_SDK_FREE_BYTES {
                return Err(SdkError::InsufficientDiskSpace {
                    path: paths.root,
                    available_bytes: drive.available_bytes,
                    required_bytes: MIN_SDK_FREE_BYTES,
                });
            }
        }
    }

    io_ctx("creating SDK root directory", &paths.root, fs::create_dir_all(&paths.root))?;
    io_ctx("creating AVD directory", &paths.avd_home, fs::create_dir_all(&paths.avd_home))?;

    let result = (|| -> Result<(), SdkError> {
        if !paths.java_bin().exists() {
            report(0.06, "Fetching portable OpenJDK 17 runtime...");
            download_and_extract_portable_jdk(&target, &paths)?;
            report(0.16, "Portable OpenJDK 17 runtime ready");
        }

        if !paths.sdkmanager().exists() {
            report(0.22, "Downloading Android Command Line Tools");
            download_and_extract_cmdline_tools(&target, &paths.root)?;
        }

        if needs_packages {
            report(0.36, "Accepting Android SDK licenses");
            accept_licenses(&paths)?;

            report(0.48, "Installing platform-tools, emulator, and Google Play image");
            run_sdk_cmd(
                &paths,
                &paths.sdkmanager(),
                &[
                    &format!("--sdk_root={}", paths.root.display()),
                    "platform-tools",
                    "emulator",
                    image,
                ],
                None,
            )?;
        }

        if !paths.avd_config_ini(&config.avd_name).exists() {
            report(0.72, "Provisioning pixel_tablet AVD (TFT_Device)");
            create_tft_avd(&paths, &target, config)?;
        } else {
            // Keep CPU, RAM, GPU, and 1080p LCD directives aligned with current settings.
            patch_avd_config_ini(&paths.avd_config_ini(&config.avd_name), config)?;
        }

        Ok(())
    })();

    let _ = fs::remove_dir_all(paths.tmp_dir());
    result?;

    report(0.82, "SDK and AVD ready");
    Ok(paths)
}

fn download_url_to_file(url: &str, dest_path: &Path) -> Result<(), SdkError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("TFT-Lite-Launcher/0.1")
        .build()
        .map_err(|source| SdkError::DownloadFailed {
            url: url.to_owned(),
            source,
        })?;

    let mut response = client
        .get(url)
        .send()
        .map_err(|source| SdkError::DownloadFailed {
            url: url.to_owned(),
            source,
        })?;

    if !response.status().is_success() {
        return Err(SdkError::HttpStatus {
            url: url.to_owned(),
            status: response.status().as_u16(),
        });
    }

    let mut out_file = io_ctx("creating download file", dest_path, File::create(dest_path))?;
    io::copy(&mut response, &mut out_file)?;
    out_file.flush()?;
    Ok(())
}

/// Downloads Eclipse Temurin OpenJDK 17 from Adoptium and flattens its
/// top-level versioned folder (and macOS `Contents/Home` if present) directly
/// into `android-sdk/jdk` while enforcing zip-slip path traversal protection.
fn download_and_extract_portable_jdk(
    target: &HostTarget,
    paths: &SdkPaths,
) -> Result<(), SdkError> {
    let url = target.adoptium_jre_url()?;
    let archive_path = paths.root.join("temurin-jdk17.download");
    download_url_to_file(&url, &archive_path)?;

    let jdk_dir = paths.jdk_dir();
    if jdk_dir.exists() {
        io_ctx("clearing existing JDK directory", &jdk_dir, fs::remove_dir_all(&jdk_dir))?;
    }
    io_ctx("creating JDK directory", &jdk_dir, fs::create_dir_all(&jdk_dir))?;

    let mut magic = [0u8; 4];
    {
        let mut probe = io_ctx("opening JDK archive", &archive_path, File::open(&archive_path))?;
        let _ = probe.read(&mut magic)?;
    }

    let extract_result = if magic[0] == 0x50 && magic[1] == 0x4B {
        extract_jdk_zip(&archive_path, &jdk_dir)
    } else if magic[0] == 0x1F && magic[1] == 0x8B {
        extract_jdk_tar_gz(&archive_path, &jdk_dir)
    } else {
        Err(SdkError::UnsupportedArchiveFormat { url })
    };

    let _ = fs::remove_file(&archive_path);
    extract_result?;

    let expected = paths.java_bin();
    if !expected.exists() {
        return Err(SdkError::JavaBinaryMissing { expected });
    }

    Ok(())
}

/// Strips the top-level versioned directory (e.g. `jdk-17.0.13+11-jre`) and
/// optional macOS bundle prefix (`Contents/Home`) while rejecting any path
/// containing parent traversal (`..`) or root prefixes.
fn flatten_jdk_relative_path(raw: &Path) -> Option<PathBuf> {
    let mut normals = Vec::new();
    for component in raw.components() {
        match component {
            Component::Normal(seg) => normals.push(seg),
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if normals.len() <= 1 {
        return None;
    }

    let mut slice = &normals[1..];
    if slice.len() >= 2 && slice[0] == "Contents" && slice[1] == "Home" {
        slice = &slice[2..];
    }

    if slice.is_empty() {
        return None;
    }

    let mut out = PathBuf::new();
    for seg in slice {
        out.push(seg);
    }
    Some(out)
}

fn extract_jdk_zip(archive_path: &Path, jdk_dir: &Path) -> Result<(), SdkError> {
    let archive_file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(archive_file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        // Zip-slip guard: reject any archive entry escaping the target root.
        let Some(enclosed) = entry.enclosed_name().map(PathBuf::from) else {
            continue;
        };
        let Some(relative) = flatten_jdk_relative_path(&enclosed) else {
            continue;
        };

        let dest_path = jdk_dir.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&dest_path)?;
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut dest_file = File::create(&dest_path)?;
        io::copy(&mut entry, &mut dest_file)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.unix_mode().unwrap_or_else(|| {
                if relative.starts_with("bin") {
                    0o755
                } else {
                    0o644
                }
            });
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
        }
    }

    Ok(())
}

fn extract_jdk_tar_gz(archive_path: &Path, jdk_dir: &Path) -> Result<(), SdkError> {
    let archive_file = File::open(archive_path)?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = TarArchive::new(decoder);

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let raw_path = entry.path()?.into_owned();
        // Path traversal guard: flatten_jdk_relative_path rejects ParentDir/RootDir/Prefix.
        let Some(relative) = flatten_jdk_relative_path(&raw_path) else {
            continue;
        };

        let dest_path = jdk_dir.join(&relative);
        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            fs::create_dir_all(&dest_path)?;
            continue;
        }

        if entry_type.is_symlink() {
            #[cfg(unix)]
            if let Ok(Some(link_target)) = entry.link_name() {
                if !link_target.is_absolute() {
                    if let Some(parent) = dest_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let _ = fs::remove_file(&dest_path);
                    let _ = std::os::unix::fs::symlink(link_target, &dest_path);
                }
            }
            continue;
        }

        if !entry_type.is_file() {
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut dest_file = File::create(&dest_path)?;
        io::copy(&mut entry, &mut dest_file)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode().unwrap_or_else(|_| {
                if relative.starts_with("bin") || relative.ends_with("jspawnhelper") {
                    0o755
                } else {
                    0o644
                }
            });
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
        }
    }

    Ok(())
}

fn download_and_extract_cmdline_tools(target: &HostTarget, sdk_root: &Path) -> Result<(), SdkError> {
    let url = target.cmdline_tools_url()?;
    let zip_path = sdk_root.join("cmdline-tools.zip");
    download_url_to_file(&url, &zip_path)?;

    // sdkmanager expects binaries under <ANDROID_HOME>/cmdline-tools/latest/bin.
    let latest_dir = sdk_root.join("cmdline-tools").join("latest");
    if latest_dir.exists() {
        io_ctx("clearing existing cmdline-tools directory", &latest_dir, fs::remove_dir_all(&latest_dir))?;
    }
    io_ctx("creating cmdline-tools directory", &latest_dir, fs::create_dir_all(&latest_dir))?;

    {
        let archive_file = io_ctx("opening cmdline-tools archive", &zip_path, File::open(&zip_path))?;
        let mut archive = ZipArchive::new(archive_file)?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let Some(enclosed) = entry.enclosed_name().map(PathBuf::from) else {
                continue;
            };

            let relative = enclosed
                .strip_prefix("cmdline-tools")
                .unwrap_or(&enclosed);
            if relative.as_os_str().is_empty() {
                continue;
            }

            let dest_path = latest_dir.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&dest_path)?;
                continue;
            }

            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut dest_file = File::create(&dest_path)?;
            io::copy(&mut entry, &mut dest_file)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = entry.unix_mode().unwrap_or_else(|| {
                    if relative.starts_with("bin") {
                        0o755
                    } else {
                        0o644
                    }
                });
                fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
            }
        }
    }

    let _ = fs::remove_file(&zip_path);
    Ok(())
}

fn accept_licenses(paths: &SdkPaths) -> Result<(), SdkError> {
    let yes_stream = "y\n".repeat(32);
    run_sdk_cmd(
        paths,
        &paths.sdkmanager(),
        &[
            &format!("--sdk_root={}", paths.root.display()),
            "--licenses",
        ],
        Some(&yes_stream),
    )?;
    Ok(())
}

fn create_tft_avd(
    paths: &SdkPaths,
    target: &HostTarget,
    config: &AppConfig,
) -> Result<(), SdkError> {
    let image = target.system_image_package();
    run_sdk_cmd(
        paths,
        &paths.avdmanager(),
        &[
            "create",
            "avd",
            "--force",
            "--name",
            &config.avd_name,
            "--package",
            image,
            "--device",
            "pixel_tablet",
        ],
        Some("no\n"),
    )?;

    patch_avd_config_ini(&paths.avd_config_ini(&config.avd_name), config)?;
    Ok(())
}

fn patch_avd_config_ini(ini_path: &Path, config: &AppConfig) -> Result<(), SdkError> {
    let existing = if ini_path.exists() {
        io_ctx("reading AVD config", ini_path, fs::read_to_string(ini_path))?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| {
            !line.starts_with("hw.cpu.ncore=")
                && !line.starts_with("hw.gpu.enabled=")
                && !line.starts_with("hw.gpu.mode=")
                && !line.starts_with("hw.ramSize=")
                && !line.starts_with("hw.keyboard=")
                && !line.starts_with("hw.lcd.width=")
                && !line.starts_with("hw.lcd.height=")
                && !line.starts_with("hw.lcd.density=")
                && !line.starts_with("PlayStore.enabled=")
        })
        .map(ToOwned::to_owned)
        .collect();

    lines.push(format!("hw.cpu.ncore={}", config.cpu_cores));
    lines.push("hw.gpu.enabled=yes".to_owned());
    lines.push(format!("hw.gpu.mode={}", config.graphics_backend.as_flag()));
    lines.push(format!("hw.ramSize={}", config.ram_mb));
    lines.push("hw.keyboard=yes".to_owned());
    // Enforce native 16:9 1080p dimensions so Teamfight Tactics renders without letterbox bars.
    lines.push("hw.lcd.width=1920".to_owned());
    lines.push("hw.lcd.height=1080".to_owned());
    lines.push("hw.lcd.density=240".to_owned());
    // Required for Google Play certification — avdmanager always writes "no" but the
    // emulator only sets PlayStore.enabled=true in hardware-qemu.ini when this is "yes".
    // Without it Google Play Services marks the device as uncertified and blocks downloads.
    lines.push("PlayStore.enabled = yes".to_owned());

    io_ctx(
        "writing AVD config",
        ini_path,
        fs::write(ini_path, format!("{}\n", lines.join("\n"))),
    )?;
    Ok(())
}

/// Builds a child `PATH` environment value with `./android-sdk/jdk/bin`
/// prepended ahead of the inherited host `PATH`.
fn build_jdk_prepended_path(jdk_home: &Path) -> OsString {
    let jdk_bin = jdk_home.join("bin");
    let mut entries = vec![jdk_bin];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(entries).unwrap_or_else(|_| jdk_home.join("bin").into_os_string())
}

pub fn run_sdk_cmd(
    paths: &SdkPaths,
    program: &Path,
    args: &[&str],
    stdin_payload: Option<&str>,
) -> Result<String, SdkError> {
    let jdk_home = paths.canonical_jdk_home();
    let merged_path = build_jdk_prepended_path(&jdk_home);
    let tmp_dir = paths.tmp_dir();
    io_ctx("creating SDK temp directory", &tmp_dir, fs::create_dir_all(&tmp_dir))?;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("ANDROID_HOME", &paths.root)
        .env("ANDROID_SDK_ROOT", &paths.root)
        .env("ANDROID_AVD_HOME", &paths.avd_home)
        .env("JAVA_HOME", &jdk_home)
        .env("PATH", &merged_path)
        .env("TMP", &tmp_dir)
        .env("TEMP", &tmp_dir)
        .env("TMPDIR", &tmp_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if stdin_payload.is_some() {
        cmd.stdin(Stdio::piped());
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = io_ctx("spawning SDK tool", program, cmd.spawn())?;
    if let Some(payload) = stdin_payload {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.as_bytes());
        }
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(SdkError::SubprocessFailed {
            program: program.display().to_string(),
            code,
            stderr,
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adoptium_urls_match_expected_platforms() {
        let win = HostTarget { os: "windows", arch: "x86_64" };
        assert_eq!(
            win.adoptium_jre_url().unwrap(),
            "https://api.adoptium.net/v3/binary/latest/17/ga/windows/x64/jre/hotspot/normal/eclipse"
        );

        let mac_arm = HostTarget { os: "macos", arch: "aarch64" };
        assert_eq!(
            mac_arm.adoptium_jre_url().unwrap(),
            "https://api.adoptium.net/v3/binary/latest/17/ga/mac/aarch64/jre/hotspot/normal/eclipse"
        );

        let mac_x64 = HostTarget { os: "macos", arch: "x86_64" };
        assert_eq!(
            mac_x64.adoptium_jre_url().unwrap(),
            "https://api.adoptium.net/v3/binary/latest/17/ga/mac/x64/jre/hotspot/normal/eclipse"
        );

        let linux = HostTarget { os: "linux", arch: "x86_64" };
        assert_eq!(
            linux.adoptium_jre_url().unwrap(),
            "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jre/hotspot/normal/eclipse"
        );
    }

    #[test]
    fn flatten_jdk_path_strips_top_level_and_macos_bundle_and_blocks_traversal() {
        assert_eq!(
            flatten_jdk_relative_path(Path::new("jdk-17.0.13+11-jre/bin/java.exe")),
            Some(PathBuf::from("bin/java.exe"))
        );
        assert_eq!(
            flatten_jdk_relative_path(Path::new("jdk-17.0.13+11-jre/Contents/Home/bin/java")),
            Some(PathBuf::from("bin/java"))
        );
        assert_eq!(
            flatten_jdk_relative_path(Path::new("jdk-17.0.13+11-jre/../../etc/passwd")),
            None
        );
    }

    #[test]
    fn strip_windows_verbatim_prefix_normalizes_paths() {
        #[cfg(windows)]
        {
            assert_eq!(
                strip_windows_verbatim_prefix(PathBuf::from(r"\\?\C:\Users\Test\jdk")),
                PathBuf::from(r"C:\Users\Test\jdk")
            );
            assert_eq!(
                strip_windows_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\jdk")),
                PathBuf::from(r"\\server\share\jdk")
            );
        }
        assert_eq!(
            strip_windows_verbatim_prefix(PathBuf::from("/tmp/android-sdk/jdk")),
            PathBuf::from("/tmp/android-sdk/jdk")
        );
    }
}
