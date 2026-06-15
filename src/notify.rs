//! Toast notifications (cross-platform via notify-rust; WinRT toast on Windows).
//! Fired on a worker thread so the UI loop never blocks.

pub fn toast(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        use notify_rust::Notification;
        if let Err(e) = Notification::new().summary(&summary).body(&body).show() {
            tracing::warn!("toast failed: {e}");
        }
    });
}
