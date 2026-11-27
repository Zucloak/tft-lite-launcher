mod config;
mod icons;
mod keymapper;
mod launcher;
mod sdk;
mod theme;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::Result;
use eframe::egui;
use egui::{
    Align, Button, Color32, Layout, Pos2, Rect, RichText, Rounding, ScrollArea, Sense, Stroke, Vec2,
};

use crate::config::{
    detect_host_hardware, detect_storage_drives, find_drive_for_path, format_bytes_gb, AppConfig,
    DisplayMode, GraphicsBackend, HostHardware, StorageDrive, MAX_CPU_CORES, MIN_CPU_CORES,
    MIN_SDK_FREE_BYTES,
};
use crate::icons::{icon_image, IconKind, IconStyle};
use crate::keymapper::KeymapperService;
use crate::launcher::{EmulatorController, LaunchEvent, LogEntry, LogLevel};
use crate::sdk::{HostTarget, SdkPaths};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavTab {
    Launcher,
    Hotkeys,
    Settings,
    Logs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchState {
    Idle,
    Provisioning,
    PlayStoreRequired,
    Running,
    Error(String),
}

struct TftLiteApp {
    theme: Theme,
    active_tab: NavTab,
    config: AppConfig,
    sdk_dir_input: String,
    host_summary: String,
    host_hardware: HostHardware,
    storage_drives: Vec<StorageDrive>,
    system_image: String,
    launch_state: LaunchState,
    progress: f32,
    progress_stage: String,
    logs: Vec<LogEntry>,
    log_drawer_open: bool,
    advanced_open: bool,
    settings_feedback: Option<String>,
    tx: Sender<LaunchEvent>,
    rx: Receiver<LaunchEvent>,
    controller: EmulatorController,
    keymapper: KeymapperService,
}

impl TftLiteApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let theme = Theme::default();
        theme.apply(&cc.egui_ctx);

        let config = AppConfig::load_or_default().unwrap_or_default();
        let sdk_dir_input = config.sdk_dir.display().to_string();
        let host_hardware = detect_host_hardware();
        let storage_drives = detect_storage_drives();

        let (host_summary, system_image) = match HostTarget::detect() {
            Ok(target) => (
                format!("{} / {}", target.os, target.arch),
                target.system_image_package().to_owned(),
            ),
            Err(err) => (err.to_string(), "Unsupported architecture".to_owned()),
        };

        let (tx, rx) = mpsc::channel();
        let keymapper = KeymapperService::spawn(
            config.keymapper_enabled,
            config.avd_name.clone(),
            config.keybinds.clone(),
            tx.clone(),
        );

        let initial_logs = vec![LogEntry::new(
            "System",
            LogLevel::Info,
            format!("Detected host target: {host_summary}"),
        )];

        Self {
            theme,
            active_tab: NavTab::Launcher,
            config,
            sdk_dir_input,
            host_summary,
            host_hardware,
            storage_drives,
            system_image,
            launch_state: LaunchState::Idle,
            progress: 0.0,
            progress_stage: "Ready to launch".to_owned(),
            logs: initial_logs,
            log_drawer_open: false,
            advanced_open: false,
            settings_feedback: None,
            tx,
            rx,
            controller: EmulatorController::default(),
            keymapper,
        }
    }

    fn drain_events(&mut self) {
        let mut refresh_drives = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LaunchEvent::Log(entry) => {
                    self.logs.push(entry);
                    if self.logs.len() > 2000 {
                        self.logs.drain(0..200);
                    }
                }
                LaunchEvent::Progress { fraction, stage } => {
                    self.progress = fraction.clamp(0.0, 1.0);
                    self.progress_stage = stage;
                }
                LaunchEvent::PlayStoreRequired => {
                    self.launch_state = LaunchState::PlayStoreRequired;
                }
                LaunchEvent::GameRunning => {
                    self.launch_state = LaunchState::Running;
                    refresh_drives = true;
                }
                LaunchEvent::Stopped => {
                    self.launch_state = LaunchState::Idle;
                    self.progress = 0.0;
                    self.progress_stage = "Ready to launch".to_owned();
                    refresh_drives = true;
                }
                LaunchEvent::Failed(message) => {
                    self.launch_state = LaunchState::Error(message);
                    refresh_drives = true;
                }
            }
        }
        if refresh_drives {
            self.storage_drives = detect_storage_drives();
        }
    }

    fn persist_config(&mut self) {
        self.config.sdk_dir = PathBuf::from(self.sdk_dir_input.trim());
        self.config.sanitize();
        self.sdk_dir_input = self.config.sdk_dir.display().to_string();
        self.keymapper.sync(
            self.config.keymapper_enabled,
            self.config.avd_name.clone(),
            self.config.keybinds.clone(),
        );

        match self.config.save() {
            Ok(()) => {
                self.settings_feedback = Some("Configuration saved".to_owned());
                self.logs.push(LogEntry::new(
                    "Config",
                    LogLevel::Info,
                    "Saved configuration to disk",
                ));
            }
            Err(err) => {
                let msg = format!("Failed to save configuration: {err}");
                self.settings_feedback = Some(msg.clone());
                self.logs
                    .push(LogEntry::new("Config", LogLevel::Error, msg));
            }
        }
    }

    fn select_sdk_dir(&mut self, dir: PathBuf) {
        self.sdk_dir_input = dir.display().to_string();
        self.config.sdk_dir = dir;
        self.persist_config();
        if matches!(self.launch_state, LaunchState::Error(_)) {
            self.launch_state = LaunchState::Idle;
            self.progress = 0.0;
            self.progress_stage = "Ready to launch".to_owned();
        }
    }

    fn render_storage_drive_picker(&mut self, ui: &mut egui::Ui, id_salt: &str, enabled: bool) {
        let current_path = Path::new(self.sdk_dir_input.trim());
        let active_drive = find_drive_for_path(current_path, &self.storage_drives);
        let active_mount = active_drive.map(|d| d.mount_point.clone());
        let selected_label = match active_drive {
            Some(drive) => format!(
                "{} ({} free)",
                drive.display_name,
                format_bytes_gb(drive.available_bytes)
            ),
            None => "Custom Path".to_owned(),
        };

        let mut chosen_dir: Option<PathBuf> = None;
        ui.add_enabled_ui(enabled, |ui| {
            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    for drive in &self.storage_drives {
                        let is_selected = active_mount.as_ref() == Some(&drive.mount_point);
                        let space = format_bytes_gb(drive.available_bytes);
                        let text = if drive.available_bytes < MIN_SDK_FREE_BYTES {
                            format!("{} — {space} free (Low space)", drive.display_name)
                        } else {
                            format!("{} — {space} free", drive.display_name)
                        };
                        if ui.selectable_label(is_selected, text).clicked() {
                            chosen_dir = Some(drive.sdk_dir.clone());
                        }
                    }
                });
        });

        if let Some(dir) = chosen_dir {
            self.select_sdk_dir(dir);
        }
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(216.0)
            .frame(
                egui::Frame::none()
                    .fill(self.theme.sidebar_top)
                    .inner_margin(egui::Margin::symmetric(14.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Paint subtle vertical sidebar gradient (#0D1118 to #080B10) and 1px right hairline
                let panel_rect = ui.max_rect().expand2(Vec2::new(14.0, 18.0));
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(panel_rect.left_top(), self.theme.sidebar_top);
                mesh.colored_vertex(panel_rect.right_top(), self.theme.sidebar_top);
                mesh.colored_vertex(panel_rect.left_bottom(), self.theme.sidebar_bottom);
                mesh.colored_vertex(panel_rect.right_bottom(), self.theme.sidebar_bottom);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(1, 3, 2);
                ui.painter().add(egui::Shape::mesh(mesh));
                ui.painter().vline(
                    panel_rect.right(),
                    panel_rect.y_range(),
                    Stroke::new(1.0_f32, self.theme.hairline),
                );

                ui.label(
                    RichText::new("TFT-Lite-Launcher")
                        .size(15.0)
                        .strong()
                        .color(self.theme.text_primary),
                );
                ui.add_space(22.0);

                ui.label(
                    RichText::new("NAVIGATION")
                        .size(10.0)
                        .extra_letter_spacing(1.2)
                        .color(self.theme.text_muted),
                );
                ui.add_space(8.0);

                self.nav_item(ui, NavTab::Launcher, "Launcher", IconKind::Play);
                self.nav_item(ui, NavTab::Hotkeys, "Hotkeys", IconKind::Keyboard);
                self.nav_item(ui, NavTab::Settings, "Settings", IconKind::Settings);
                self.nav_item(ui, NavTab::Logs, "Logs", IconKind::Terminal);

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    let drawer_style = IconStyle::from_active(self.log_drawer_open);
                    let drawer_tint = if self.log_drawer_open {
                        self.theme.teal
                    } else {
                        self.theme.text_muted
                    };

                    let btn = Button::image_and_text(
                        icon_image(IconKind::Terminal, drawer_style, 15.0, drawer_tint),
                        RichText::new(if self.log_drawer_open {
                            "Hide Log Drawer"
                        } else {
                            "Show Log Drawer"
                        })
                        .size(12.5)
                        .color(self.theme.text_secondary),
                    )
                    .fill(self.theme.glass_surface)
                    .stroke(Stroke::new(1.0_f32, self.theme.hairline))
                    .rounding(Rounding::same(self.theme.rounding_sm))
                    .min_size(Vec2::new(ui.available_width(), 32.0));

                    if ui.add(btn).clicked() {
                        self.log_drawer_open = !self.log_drawer_open;
                    }
                    ui.add_space(10.0);
                    self.theme.hairline(ui);
                });
            });
    }

    fn nav_item(&mut self, ui: &mut egui::Ui, tab: NavTab, label: &str, icon: IconKind) {
        let active = self.active_tab == tab;
        let desired = Vec2::new(ui.available_width(), 34.0);
        let (rect, response) = ui.allocate_exact_size(desired, Sense::click());

        let bg = if active {
            self.theme.gold_tint
        } else if response.hovered() {
            self.theme.glass_surface
        } else {
            Color32::TRANSPARENT
        };

        ui.painter()
            .rect_filled(rect, Rounding::same(self.theme.rounding_sm), bg);

        if active {
            let bar = Rect::from_min_size(
                Pos2::new(rect.left(), rect.top() + 5.0),
                Vec2::new(2.0, rect.height() - 10.0),
            );
            ui.painter()
                .rect_filled(bar, Rounding::same(1.0), self.theme.gold);
        }

        let tint = if active {
            self.theme.gold
        } else {
            self.theme.text_secondary
        };
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 18.0, rect.center().y),
            Vec2::splat(16.0),
        );
        icon_image(icon, IconStyle::from_active(active), 16.0, tint).paint_at(ui, icon_rect);

        ui.painter().text(
            Pos2::new(rect.left() + 36.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::new(13.5, egui::FontFamily::Proportional),
            if active {
                self.theme.text_primary
            } else {
                self.theme.text_secondary
            },
        );

        if response.clicked() {
            self.active_tab = tab;
        }
        ui.add_space(2.0);
    }

    fn render_launcher_view(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(720.0);

        // Focused Hero Header
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Teamfight Tactics Mobile")
                        .size(22.0)
                        .strong()
                        .color(self.theme.text_primary),
                );
                ui.add_space(4.0);
                let ram_gb = (self.config.ram_mb as f32) / 1024.0;
                ui.label(
                    RichText::new(format!(
                        "{}  /  {} Cores  /  {:.0} GB  /  {}",
                        self.config.avd_name,
                        self.config.cpu_cores,
                        ram_gb,
                        self.config.graphics_backend.short_label()
                    ))
                    .monospace()
                    .size(12.5)
                    .color(self.theme.text_secondary),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let is_active = matches!(
                    self.launch_state,
                    LaunchState::Provisioning
                        | LaunchState::Running
                        | LaunchState::PlayStoreRequired
                );

                if is_active {
                    let stop_btn = Button::image_and_text(
                        icon_image(
                            IconKind::Power,
                            IconStyle::Solid,
                            15.0,
                            self.theme.gold_text,
                        ),
                        RichText::new("Stop Emulator")
                            .strong()
                            .color(self.theme.gold_text),
                    )
                    .fill(self.theme.danger)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(self.theme.rounding_sm))
                    .min_size(Vec2::new(146.0, 38.0));

                    if ui.add(stop_btn).clicked() {
                        self.controller.stop(&self.config, &self.tx);
                    }
                } else {
                    let play_btn = Button::image_and_text(
                        icon_image(IconKind::Play, IconStyle::Solid, 15.0, self.theme.gold_text),
                        RichText::new("Play TFT")
                            .strong()
                            .color(self.theme.gold_text),
                    )
                    .fill(self.theme.gold)
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(self.theme.rounding_sm))
                    .min_size(Vec2::new(146.0, 38.0));

                    if ui.add(play_btn).clicked() {
                        if self.sdk_dir_input.trim() != self.config.sdk_dir.to_string_lossy() {
                            self.persist_config();
                        }
                        self.launch_state = LaunchState::Provisioning;
                        self.progress = 0.04;
                        self.progress_stage = "Starting setup pipeline".to_owned();
                        self.controller
                            .spawn_launch(self.config.clone(), self.tx.clone());
                    }
                }
            });
        });

        ui.add_space(20.0);

        // Razor-thin 3px progress bar
        let bar_color = match self.launch_state {
            LaunchState::Running => self.theme.teal,
            LaunchState::Error(_) => self.theme.danger,
            _ => self.theme.gold,
        };
        self.theme.thin_progress_bar(ui, self.progress, bar_color);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&self.progress_stage)
                    .size(12.0)
                    .color(self.theme.text_secondary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", self.progress * 100.0))
                        .monospace()
                        .size(11.5)
                        .color(self.theme.text_muted),
                );
            });
        });

        // Conditional Play Store or Error inline banner
        match &self.launch_state {
            LaunchState::PlayStoreRequired => {
                ui.add_space(18.0);
                self.theme.hairline(ui);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add(icon_image(
                        IconKind::AlertCircle,
                        IconStyle::Solid,
                        16.0,
                        self.theme.gold,
                    ));
                    ui.label(
                        RichText::new(
                            "Sign into Google Play in the emulator window to install Teamfight Tactics. Future launches open the game directly.",
                        )
                        .size(12.5)
                        .color(self.theme.gold),
                    );
                });
            }
            LaunchState::Error(msg) => {
                ui.add_space(18.0);
                self.theme.hairline(ui);
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add(icon_image(
                        IconKind::AlertCircle,
                        IconStyle::Solid,
                        16.0,
                        self.theme.danger,
                    ));
                    ui.label(
                        RichText::new(msg)
                            .size(12.5)
                            .color(self.theme.danger),
                    );
                });
            }
            _ => {}
        }

        ui.add_space(26.0);
        self.theme.hairline(ui);
        ui.add_space(18.0);

        // Compact status rows separated by hairline rules
        let paths = SdkPaths::from_config(&self.config);
        let sdk_ready = paths.is_provisioned(&self.config.avd_name);
        let is_active = matches!(
            self.launch_state,
            LaunchState::Provisioning | LaunchState::Running | LaunchState::PlayStoreRequired
        );

        ui.horizontal(|ui| {
            ui.add(icon_image(
                IconKind::CheckCircle,
                IconStyle::from_active(sdk_ready),
                15.0,
                if sdk_ready {
                    self.theme.teal
                } else {
                    self.theme.text_muted
                },
            ));
            ui.label(RichText::new("Android SDK and AVD").color(self.theme.text_primary));
            self.theme.info_tooltip(
                ui,
                "Automated Play Store Provisioning",
                &format!(
                    "Host: {}\nImage: {}\nLocation: {}\nDownloads Google Command Line Tools locally with Play Integrity and libhoudini ARM translation.",
                    self.host_summary,
                    self.system_image,
                    paths.root.display()
                ),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(if sdk_ready {
                        "Provisioned"
                    } else {
                        "Auto-provisions on Play"
                    })
                    .size(12.5)
                    .color(self.theme.text_secondary),
                );
            });
        });

        ui.add_space(14.0);
        self.theme.hairline(ui);
        ui.add_space(14.0);

        ui.horizontal(|ui| {
            ui.add(icon_image(
                IconKind::Folder,
                IconStyle::Stroke,
                15.0,
                self.theme.text_secondary,
            ));
            ui.label(RichText::new("Storage Drive").color(self.theme.text_primary));
            self.theme.info_tooltip(
                ui,
                "SDK & Emulator Storage Disk",
                &format!(
                    "Current SDK directory:\n{}\n\nSelect any drive with at least 8 GB free space before provisioning.",
                    paths.root.display()
                ),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.render_storage_drive_picker(ui, "launcher_storage_drive", !is_active);
            });
        });

        if !sdk_ready && !is_active {
            let current_drive_low = find_drive_for_path(&paths.root, &self.storage_drives)
                .filter(|d| d.available_bytes < MIN_SDK_FREE_BYTES)
                .map(|d| (d.display_name.clone(), d.available_bytes));
            let best_alternative = self
                .storage_drives
                .iter()
                .filter(|d| d.available_bytes >= MIN_SDK_FREE_BYTES)
                .max_by_key(|d| d.available_bytes)
                .map(|d| (d.display_name.clone(), d.available_bytes, d.sdk_dir.clone()));

            if let Some((cur_name, cur_bytes)) = current_drive_low {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{cur_name} has {} free (~8.0 GB required for Android SDK & Play image).",
                            format_bytes_gb(cur_bytes)
                        ))
                        .size(12.0)
                        .color(self.theme.gold),
                    );
                    if let Some((best_name, best_bytes, best_dir)) = best_alternative {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let switch_btn = Button::new(
                                RichText::new(format!(
                                    "Use {best_name} ({})",
                                    format_bytes_gb(best_bytes)
                                ))
                                .size(12.0)
                                .strong()
                                .color(self.theme.gold_text),
                            )
                            .fill(self.theme.gold)
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(self.theme.rounding_sm));
                            if ui.add(switch_btn).clicked() {
                                self.select_sdk_dir(best_dir);
                            }
                        });
                    }
                });
            }
        }

        ui.add_space(14.0);
        self.theme.hairline(ui);
        ui.add_space(14.0);

        ui.horizontal(|ui| {
            ui.add(icon_image(
                IconKind::Keyboard,
                IconStyle::from_active(self.config.keymapper_enabled),
                15.0,
                if self.config.keymapper_enabled {
                    self.theme.teal
                } else {
                    self.theme.text_muted
                },
            ));
            ui.label(RichText::new("1-to-1 Keymapper").color(self.theme.text_primary));
            self.theme.info_tooltip(
                ui,
                "Compliant Input Mapping",
                "Dispatches single window-relative clicks only when TFT_Device is focused. Zero macros, OCR, or memory inspection.",
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(if self.config.keymapper_enabled {
                        "Active (D, F, E)"
                    } else {
                        "Disabled"
                    })
                    .size(12.5)
                    .color(self.theme.text_secondary),
                );
            });
        });
    }

    fn render_hotkeys_view(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(720.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Hotkeys")
                    .size(20.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            self.theme.info_tooltip(
                ui,
                "Strictly 1-to-1 Input",
                "Listens via rdev and sends single window-relative mouse clicks via enigo only while the emulator window has focus.",
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let toggle = Button::image_and_text(
                    icon_image(
                        IconKind::Keyboard,
                        IconStyle::from_active(self.config.keymapper_enabled),
                        14.0,
                        if self.config.keymapper_enabled {
                            self.theme.gold_text
                        } else {
                            self.theme.text_secondary
                        },
                    ),
                    RichText::new(if self.config.keymapper_enabled {
                        "Keymapper Active"
                    } else {
                        "Keymapper Disabled"
                    })
                    .size(12.5)
                    .color(if self.config.keymapper_enabled {
                        self.theme.gold_text
                    } else {
                        self.theme.text_primary
                    }),
                )
                .fill(if self.config.keymapper_enabled {
                    self.theme.teal
                } else {
                    self.theme.glass_surface
                })
                .stroke(Stroke::new(1.0_f32, self.theme.hairline))
                .rounding(Rounding::same(14.0));

                if ui.add(toggle).clicked() {
                    self.config.keymapper_enabled = !self.config.keymapper_enabled;
                    self.persist_config();
                }
            });
        });

        ui.add_space(20.0);
        self.theme.hairline(ui);
        ui.add_space(18.0);

        let mut changed = false;
        for bind in &mut self.config.keybinds {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&bind.action)
                        .strong()
                        .color(self.theme.text_primary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let resp = ui.add_sized(
                        Vec2::new(56.0, 26.0),
                        egui::TextEdit::singleline(&mut bind.key)
                            .font(egui::TextStyle::Monospace),
                    );
                    if resp.changed() {
                        changed = true;
                    }
                    ui.label(RichText::new("Key").size(12.0).color(self.theme.text_muted));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("X %").monospace().size(12.0).color(self.theme.text_muted));
                if ui
                    .add(egui::Slider::new(&mut bind.x_pct, 0.0..=100.0).fixed_decimals(1))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(16.0);
                ui.label(RichText::new("Y %").monospace().size(12.0).color(self.theme.text_muted));
                if ui
                    .add(egui::Slider::new(&mut bind.y_pct, 0.0..=100.0).fixed_decimals(1))
                    .changed()
                {
                    changed = true;
                }
            });
            ui.add_space(14.0);
            self.theme.hairline(ui);
            ui.add_space(14.0);
        }

        if ui.button("Reset Defaults").clicked() {
            self.config.keybinds = AppConfig::default().keybinds;
            changed = true;
        }

        if changed {
            self.persist_config();
        }
    }

    fn render_settings_view(&mut self, ui: &mut egui::Ui) {
        ui.set_max_width(720.0);

        ui.label(
            RichText::new("Settings")
                .size(20.0)
                .strong()
                .color(self.theme.text_primary),
        );
        ui.add_space(20.0);
        self.theme.hairline(ui);
        ui.add_space(20.0);

        // Hardware Sliders (macOS Control Center stretchy glass tracks)
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Hardware Allocation")
                    .size(13.5)
                    .strong()
                    .color(self.theme.text_primary),
            );
            let hw = self.host_hardware;
            self.theme.info_tooltip(
                ui,
                "Auto-Detected via sysinfo",
                &format!(
                    "Detected {} logical threads and {} MB RAM. Recommended defaults: {} cores and {} MB written directly to config.ini.",
                    hw.logical_threads, hw.total_ram_mb, hw.recommended_cores, hw.recommended_ram_mb
                ),
            );
        });
        ui.add_space(12.0);

        let cores_label = format!("{} Cores", self.config.cpu_cores);
        self.theme.glass_slider_u32(
            ui,
            "CPU Cores",
            &mut self.config.cpu_cores,
            MIN_CPU_CORES,
            MAX_CPU_CORES,
            1,
            &cores_label,
        );
        ui.add_space(10.0);

        let ram_label = format!("{} MB", self.config.ram_mb);
        self.theme.glass_slider_u32(
            ui,
            "Emulator RAM",
            &mut self.config.ram_mb,
            2048,
            8192,
            512,
            &ram_label,
        );

        ui.add_space(24.0);
        self.theme.hairline(ui);
        ui.add_space(20.0);

        // Graphics Backend & Display Mode
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Graphics Backend")
                    .size(13.5)
                    .color(self.theme.text_primary),
            );
            self.theme.info_tooltip(
                ui,
                "Hybrid Laptop GPU Routing",
                "On Windows, emulator.exe is automatically spawned with SHIM_MCCOMPAT=0x800000001 and CUDA_VISIBLE_DEVICES=0 to prefer dedicated GPUs.",
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                egui::ComboBox::from_id_salt("graphics_backend_select")
                    .selected_text(self.config.graphics_backend.label())
                    .show_ui(ui, |ui| {
                        for backend in [
                            GraphicsBackend::Host,
                            GraphicsBackend::AngleIndirect,
                            GraphicsBackend::SwiftshaderIndirect,
                        ] {
                            ui.selectable_value(
                                &mut self.config.graphics_backend,
                                backend,
                                backend.label(),
                            );
                        }
                    });
            });
        });

        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Display Mode")
                    .size(13.5)
                    .color(self.theme.text_primary),
            );
            self.theme.info_tooltip(
                ui,
                "1080p Widescreen Geometry",
                "Writes hw.lcd.width=1920, hw.lcd.height=1080, and hw.lcd.density=240 into config.ini. Selecting Fullscreen sends F11 automatically on boot.",
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                egui::ComboBox::from_id_salt("display_mode_select")
                    .selected_text(self.config.display_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            DisplayMode::BorderlessFullscreen,
                            DisplayMode::Fullscreen,
                            DisplayMode::Windowed1600x900,
                        ] {
                            ui.selectable_value(
                                &mut self.config.display_mode,
                                mode,
                                mode.label(),
                            );
                        }
                    });
            });
        });

        ui.add_space(24.0);
        self.theme.hairline(ui);
        ui.add_space(20.0);

        // SDK Path & Storage Drive
        ui.horizontal(|ui| {
            ui.add(icon_image(
                IconKind::Folder,
                IconStyle::Stroke,
                15.0,
                self.theme.text_secondary,
            ));
            ui.label(
                RichText::new("Storage Drive & SDK Directory")
                    .size(13.5)
                    .color(self.theme.text_primary),
            );
            self.theme.info_tooltip(
                ui,
                "Local SDK Storage",
                "Google Command Line Tools, platform-tools, emulator binaries, and the TFT_Device AVD live inside this folder.",
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Refresh").clicked() {
                    self.storage_drives = detect_storage_drives();
                }
                self.render_storage_drive_picker(ui, "settings_storage_drive", true);
            });
        });
        ui.add_space(8.0);
        ui.add_sized(
            Vec2::new(ui.available_width(), 30.0),
            egui::TextEdit::singleline(&mut self.sdk_dir_input)
                .font(egui::TextStyle::Monospace),
        );

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            let save_btn = Button::new(
                RichText::new("Save Configuration")
                    .strong()
                    .color(self.theme.gold_text),
            )
            .fill(self.theme.gold)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(self.theme.rounding_sm));

            if ui.add(save_btn).clicked() {
                self.persist_config();
            }

            if ui.button("Reset Defaults").clicked() {
                self.storage_drives = detect_storage_drives();
                self.config = AppConfig::default();
                self.sdk_dir_input = self.config.sdk_dir.display().to_string();
                self.persist_config();
            }

            if let Some(feedback) = &self.settings_feedback {
                ui.add_space(8.0);
                ui.label(RichText::new(feedback).size(12.0).color(self.theme.teal));
            }
        });

        // Collapsed-by-default Advanced disclosure section
        ui.add_space(24.0);
        self.theme.hairline(ui);
        ui.add_space(14.0);

        let adv_btn = Button::image_and_text(
            icon_image(
                IconKind::ChevronDown,
                IconStyle::from_active(self.advanced_open),
                14.0,
                self.theme.text_secondary,
            ),
            RichText::new("Advanced")
                .size(13.0)
                .color(self.theme.text_secondary),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE);

        if ui.add(adv_btn).clicked() {
            self.advanced_open = !self.advanced_open;
        }

        if self.advanced_open {
            ui.add_space(10.0);
            let toml_preview = toml::to_string_pretty(&self.config).unwrap_or_default();
            ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                ui.label(
                    RichText::new(toml_preview)
                        .monospace()
                        .size(11.5)
                        .color(self.theme.text_secondary),
                );
            });
        }
    }

    fn render_logs_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Logs")
                    .size(20.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Clear Logs").clicked() {
                    self.logs.clear();
                }
            });
        });
        ui.add_space(16.0);
        self.theme.hairline(ui);
        ui.add_space(12.0);
        self.render_log_entries(ui, ui.available_height());
    }

    fn render_log_entries(&self, ui: &mut egui::Ui, max_height: f32) {
        ScrollArea::vertical()
            .max_height(max_height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.logs.is_empty() {
                    ui.label(
                        RichText::new("No runtime events recorded yet.")
                            .monospace()
                            .color(self.theme.text_muted),
                    );
                    return;
                }
                for entry in &self.logs {
                    let level_color = match entry.level {
                        LogLevel::Info => self.theme.teal,
                        LogLevel::Warn => self.theme.gold,
                        LogLevel::Error => self.theme.danger,
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(&entry.timestamp)
                                .monospace()
                                .size(11.5)
                                .color(self.theme.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("[{}]", entry.source))
                                .monospace()
                                .size(11.5)
                                .color(level_color),
                        );
                        ui.label(
                            RichText::new(&entry.message)
                                .monospace()
                                .size(11.5)
                                .color(self.theme.text_primary),
                        );
                    });
                }
            });
    }
}

