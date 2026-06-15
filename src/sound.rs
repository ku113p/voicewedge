//! Short audio cues via the native Windows `Beep`. Each cue plays on its own
//! thread (Beep is blocking). Notes are (frequency_hz, duration_ms) sequences.

fn play(notes: Vec<(u32, u32)>) {
    std::thread::spawn(move || {
        for (freq, ms) in notes {
            beep(freq, ms);
        }
    });
}

#[cfg(windows)]
fn beep(freq: u32, ms: u32) {
    use windows::Win32::System::Diagnostics::Debug::Beep;
    unsafe {
        let _ = Beep(freq.clamp(37, 32767), ms);
    }
}

#[cfg(not(windows))]
fn beep(_freq: u32, ms: u32) {
    // No portable console beep; just hold the slot so timing/feel is consistent.
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

pub fn start() {
    play(vec![(660, 70), (950, 80)]);
}

pub fn finish() {
    play(vec![(950, 90), (640, 150)]);
}

pub fn alert() {
    play(vec![(520, 150), (380, 260)]);
}
