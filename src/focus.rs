//! Whether the focused UI element accepts typed text, via UI Automation.
//!
//! Conservative: only reports "no" for clearly non-text targets, so it never
//! blocks a real input box it doesn't recognize.

#[cfg(windows)]
pub fn focused_accepts_text() -> (bool, i32) {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    unsafe {
        // Safe to call repeatedly; ignore "already initialized".
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let automation: IUIAutomation =
            match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(a) => a,
                Err(_) => return (true, -1), // can't check -> don't block
            };

        let element = match automation.GetFocusedElement() {
            Ok(e) => e,
            Err(_) => return (false, -1), // nothing focused -> block
        };

        let ct = element.CurrentControlType().map(|c| c.0).unwrap_or(-1);

        // Clearly non-text targets. Everything else (Edit, Document, ComboBox,
        // Text, Group, Custom, unknown) is allowed.
        const BLOCK: &[i32] = &[
            50000, // Button
            50002, // CheckBox
            50006, // Image
            50008, // List
            50010, // MenuBar
            50011, // MenuItem
            50013, // RadioButton
            50018, // Tab
            50023, // Tree
            50027, // ToolBar
            50029, // StatusBar
            50032, // Window
            50033, // Pane
            50034, // TreeItem
            50037, // TitleBar
        ];
        (!BLOCK.contains(&ct), ct)
    }
}

#[cfg(not(windows))]
pub fn focused_accepts_text() -> (bool, i32) {
    (true, -1)
}
