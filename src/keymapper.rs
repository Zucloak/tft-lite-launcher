use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};
use std::thread;

use active_win_pos_rs::get_active_window;
use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
use rdev::{Event, EventType, Key};

use crate::config::KeybindTarget;
use crate::launcher::{LaunchEvent, LogEntry, LogLevel};

#[derive(Debug, Clone)]
struct SharedKeymapState {
    enabled: bool,
    avd_name: String,
    keybinds: Vec<KeybindTarget>,
}

/// Strictly 1-to-1 global key listener paired with window-relative mouse clicks.
/// Performs zero macros, timers, OCR, or memory inspection.
#[derive(Clone)]
pub struct KeymapperService {
    state: Arc<RwLock<SharedKeymapState>>,
}

impl KeymapperService {
    pub fn spawn(
        enabled: bool,
        avd_name: String,
        keybinds: Vec<KeybindTarget>,
        tx: Sender<LaunchEvent>,
    ) -> Self {
        let state = Arc::new(RwLock::new(SharedKeymapState {
            enabled,
            avd_name,
            keybinds,
        }));

        let listener_state = Arc::clone(&state);
        thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(instance) => instance,
                Err(err) => {
                    let _ = tx.send(LaunchEvent::Log(LogEntry::new(
                        "Keymapper",
                        LogLevel::Error,
                        format!("Failed to initialize input injector: {err}"),
                    )));
                    return;
                }
            };

            let callback = move |event: Event| {
                if let EventType::KeyPress(key) = event.event_type {
                    handle_keypress(key, &listener_state, &mut enigo, &tx);
                }
            };

            if let Err(err) = rdev::listen(callback) {
                let _ = tx.send(LaunchEvent::Log(LogEntry::new(
                    "Keymapper",
                    LogLevel::Error,
                    format!("Global key listener halted: {err:?}"),
                )));
            }
        });

        Self { state }
    }

    pub fn sync(&self, enabled: bool, avd_name: String, keybinds: Vec<KeybindTarget>) {
        if let Ok(mut guard) = self.state.write() {
            guard.enabled = enabled;
            guard.avd_name = avd_name;
            guard.keybinds = keybinds;
        }
    }
}

fn handle_keypress(
    key: Key,
    state: &Arc<RwLock<SharedKeymapState>>,
    enigo: &mut Enigo,
    tx: &Sender<LaunchEvent>,
) {
    let Some(key_label) = key_to_label(key) else {
        return;
    };

    let (enabled, avd_name, matched_target) = match state.read() {
        Ok(guard) => {
            if !guard.enabled {
                return;
            }
            let target = guard
                .keybinds
                .iter()
                .find(|bind| bind.key.eq_ignore_ascii_case(key_label))
                .cloned();
            (guard.enabled, guard.avd_name.clone(), target)
        }
        Err(_) => return,
    };

    if !enabled {
        return;
    }

    let Some(target) = matched_target else {
        return;
    };

    let Ok(active_win) = get_active_window() else {
        return;
    };

    if !is_emulator_window(&active_win.title, &active_win.app_name, &avd_name) {
        return;
    }

    let x = active_win.position.x + active_win.position.width * (f64::from(target.x_pct) / 100.0);
    let y = active_win.position.y + active_win.position.height * (f64::from(target.y_pct) / 100.0);

    // Single instantaneous 1-to-1 coordinate click while preserving zero queuing.
    if enigo
        .move_mouse(x.round() as i32, y.round() as i32, Coordinate::Abs)
        .is_ok()
        && enigo.button(Button::Left, Direction::Click).is_ok()
    {
        let _ = tx.send(LaunchEvent::Log(LogEntry::new(
            "Keymapper",
            LogLevel::Info,
            format!(
                "Dispatched '{}' ({}) at {:.1}%, {:.1}% ({}x{})",
                target.action,
                target.key,
                target.x_pct,
                target.y_pct,
                x.round() as i32,
                y.round() as i32
            ),
        )));
    }
}

fn is_emulator_window(title: &str, app_name: &str, avd_name: &str) -> bool {
    let title_lc = title.to_ascii_lowercase();
    let app_lc = app_name.to_ascii_lowercase();
    let avd_lc = avd_name.to_ascii_lowercase();

    title_lc.contains(&avd_lc)
        || title_lc.contains("android emulator")
        || app_lc.contains("qemu")
        || app_lc.contains("emulator")
}

fn key_to_label(key: Key) -> Option<&'static str> {
    match key {
        Key::KeyA => Some("A"),
        Key::KeyB => Some("B"),
        Key::KeyC => Some("C"),
        Key::KeyD => Some("D"),
        Key::KeyE => Some("E"),
        Key::KeyF => Some("F"),
        Key::KeyG => Some("G"),
        Key::KeyH => Some("H"),
        Key::KeyI => Some("I"),
        Key::KeyJ => Some("J"),
        Key::KeyK => Some("K"),
        Key::KeyL => Some("L"),
        Key::KeyM => Some("M"),
        Key::KeyN => Some("N"),
        Key::KeyO => Some("O"),
        Key::KeyP => Some("P"),
        Key::KeyQ => Some("Q"),
        Key::KeyR => Some("R"),
        Key::KeyS => Some("S"),
        Key::KeyT => Some("T"),
        Key::KeyU => Some("U"),
        Key::KeyV => Some("V"),
        Key::KeyW => Some("W"),
        Key::KeyX => Some("X"),
        Key::KeyY => Some("Y"),
        Key::KeyZ => Some("Z"),
        Key::Num1 => Some("1"),
        Key::Num2 => Some("2"),
        Key::Num3 => Some("3"),
        Key::Num4 => Some("4"),
        Key::Num5 => Some("5"),
        Key::Space => Some("SPACE"),
        _ => None,
    }
}