impl eframe::App for TftLiteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();

        if matches!(self.launch_state, LaunchState::Provisioning) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        self.render_sidebar(ctx);

        if self.log_drawer_open && self.active_tab != NavTab::Logs {
            egui::TopBottomPanel::bottom("log_drawer")
                .resizable(true)
                .default_height(148.0)
                .frame(
                    egui::Frame::none()
                        .fill(Color32::from_rgb(0x08, 0x0B, 0x10))
                        .stroke(Stroke::new(1.0_f32, self.theme.hairline))
                        .inner_margin(egui::Margin::symmetric(18.0, 10.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(icon_image(
                            IconKind::Terminal,
                            IconStyle::Solid,
                            14.0,
                            self.theme.teal,
                        ));
                        ui.label(
                            RichText::new("Live Output")
                                .size(12.0)
                                .strong()
                                .color(self.theme.text_primary),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Collapse").clicked() {
                                self.log_drawer_open = false;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    self.render_log_entries(ui, 108.0);
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(self.theme.canvas)
                    .inner_margin(egui::Margin::symmetric(32.0, 28.0)),
            )
            .show(ctx, |ui| match self.active_tab {
                NavTab::Launcher => self.render_launcher_view(ui),
                NavTab::Hotkeys => self.render_hotkeys_view(ui),
                NavTab::Settings => self.render_settings_view(ui),
                NavTab::Logs => self.render_logs_view(ui),
            });
    }
}

fn main() -> Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TFT-Lite-Launcher")
            .with_inner_size([920.0, 620.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "TFT-Lite-Launcher",
        native_options,
        Box::new(|cc| Ok(Box::new(TftLiteApp::new(cc)))),
    )
    .map_err(|err| anyhow::anyhow!("eframe runtime failed: {err}"))?;

    Ok(())
}
