//! Toast notifications via notify-rust, fired on a worker thread.

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
