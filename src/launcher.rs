use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Local;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::config::{AppConfig, DisplayMode};
use crate::sdk::{ensure_provisioned, run_sdk_cmd, SdkPaths};

pub const TFT_PACKAGE_ID: &str = "com.riotgames.league.teamfighttactics";
const BOOT_TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: String,
    pub level: LogLevel,
    pub message: String,
}

impl LogEntry {
    pub fn new(source: impl Into<String>, level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            source: source.into(),
            level,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum LaunchEvent {
    Log(LogEntry),
    Progress { fraction: f32, stage: String },
    PlayStoreRequired,
    GameRunning,
    Stopped,
    Failed(String),
}

#[derive(Clone, Default)]
pub struct EmulatorController {
    child: Arc<Mutex<Option<Child>>>,
    cancel_flag: Arc<AtomicBool>,
}

impl EmulatorController {
    pub fn spawn_launch(&self, config: AppConfig, tx: Sender<LaunchEvent>) {
        self.cancel_flag.store(false, Ordering::SeqCst);
        let child_slot = Arc::clone(&self.child);
        let cancel_flag = Arc::clone(&self.cancel_flag);

        thread::spawn(move || {
            if let Err(err) = run_launch_pipeline(&config, &tx, &child_slot, &cancel_flag) {
                let _ = tx.send(LaunchEvent::Log(LogEntry::new(
                    "Launcher",
                    LogLevel::Error,
                    err.clone(),
                )));
                let _ = tx.send(LaunchEvent::Failed(err));
            }
        });
    }

    pub fn stop(&self, config: &AppConfig, tx: &Sender<LaunchEvent>) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        let paths = SdkPaths::from_config(config);

        if paths.adb().exists() {
            let _ = run_sdk_cmd(&paths, &paths.adb(), &["emu", "kill"], None);
        }

        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }

        let _ = tx.send(LaunchEvent::Log(LogEntry::new(
            "Emulator",
            LogLevel::Info,
            "Emulator process terminated",
        )));
        let _ = tx.send(LaunchEvent::Stopped);
    }
}

fn run_launch_pipeline(
    config: &AppConfig,
    tx: &Sender<LaunchEvent>,
    child_slot: &Arc<Mutex<Option<Child>>>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    let _ = tx.send(LaunchEvent::Log(LogEntry::new(
        "SDK",
        LogLevel::Info,
        format!("Verifying Android SDK in {}", config.sdk_dir.display()),
    )));

    let paths = ensure_provisioned(config, |fraction, stage| {
        let _ = tx.send(LaunchEvent::Progress {
            fraction,
            stage: stage.to_owned(),
        });
        let _ = tx.send(LaunchEvent::Log(LogEntry::new(
            "SDK",
            LogLevel::Info,
            stage,
        )));
    })
    .map_err(|err| err.to_string())?;

    if cancel_flag.load(Ordering::SeqCst) {
        let _ = tx.send(LaunchEvent::Stopped);
        return Ok(());
    }

    let gpu_flag = config.graphics_backend.as_flag();
    let _ = tx.send(LaunchEvent::Progress {
        fraction: 0.86,
        stage: format!(
            "Booting {} ({} cores, {} MB RAM, -gpu {})",
            config.avd_name, config.cpu_cores, config.ram_mb, gpu_flag
        ),
    });

    // -no-snapshot-save prevents corrupted Quick Boot snapshots from stalling
    // subsequent launches or breaking Play Integrity device state.
    // -no-boot-anim trims cold boot time on desktop GPUs.
    let ram_arg = config.ram_mb.to_string();
    let mut emulator_args = vec![
        "-avd",
        &config.avd_name,
        "-gpu",
        gpu_flag,
        "-memory",
        &ram_arg,
        "-no-boot-anim",
        "-no-snapshot-save",
    ];
    if config.display_mode == DisplayMode::Windowed1600x900 {
        emulator_args.extend(["-window-size", "1600,900"]);
    }

    let mut emulator_cmd = Command::new(paths.emulator());
    emulator_cmd
        .args(&emulator_args)
        .env("ANDROID_HOME", &paths.root)
        .env("ANDROID_SDK_ROOT", &paths.root)
        .env("ANDROID_AVD_HOME", &paths.avd_home)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Route hybrid AMD/Intel + NVIDIA Windows laptops to the dedicated GPU
    // instead of integrated graphics when spawning emulator.exe.
    #[cfg(target_os = "windows")]
    {
        emulator_cmd
            .env("SHIM_MCCOMPAT", "0x800000001")
            .env("CUDA_VISIBLE_DEVICES", "0");
        let _ = tx.send(LaunchEvent::Log(LogEntry::new(
            "Emulator",
            LogLevel::Info,
            "Applied Windows hybrid GPU hints (SHIM_MCCOMPAT=0x800000001, CUDA_VISIBLE_DEVICES=0)",
        )));
    }

    let child = emulator_cmd.spawn().map_err(|err| {
        format!(
            "Failed to launch Android emulator ({}): {err}",
            paths.emulator().display()
        )
    })?;

    if let Ok(mut guard) = child_slot.lock() {
        *guard = Some(child);
    }

    let _ = tx.send(LaunchEvent::Log(LogEntry::new(
        "Emulator",
        LogLevel::Info,
        format!(
            "Started emulator -avd {} -gpu {} -memory {} -no-boot-anim -no-snapshot-save",
            config.avd_name, gpu_flag, config.ram_mb
        ),
    )));

    let _ = tx.send(LaunchEvent::Progress {
        fraction: 0.90,
        stage: "Waiting for ADB transport".to_owned(),
    });
    run_sdk_cmd(&paths, &paths.adb(), &["wait-for-device"], None)
        .map_err(|err| err.to_string())?;

    let _ = tx.send(LaunchEvent::Progress {
        fraction: 0.94,
        stage: "Waiting for Android boot completion".to_owned(),
    });
    wait_for_boot_completed(&paths, cancel_flag)?;

    if cancel_flag.load(Ordering::SeqCst) {
        let _ = tx.send(LaunchEvent::Stopped);
        return Ok(());
    }

    let _ = tx.send(LaunchEvent::Progress {
        fraction: 0.98,
        stage: "Checking Teamfight Tactics installation".to_owned(),
    });
    let installed = is_tft_installed(&paths)?;

    if !installed {
        let _ = tx.send(LaunchEvent::Log(LogEntry::new(
            "ADB",
            LogLevel::Warn,
            "Package com.riotgames.league.teamfighttactics not found; opening Google Play Store",
        )));
        open_play_store(&paths)?;
        let _ = tx.send(LaunchEvent::Progress {
            fraction: 1.0,
            stage: "Play Store opened for first-time installation".to_owned(),
        });
        let _ = tx.send(LaunchEvent::PlayStoreRequired);
        return Ok(());
    }

    launch_tft_activity(&paths)?;

    if config.display_mode == DisplayMode::Fullscreen {
        trigger_fullscreen_hotkey(tx);
    }

    let _ = tx.send(LaunchEvent::Log(LogEntry::new(
        "ADB",
        LogLevel::Info,
        "Launched com.riotgames.league.teamfighttactics directly via monkey",
    )));
    let _ = tx.send(LaunchEvent::Progress {
        fraction: 1.0,
        stage: "Teamfight Tactics running".to_owned(),
    });
    let _ = tx.send(LaunchEvent::GameRunning);
    Ok(())
}

