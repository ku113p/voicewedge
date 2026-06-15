//! Global keyboard hook for the stop/cancel keys, via `rdev::grab`.
//!
//! While recording, Enter -> Finish and Escape -> Cancel are SWALLOWED so they
//! never reach the focused window — otherwise stop-Enter would submit an empty
//! line in the chat box before the transcript exists.
//!
//! `rdev::grab` runs its own blocking message loop, so it lives on its own thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use rdev::{grab, Event, EventType, Key};

#[derive(Debug, Clone, Copy)]
pub enum StopKind {
    Finish,
    Cancel,
}

pub fn spawn(recording: Arc<AtomicBool>, tx: Sender<StopKind>) {
    std::thread::spawn(move || {
        let callback = move |event: Event| -> Option<Event> {
            if recording.load(Ordering::SeqCst) {
                match event.event_type {
                    EventType::KeyPress(Key::Return) | EventType::KeyPress(Key::KpReturn) => {
                        let _ = tx.send(StopKind::Finish);
                        return None;
                    }
                    EventType::KeyPress(Key::Escape) => {
                        let _ = tx.send(StopKind::Cancel);
                        return None;
                    }
                    // Swallow the matching releases too so no stray key-up leaks through.
                    EventType::KeyRelease(Key::Return)
                    | EventType::KeyRelease(Key::KpReturn)
                    | EventType::KeyRelease(Key::Escape) => {
                        return None;
                    }
                    _ => {}
                }
            }
            Some(event)
        };
        if let Err(e) = grab(callback) {
            tracing::error!("keyboard grab failed: {e:?}");
        }
    });
}
