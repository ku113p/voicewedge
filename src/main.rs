//! voicewedge — Phase 2.
//!
//! Tray app: hotkey starts recording; Enter finishes (transcribe + inject), Escape
//! cancels. The stop keys are swallowed by a global keyboard hook so they don't leak
//! into the focused window. On finish, the transcript (plus the active profile's
//! append string) is pasted into whatever field has focus.
//!
//! Flow: hotkey -> record (icon red, toast) -> Enter -> WAV -> transcribe (worker)
//!       -> paste "<text>  <append>" + Enter -> restore clipboard -> toast.
//!       Escape -> discard, no file, no request.

mod audio;
mod config;
mod hook;
mod inject;
mod notify;
mod stt;

use std::path::PathBuf;
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
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIconBuilder,
};
use tracing::{error, info, warn};

fn make_icon(rgb: [u8; 3]) -> Icon {
    let size: u32 = 32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for _ in 0..(size * size) {
        rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 0xff]);
    }
    Icon::from_rgba(rgba, size, size).expect("valid icon dimensions")
}

fn timestamp_name() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("rec-{millis}.wav")
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("voicewedge starting (Phase 2)");

    let cfg = config::load();
    let api_key = cfg.resolve_api_key();
    if api_key.is_none() {
        warn!("no OpenRouter API key — set it in config.toml; transcription disabled");
    } else {
        info!("STT model = {}, lang = {}", cfg.stt.model, cfg.stt.language);
    }
    let active = cfg.active_profile();
    info!(
        "active profile = '{}' (append = {:?}, enter = {})",
        active.name, active.append, active.press_enter
    );

    let inbox_dir = PathBuf::from(&cfg.audio.inbox_dir);
    if let Err(e) = std::fs::create_dir_all(&inbox_dir) {
        error!("could not create inbox dir {}: {e}", inbox_dir.display());
    }

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

    // --- Tray icon + menu ---
    let menu = Menu::new();
    let quit_item = MenuItem::new("Quit voicewedge", true, None);
    menu.append(&quit_item).expect("append Quit item");
    let quit_id = quit_item.id().clone();

    let icon_idle = make_icon([0x2e, 0x7d, 0xff]); // blue
    let icon_rec = make_icon([0xff, 0x3b, 0x30]); // red

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
                continue; // already recording; Enter/Esc end it
            }
            match audio::Recorder::start() {
                Ok(rec) => {
                    recorder = Some(rec);
                    recording_flag.store(true, Ordering::SeqCst);
                    let _ = tray.set_icon(Some(icon_rec.clone()));
                    let _ = tray.set_tooltip(Some("voicewedge — recording… (Enter=finish, Esc=cancel)"));
                    notify::toast("voicewedge", "Recording… Enter to finish, Esc to cancel");
                    info!("recording started");
                }
                Err(e) => {
                    error!("could not start recording: {e}");
                    notify::toast("voicewedge", &format!("Mic error: {e}"));
                }
            }
        }

        // Finish / cancel from the keyboard hook.
        while let Ok(kind) = stop_rx.try_recv() {
            recording_flag.store(false, Ordering::SeqCst);
            let Some(rec) = recorder.take() else { continue };
            let _ = tray.set_icon(Some(icon_idle.clone()));

            match kind {
                hook::StopKind::Cancel => {
                    drop(rec);
                    let _ = tray.set_tooltip(Some("voicewedge — ready"));
                    info!("recording cancelled");
                    notify::toast("voicewedge", "Cancelled");
                }
                hook::StopKind::Finish => {
                    let (samples, sample_rate) = rec.stop();
                    let secs = samples.len() as f32 / sample_rate.max(1) as f32;
                    info!("recording stopped: {:.1}s, {} samples", secs, samples.len());
                    let _ = tray.set_tooltip(Some("voicewedge — transcribing…"));

                    let path = inbox_dir.join(timestamp_name());
                    if let Err(e) = audio::write_wav(&path, &samples, sample_rate) {
                        error!("WAV write failed: {e}");
                        notify::toast("voicewedge", "WAV write failed");
                        continue;
                    }

                    let Some(key) = api_key.clone() else {
                        warn!("no API key — saved {} but skipping transcription", path.display());
                        continue;
                    };
                    let model = cfg.stt.model.clone();
                    let language = cfg.stt.language.clone();
                    let endpoint = cfg.stt.endpoint.clone();
                    let timeout = cfg.stt.timeout_secs;
                    let profile = active.clone();
                    let method = cfg.inject.method.clone();
                    let restore = cfg.inject.restore_clipboard;

                    std::thread::spawn(move || {
                        match stt::transcribe(&path, &key, &model, &language, &endpoint, timeout) {
                            Ok(text) => {
                                info!("TRANSCRIPT: {text}");
                                let filename = path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let line = if profile.append.is_empty() {
                                    text.clone()
                                } else {
                                    let appended = profile.append.replace("{filename}", &filename);
                                    format!("{text}  {appended}")
                                };
                                match inject::inject(&line, profile.press_enter, &method, restore) {
                                    Ok(()) => notify::toast("voicewedge", "Transcribed & inserted"),
                                    Err(e) => {
                                        error!("injection failed: {e}");
                                        notify::toast("voicewedge", "Transcribed (insert failed)");
                                    }
                                }
                            }
                            Err(e) => {
                                error!("transcription failed: {e}");
                                notify::toast("voicewedge", "Transcription failed");
                            }
                        }
                    });
                }
            }
        }

        while let Ok(ev) = menu_rx.try_recv() {
            if ev.id == quit_id {
                info!("quit selected — exiting");
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}