/// Dispatches F11 once Android boot finishes so the Qt emulator viewport enters
/// fullscreen automatically when DisplayMode::Fullscreen is selected.
fn trigger_fullscreen_hotkey(tx: &Sender<LaunchEvent>) {
    thread::sleep(Duration::from_millis(600));
    match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => {
            if enigo.key(Key::F11, Direction::Click).is_ok() {
                let _ = tx.send(LaunchEvent::Log(LogEntry::new(
                    "Emulator",
                    LogLevel::Info,
                    "Triggered F11 fullscreen mode automatically",
                )));
            }
        }
        Err(err) => {
            let _ = tx.send(LaunchEvent::Log(LogEntry::new(
                "Emulator",
                LogLevel::Warn,
                format!("Skipped automatic F11 fullscreen toggle: {err}"),
            )));
        }
    }
}

fn wait_for_boot_completed(paths: &SdkPaths, cancel_flag: &Arc<AtomicBool>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(BOOT_TIMEOUT_SECS);
    while Instant::now() < deadline {
        if cancel_flag.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Ok(out) = run_sdk_cmd(
            paths,
            &paths.adb(),
            &["shell", "getprop", "sys.boot_completed"],
            None,
        ) {
            if out.trim() == "1" {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_secs(2));
    }

    Err(format!(
        "Timed out after {BOOT_TIMEOUT_SECS}s waiting for Android sys.boot_completed"
    ))
}

fn is_tft_installed(paths: &SdkPaths) -> Result<bool, String> {
    let stdout = run_sdk_cmd(
        paths,
        &paths.adb(),
        &["shell", "pm", "list", "packages", TFT_PACKAGE_ID],
        None,
    )
    .map_err(|err| err.to_string())?;

    Ok(stdout.contains(&format!("package:{TFT_PACKAGE_ID}")))
}

fn open_play_store(paths: &SdkPaths) -> Result<(), String> {
    let uri = format!("market://details?id={TFT_PACKAGE_ID}");
    run_sdk_cmd(
        paths,
        &paths.adb(),
        &[
            "shell",
            "am",
            "start",
            "-a",
            "android.intent.action.VIEW",
            "-d",
            &uri,
        ],
        None,
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn launch_tft_activity(paths: &SdkPaths) -> Result<(), String> {
    // Using monkey with CATEGORY_LAUNCHER avoids hardcoding an internal Riot
    // Activity class name that may change across game patches.
    run_sdk_cmd(
        paths,
        &paths.adb(),
        &[
            "shell",
            "monkey",
            "-p",
            TFT_PACKAGE_ID,
            "-c",
            "android.intent.category.LAUNCHER",
            "1",
        ],
        None,
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}
