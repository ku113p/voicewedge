//! Probe which global hotkeys are FREE on this machine. Run: cargo run --example probe_hotkeys
//! Stop voicewedge first, or its own hotkey shows as TAKEN.

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyManager,
};

fn main() {
    let mgr = GlobalHotKeyManager::new().expect("manager");

    let m_winalt = Modifiers::META | Modifiers::ALT;
    let m_winshift = Modifiers::META | Modifiers::SHIFT;
    let m_winctrl = Modifiers::META | Modifiers::CONTROL;
    let m_ctrlshift = Modifiers::CONTROL | Modifiers::SHIFT;

    let candidates: &[(&str, Modifiers, Code)] = &[
        // Win+Alt cluster (bottom-left, no AltGr issue) — preferred
        ("Win+Alt+Space", m_winalt, Code::Space),
        ("Win+Alt+Z", m_winalt, Code::KeyZ),
        ("Win+Alt+X", m_winalt, Code::KeyX),
        ("Win+Alt+A", m_winalt, Code::KeyA),
        ("Win+Alt+S", m_winalt, Code::KeyS),
        ("Win+Alt+D", m_winalt, Code::KeyD),
        ("Win+Alt+Q", m_winalt, Code::KeyQ),
        ("Win+Alt+Backquote", m_winalt, Code::Backquote),
        ("Win+Shift+Space", m_winshift, Code::Space),
        ("Win+Shift+Z", m_winshift, Code::KeyZ),
        ("Win+Ctrl+Space", m_winctrl, Code::Space),
        ("Ctrl+Shift+Space", m_ctrlshift, Code::Space),
        ("Pause", Modifiers::empty(), Code::Pause),
        ("ScrollLock", Modifiers::empty(), Code::ScrollLock),
        ("F13", Modifiers::empty(), Code::F13),
    ];

    for (name, mods, code) in candidates {
        let hk = HotKey::new(Some(*mods), *code);
        match mgr.register(hk) {
            Ok(()) => {
                println!("FREE   {name}");
                let _ = mgr.unregister(hk);
            }
            Err(e) => println!("TAKEN  {name}  ({e})"),
        }
    }
}
