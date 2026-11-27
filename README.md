# TFT-Lite-Launcher

TFT-Lite-Launcher is an open-source, cross-platform native desktop launcher written in Rust (`eframe` / `egui`) that runs Teamfight Tactics Mobile on Windows, macOS, and Linux using Google's official Android Command Line Tools. It requires no Android Studio installation, bundles no proprietary binaries, and introduces no adware or background services.

## Architecture and How It Works

The launcher separates UI rendering from heavy subprocess management so the interface remains responsive during SDK downloads, image installation, and emulator cold boots.

1. **SDK & Portable OpenJDK 17 Provisioning (`src/sdk.rs`)**
   - Detects the host operating system (`windows`, `macos`, `linux`) and CPU architecture (`x86_64` or `aarch64`).
   - Checks for `./android-sdk/jdk/bin/java` (or `java.exe` on Windows). If missing, automatically downloads Eclipse Temurin OpenJDK 17 from the official Adoptium API (`api.adoptium.net`), safely extracts and flattens it into `./android-sdk/jdk`, and injects canonicalized `JAVA_HOME` + prepended `PATH` into every `sdkmanager` and `avdmanager` subprocess—requiring zero host Java installation.
   - Downloads Google's official Android Command Line Tools archive directly from `://google.com` and extracts it into `./android-sdk/cmdline-tools/latest`.
   - Invokes `sdkmanager --licenses` and installs `platform-tools`, `emulator`, and the unrooted Google Play system image matching the host CPU:
     - `system-images;android-34;google_apis_playstore;x86_64` on Intel/AMD hosts (includes Play Integrity support and Intel `libhoudini` ARM translation).
     - `system-images;android-34;google_apis_playstore;arm64-v8a` on Apple Silicon and ARM64 hosts.
   - Provisions a `pixel_tablet` AVD named `TFT_Device` and writes `hw.cpu.ncore=<CPU_CORES>`, `hw.gpu.enabled=yes`, `hw.gpu.mode=<GRAPHICS_BACKEND>`, `hw.ramSize=<RAM>`, `hw.lcd.width=1920`, `hw.lcd.height=1080`, `hw.lcd.density=240`, and `hw.keyboard=yes` into its `config.ini`.

2. **Direct-to-Game Pipeline (`src/launcher.rs`)**
   - Executes all CLI commands on a dedicated worker thread connected to the `egui` render loop via `std::sync::mpsc`.
   - Boots the AVD with `-gpu <backend> -memory <RAM> -no-boot-anim -no-snapshot-save` (defaulting to `-gpu host`). Disabling Quick Boot snapshot saves prevents corrupted device states across restarts and improves Play Integrity reliability.
   - On Windows, automatically sets `SHIM_MCCOMPAT=0x800000001` and `CUDA_VISIBLE_DEVICES=0` when spawning `emulator.exe` so hybrid AMD/Intel and NVIDIA laptops route rendering to the dedicated GPU.
   - When `Display Mode` is set to `Fullscreen`, sends `F11` automatically after game launch to enter native 1920x1080 fullscreen.
   - Polls `adb wait-for-device` and `adb shell getprop sys.boot_completed` until Android finishes booting.
   - Queries `adb shell pm list packages com.riotgames.league.teamfighttactics`. If absent, opens `market://details?id=com.riotgames.league.teamfighttactics` inside Google Play so you can sign in and install. Once installed, launches Teamfight Tactics directly via `adb shell monkey`, bypassing the Android home screen.

3. **Compliant 1-to-1 Keymapper (`src/keymapper.rs`)**
   - Runs a toggleable global key listener (`rdev`) paired with `enigo` and `active-win-pos-rs`.
   - Dispatches a single window-relative mouse click only when the Android Emulator window is focused.
   - Default bindings: `D` (Refresh Shop), `F` (Buy XP), and `E` (Sell Unit), with user-configurable keybinds and X/Y percentage calibration sliders in the `Hotkeys` view.
   - Strictly 1-to-1 input mapping only. No macros, timers, OCR, or memory inspection are implemented.

4. **Hardware Auto-Detection and Configuration Persistence (`src/config.rs`)**
   - Uses `sysinfo` on first run to detect host logical CPU threads and total physical memory, defaulting `cpu_cores` to half of logical threads (clamped between `2` and `6`) and scaling RAM allocation safely.
   - Persists CPU cores, RAM allocation, graphics backend, SDK path, and hotkey coordinates in TOML format at the OS-standard configuration directory (`dirs::config_dir()`):
     - Windows: `%APPDATA%\tft-lite-launcher\config.toml`
     - macOS: `~/Library/Application Support/tft-lite-launcher/config.toml`
     - Linux: `~/.config/tft-lite-launcher/config.toml`

## Required Bundled Assets

### Icons (`assets/icons/`)

All UI iconography uses bundled Hugeicons SVG files in `#FFFFFF` for runtime multiplicative tinting via `egui_extras`. Each icon requires both a `-stroke.svg` (idle or inactive state) and `-solid.svg` (active or confirmed state) file:

