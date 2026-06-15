//! voicewedge — Phase 1.
//!
//! Tray app that records the mic on a global hotkey and transcribes via OpenRouter.
//! Phase 1 stop trigger is a SECOND hotkey press (toggle); the Enter-to-stop /
//! Escape-to-cancel keyboard hook + text injection come in Phase 2.
//!
//! Flow: hotkey -> record (icon red) -> hotkey again -> stop -> write WAV ->
//!       transcribe on a worker thread -> log the recognized text (icon blue).

mod audio;
mod config;
mod stt;

use std::path::PathBuf;
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

/// Build a 32x32 solid-colour tray icon in code (no asset file needed yet).
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

    info!("voicewedge starting (Phase 1)");

    let cfg = config::load();
    let api_key = cfg.resolve_api_key();
    if api_key.is_none() {
        warn!("no OpenRouter API key — recording will save WAV but skip transcription");
    } else {
        info!("STT model = {}, lang = {}", cfg.stt.model, cfg.stt.language);
    }

    let inbox_dir = PathBuf::from(&cfg.audio.inbox_dir);
    if let Err(e) = std::fs::create_dir_all(&inbox_dir) {
        error!("could not create inbox dir {}: {e}", inbox_dir.display());
    }

    let event_loop = EventLoop::new();

    // --- Global hotkey: Ctrl+Shift+Space ---
    let hotkey_manager = GlobalHotKeyManager::new().expect("create hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    let hotkey_id = hotkey.id();
    hotkey_manager.register(hotkey).expect("register Ctrl+Shift+Space");
    info!("hotkey Ctrl+Shift+Space ready (press to start, press again to stop)");

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

    info!("ready — press Ctrl+Shift+Space to record");

    let mut recorder: Option<audio::Recorder> = None;

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100));

        while let Ok(ev) = hotkey_rx.try_recv() {
            if ev.id != hotkey_id || ev.state != HotKeyState::Pressed {
                continue;
            }

            if recorder.is_none() {
                // Start recording.
                match audio::Recorder::start() {
                    Ok(rec) => {
                        recorder = Some(rec);
                        let _ = tray.set_icon(Some(icon_rec.clone()));
                        let _ = tray.set_tooltip(Some("voicewedge — recording… (press again to stop)"));
                        info!("recording started");
                    }
                    Err(e) => error!("could not start recording: {e}"),
                }
            } else {
                // Stop recording -> save -> transcribe.
                let rec = recorder.take().unwrap();
                let (samples, sample_rate) = rec.stop();
                let _ = tray.set_icon(Some(icon_idle.clone()));
                let _ = tray.set_tooltip(Some("voicewedge — transcribing…"));
                let secs = samples.len() as f32 / sample_rate.max(1) as f32;
                info!("recording stopped: {:.1}s, {} samples", secs, samples.len());

                let path = inbox_dir.join(timestamp_name());
                if let Err(e) = audio::write_wav(&path, &samples, sample_rate) {
                    error!("WAV write failed: {e}");
                    continue;
                }
                info!("saved {}", path.display());

                // Transcribe on a worker thread so the UI loop stays responsive.
                if let Some(key) = api_key.clone() {
                    let model = cfg.stt.model.clone();
                    let language = cfg.stt.language.clone();
                    let endpoint = cfg.stt.endpoint.clone();
                    let timeout = cfg.stt.timeout_secs;
                    std::thread::spawn(move || {
                        info!("transcribing {}…", path.display());
                        match stt::transcribe(&path, &key, &model, &language, &endpoint, timeout) {
                            Ok(text) => info!("TRANSCRIPT: {text}"),
                            Err(e) => error!("transcription failed: {e}"),
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
