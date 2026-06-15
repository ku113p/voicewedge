//! voicewedge — Phase 2 (+feedback, settings menu, inject template, mic icon, sounds, language modes).
//!
//! Tray app: hotkey starts recording; Enter finishes (transcribe + inject), Escape
//! cancels. Stop keys are swallowed by a global keyboard hook. The injected line is
//! built from the active profile's `template`. Language follows config (fixed / "auto"
//! / "layout" = active keyboard layout). Audio cues + a mic tray icon give feedback.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod hook;
mod inject;
mod notify;
mod sound;
mod stt;

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use tao::event_loop::{ControlFlow, EventLoop};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};
use tracing::{error, info, warn};

/// A simple anti-aliased microphone icon drawn in code (64x64 RGBA). `dot` adds a
/// state indicator in the bottom-right (e.g. red = recording, orange = transcribing).
fn make_mic_icon(dot: Option<[u8; 3]>) -> Icon {
    let size: u32 = 64;
    let s = size as f32;
    let mic = [0x4d, 0x9b, 0xff]; // light blue

    let seg = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32| -> f32 {
        let (abx, aby) = (bx - ax, by - ay);
        let t = (((px - ax) * abx + (py - ay) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * abx, ay + t * aby);
        let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        (r - d + 0.5).clamp(0.0, 1.0)
    };
    let circ = |px: f32, py: f32, cx: f32, cy: f32, r: f32| -> f32 {
        let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        (r - d + 0.5).clamp(0.0, 1.0)
    };

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let body = seg(px, py, 0.5 * s, 0.25 * s, 0.5 * s, 0.47 * s, 0.16 * s);
            let stem = seg(px, py, 0.5 * s, 0.66 * s, 0.5 * s, 0.80 * s, 0.045 * s);
            let base = seg(px, py, 0.34 * s, 0.82 * s, 0.66 * s, 0.82 * s, 0.045 * s);
            let mut col = mic;
            let mut a = body.max(stem).max(base);
            if let Some(dc) = dot {
                let dcov = circ(px, py, 0.76 * s, 0.76 * s, 0.20 * s);
                if dcov > 0.0 {
                    col = dc;
                    a = a.max(dcov);
                }
            }
            let i = ((y * size + x) * 4) as usize;
            rgba[i] = col[0];
            rgba[i + 1] = col[1];
            rgba[i + 2] = col[2];
            rgba[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    Icon::from_rgba(rgba, size, size).expect("valid icon")
}

fn timestamp_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("rec-{millis}.wav")
}

fn open_in_editor(path: &std::path::Path) {
    #[cfg(windows)]
    let r = Command::new("notepad").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").args(["-t".as_ref(), path.as_os_str()]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        error!("could not open editor for {}: {e}", path.display());
    }
}

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let r = Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        error!("could not open folder {}: {e}", path.display());
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("voicewedge starting");

    let cfg = config::load();
    let api_key = cfg.resolve_api_key();
    if api_key.is_none() {
        warn!("no OpenRouter API key — set it in config.toml; transcription disabled");
    } else {
        info!("STT model = {}, language mode = {}", cfg.stt.model, cfg.stt.language);
    }
    let active = cfg.active_profile();
    let sound_on = cfg.feedback.sound;

    let inbox_dir = PathBuf::from(&cfg.audio.inbox_dir);
    if let Err(e) = std::fs::create_dir_all(&inbox_dir) {
        error!("could not create inbox dir {}: {e}", inbox_dir.display());
    }
    let config_file = config::config_path();

    let event_loop = EventLoop::new();

    // --- Global hotkey: Ctrl+Shift+Space (start only) ---
    let hotkey_manager = GlobalHotKeyManager::new().expect("create hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    let hotkey_id = hotkey.id();
    hotkey_manager.register(hotkey).expect("register Ctrl+Shift+Space");
    info!("hotkey Ctrl+Shift+Space ready — press to record, Enter to finish, Esc to cancel");

    // --- Keyboard hook for Enter (finish) / Escape (cancel) ---
    let recording_flag = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = mpsc::channel::<hook::StopKind>();
    hook::spawn(recording_flag.clone(), stop_tx);

    // --- Worker -> UI "transcription finished" signal (resets the tray icon) ---
    let (done_tx, done_rx) = mpsc::channel::<()>();

    // --- Tray icon + menu ---
    let menu = Menu::new();
    let settings_item = MenuItem::new("Settings (edit config)", true, None);
    let recordings_item = MenuItem::new("Open recordings folder", true, None);
    let quit_item = MenuItem::new("Quit voicewedge", true, None);
    let _ = menu.append(&settings_item);
    let _ = menu.append(&recordings_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);
    let settings_id = settings_item.id().clone();
    let recordings_id = recordings_item.id().clone();
    let quit_id = quit_item.id().clone();

    let icon_idle = make_mic_icon(None);
    let icon_rec = make_mic_icon(Some([0xff, 0x3b, 0x30])); // red dot
    let icon_busy = make_mic_icon(Some([0xff, 0xa5, 0x00])); // orange dot

    let tray = TrayIconBuilder::new()
        .with_tooltip("voicewedge — ready")
        .with_menu(Box::new(menu))
        .with_icon(icon_idle.clone())
        .build()
        .expect("build tray icon");

    let hotkey_rx = GlobalHotKeyEvent::receiver();
    let menu_rx = MenuEvent::receiver();

    info!("ready");

    let mut recorder: Option<audio::Recorder> = None;

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));

        // Start on hotkey.
        while let Ok(ev) = hotkey_rx.try_recv() {
            if ev.id != hotkey_id || ev.state != HotKeyState::Pressed {
                continue;
            }
            if recorder.is_some() {
                continue;
            }
            match audio::Recorder::start() {
                Ok(rec) => {
                    recorder = Some(rec);
                    recording_flag.store(true, Ordering::SeqCst);
                    let _ = tray.set_icon(Some(icon_rec.clone()));
                    let _ = tray.set_tooltip(Some("voicewedge — recording… (Enter=finish, Esc=cancel)"));
                    notify::toast("voicewedge", "Recording… Enter to finish, Esc to cancel");
                    if sound_on {
                        sound::start();
                    }
                    info!("recording started");
                }
                Err(e) => {
                    error!("could not start recording: {e}");
                    notify::toast("voicewedge", &format!("Mic error: {e}"));
                    if sound_on {
                        sound::alert();
                    }
                }
            }
        }

        // Finish / cancel from the keyboard hook.
        while let Ok(kind) = stop_rx.try_recv() {
            recording_flag.store(false, Ordering::SeqCst);
            let Some(rec) = recorder.take() else { continue };

            match kind {
                hook::StopKind::Cancel => {
                    drop(rec);
                    let _ = tray.set_icon(Some(icon_idle.clone()));
                    let _ = tray.set_tooltip(Some("voicewedge — ready"));
                    info!("recording cancelled");
                    notify::toast("voicewedge", "Cancelled");
                }
                hook::StopKind::Finish => {
                    let (samples, sample_rate) = rec.stop();
                    let secs = samples.len() as f32 / sample_rate.max(1) as f32;
                    info!("recording stopped: {:.1}s, {} samples", secs, samples.len());
                    let _ = tray.set_icon(Some(icon_busy.clone()));
                    let _ = tray.set_tooltip(Some("voicewedge — transcribing… (please wait)"));
                    notify::toast("voicewedge", "Transcribing… (do not click away)");
                    if sound_on {
                        sound::finish();
                    }

                    let path = inbox_dir.join(timestamp_name());
                    if let Err(e) = audio::write_wav(&path, &samples, sample_rate) {
                        error!("WAV write failed: {e}");
                        notify::toast("voicewedge", "WAV write failed");
                        let _ = tray.set_icon(Some(icon_idle.clone()));
                        continue;
                    }

                    let Some(key) = api_key.clone() else {
                        warn!("no API key — saved {} but skipping transcription", path.display());
                        let _ = tray.set_icon(Some(icon_idle.clone()));
                        continue;
                    };
                    let model = cfg.stt.model.clone();
                    let language = config::resolve_language(&cfg.stt.language);
                    let endpoint = cfg.stt.endpoint.clone();
                    let timeout = cfg.stt.timeout_secs;
                    let profile = active.clone();
                    let method = cfg.inject.method.clone();
                    let restore = cfg.inject.restore_clipboard;
                    let done = done_tx.clone();

                    std::thread::spawn(move || {
                        match stt::transcribe(&path, &key, &model, &language, &endpoint, timeout) {
                            Ok(text) => {
                                info!("TRANSCRIPT: {text}");
                                let filename = path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let tmpl = if profile.template.is_empty() {
                                    "{text}".to_string()
                                } else {
                                    profile.template.clone()
                                };
                                let line = tmpl.replace("{text}", &text).replace("{filename}", &filename);
                                match inject::inject(&line, profile.press_enter, &method, restore) {
                                    Ok(()) => notify::toast("voicewedge", "Transcribed & inserted"),
                                    Err(e) => {
                                        error!("injection failed: {e}");
                                        notify::toast("voicewedge", "Transcribed (insert failed)");
                                        if sound_on {
                                            sound::alert();
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("transcription failed: {e}");
                                notify::toast("voicewedge", "Transcription failed");
                                if sound_on {
                                    sound::alert();
                                }
                            }
                        }
                        let _ = done.send(());
                    });
                }
            }
        }

        // Transcription finished -> reset the tray icon to idle.
        while done_rx.try_recv().is_ok() {
            let _ = tray.set_icon(Some(icon_idle.clone()));
            let _ = tray.set_tooltip(Some("voicewedge — ready"));
        }

        // Menu actions.
        while let Ok(ev) = menu_rx.try_recv() {
            if ev.id == quit_id {
                info!("quit selected — exiting");
                *control_flow = ControlFlow::Exit;
            } else if ev.id == settings_id {
                open_in_editor(&config_file);
            } else if ev.id == recordings_id {
                open_folder(&inbox_dir);
            }
        }
    });
}