- `assets/icons/play-stroke.svg`
- `assets/icons/play-solid.svg`
- `assets/icons/settings-stroke.svg`
- `assets/icons/settings-solid.svg`
- `assets/icons/keyboard-stroke.svg`
- `assets/icons/keyboard-solid.svg`
- `assets/icons/terminal-stroke.svg`
- `assets/icons/terminal-solid.svg`
- `assets/icons/check-circle-stroke.svg`
- `assets/icons/check-circle-solid.svg`
- `assets/icons/alert-circle-stroke.svg`
- `assets/icons/alert-circle-solid.svg`
- `assets/icons/folder-stroke.svg`
- `assets/icons/folder-solid.svg`
- `assets/icons/power-stroke.svg`
- `assets/icons/power-solid.svg`
- `assets/icons/information-circle-stroke.svg`
- `assets/icons/information-circle-solid.svg`
- `assets/icons/chevron-down-stroke.svg`
- `assets/icons/chevron-down-solid.svg`

## Build Prerequisites

### All Platforms

- Rust stable toolchain installed via `rustup` (edition 2021, Rust 1.78 or newer).
- Zero runtime prerequisites: a portable Eclipse Temurin OpenJDK 17 runtime is automatically fetched into `./android-sdk/jdk` on first launch.

### Windows

- Visual Studio Build Tools with the C++ desktop development workload (`x86_64-pc-windows-msvc`).
- Hardware virtualization enabled in UEFI/BIOS.

### macOS

- Xcode Command Line Tools (`xcode-select --install`).
- Grant Input Monitoring and Accessibility permissions to `tft-lite-launcher` in System Settings so `rdev` and `enigo` can dispatch 1-to-1 clicks when the emulator window is focused.

### Linux (Ubuntu / Debian)

Install the X11, Wayland, OpenGL, and input automation development packages required by `eframe`, `rdev`, and `enigo`:

```bash
sudo apt-get update
sudo apt-get install -y \
  libx11-dev \
  libxi-dev \
  libxtst-dev \
  libxkbcommon-dev \
  libwayland-dev \
  libgl1-mesa-dev \
  libxdo-dev \
  pkg-config
```

Note: Global key listening via `rdev` and coordinate clicks via `enigo` require an X11 session or XWayland compatibility layer on Linux desktops.

## Building From Source

```bash
git clone https://github.com
cd tft-lite-launcher
cargo build --release
./target/release/tft-lite-launcher
```

## Performance

### CPU Cores and Memory Allocation

On first launch, TFT-Lite-Launcher queries host logical threads and physical RAM via `sysinfo` and writes `hw.cpu.ncore` and `hw.ramSize` directly into the `TFT_Device` AVD `config.ini`. You can override both values at any time in the `Settings` view (`CPU Cores` from `2` to `8`, `Emulator RAM` from `2048 MB` to `8192 MB`).

### Graphics Backend and 1080p Display Mode

Choose the rendering backend and window mode in `Settings`:

- **Host / Native GPU (`-gpu host`)**: Default. Uses host OpenGL/Vulkan directly for lowest latency and highest frame rate.
- **ANGLE / DirectX 11 (`-gpu angle_indirect`)**: Recommended on Windows laptops experiencing OpenGL driver instability or black frames under `-gpu host`.
- **SwiftShader (`-gpu swiftshader_indirect`)**: CPU-based fallback when host GPU drivers do not support hardware acceleration.
- **Display Mode**: Supports `Borderless Fullscreen` (default), `Fullscreen` (triggers `F11` automatically on boot), and `Windowed 1600x900`. All modes configure the AVD at `1920x1080` (`240 dpi`) for native 16:9 widescreen without black bars.

### Hybrid Windows Laptops (AMD/Intel Integrated + Dedicated NVIDIA/AMD GPU)

TFT-Lite-Launcher automatically passes `SHIM_MCCOMPAT=0x800000001` and `CUDA_VISIBLE_DEVICES=0` to `emulator.exe` on Windows. Because Windows 10 and Windows 11 OS-level GPU scheduling can override driver hints for child QEMU processes, pin the emulator binary to your dedicated GPU once:

1. Open **Windows Settings**, navigate to **System**, **Display**, and then **Graphics**.
2. Under **Add an app**, select **Desktop app** and click **Browse**.
3. Select `./android-sdk/emulator/qemu/windows-x86_64/qemu-system-x86_64.exe` inside your configured SDK directory.
4. Click **Options** for `qemu-system-x86_64.exe`, choose **High Performance** (your dedicated GPU), and click **Save**.

## Troubleshooting

### Emulator Fails to Boot: Virtualization Disabled (VT-x / SVM)

Hardware virtualization is mandatory for running the Android Emulator at playable frame rates.

- **Intel CPUs**: Reboot into UEFI/BIOS setup and enable **Intel Virtualization Technology (VT-x)** and **Intel VT-d**.
- **AMD CPUs**: Reboot into UEFI/BIOS setup and enable **SVM Mode** (Secure Virtual Machine) under Advanced CPU Configuration.
- **Windows Hypervisor**: Ensure **Windows Hypervisor Platform** is enabled in "Turn Windows features on or off" so the Android Emulator can use WHPX alongside Hyper-V or WSL2.
- **Linux KVM Permissions**: Verify `/dev/kvm` exists and add your user to the `kvm` group:

```bash
ls -la /dev/kvm
sudo usermod -aG kvm "$USER"
```

Log out and log back in for group membership changes to take effect.

### Black Screen or Graphics Artifacting

If host OpenGL/Vulkan drivers cause rendering issues with `-gpu host`, switch **Graphics Backend** in the `Settings` view from `Host / Native GPU` to `ANGLE / DirectX 11` (`-gpu angle_indirect`) or `SwiftShader` (`-gpu swiftshader_indirect`) and relaunch.

