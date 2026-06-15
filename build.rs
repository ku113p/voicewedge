fn main() {
    // Embed the app icon into the Windows .exe (shows on the taskbar / shortcut).
    // Tolerant: if the .ico hasn't been generated yet, just skip.
    #[cfg(windows)]
    {
        if std::path::Path::new("assets/voicewedge.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/voicewedge.ico");
            if let Err(e) = res.compile() {
                println!("cargo:warning=icon embed skipped: {e}");
            }
        }
        println!("cargo:rerun-if-changed=assets/voicewedge.ico");
    }
}
