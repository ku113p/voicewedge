//! Inject text into the focused window. "type" (default) uses enigo Unicode
//! (layout-independent, reliable for Cyrillic); "paste" uses clipboard + Ctrl+V.

use std::thread::sleep;
use std::time::Duration;

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub fn inject(text: &str, press_enter: bool, method: &str, restore_clipboard: bool) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| anyhow!("enigo init: {e}"))?;

    match method {
        "paste" => {
            let mut clipboard = Clipboard::new().map_err(|e| anyhow!("clipboard open: {e}"))?;
            let saved = if restore_clipboard {
                clipboard.get_text().ok()
            } else {
                None
            };
            clipboard
                .set_text(text.to_string())
                .map_err(|e| anyhow!("clipboard set: {e}"))?;
            sleep(Duration::from_millis(60));
            enigo.key(Key::Control, Direction::Press).map_err(|e| anyhow!("ctrl down: {e}"))?;
            paste_v(&mut enigo)?;
            enigo.key(Key::Control, Direction::Release).map_err(|e| anyhow!("ctrl up: {e}"))?;
            sleep(Duration::from_millis(120));
            if restore_clipboard {
                if let Some(prev) = saved {
                    let _ = clipboard.set_text(prev);
                }
            }
        }
        _ => {
            // "type" (default)
            sleep(Duration::from_millis(30));
            enigo.text(text).map_err(|e| anyhow!("type: {e}"))?;
        }
    }

    if press_enter {
        sleep(Duration::from_millis(40));
        enigo.key(Key::Return, Direction::Click).map_err(|e| anyhow!("enter: {e}"))?;
    }
    Ok(())
}

/// Send the V key combinable with a held Ctrl. On Windows that needs the real
/// virtual key (VK_V = 0x56); enigo's Unicode 'v' can't combine with modifiers.
#[cfg(target_os = "windows")]
fn paste_v(enigo: &mut Enigo) -> Result<()> {
    enigo
        .key(Key::Other(0x56), Direction::Click)
        .map_err(|e| anyhow!("paste v: {e}"))
}

#[cfg(not(target_os = "windows"))]
fn paste_v(enigo: &mut Enigo) -> Result<()> {
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow!("paste v: {e}"))
}
