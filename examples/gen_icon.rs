//! Generate assets/voicewedge.ico (the microphone app icon) at several sizes.
//! Run once after changing the art: cargo run --example gen_icon

use std::fs::File;

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};

/// Anti-aliased microphone, RGBA, on transparent — same shape as the tray icon.
fn mic_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mic = [0x4d_u8, 0x9b, 0xff];

    let seg = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32| -> f32 {
        let (abx, aby) = (bx - ax, by - ay);
        let t = (((px - ax) * abx + (py - ay) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * abx, ay + t * aby);
        let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        (r - d + 0.5).clamp(0.0, 1.0)
    };

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let body = seg(px, py, 0.5 * s, 0.25 * s, 0.5 * s, 0.47 * s, 0.16 * s);
            let stem = seg(px, py, 0.5 * s, 0.66 * s, 0.5 * s, 0.80 * s, 0.045 * s);
            let base = seg(px, py, 0.34 * s, 0.82 * s, 0.66 * s, 0.82 * s, 0.045 * s);
            let a = body.max(stem).max(base);
            let i = ((y * size + x) * 4) as usize;
            rgba[i] = mic[0];
            rgba[i + 1] = mic[1];
            rgba[i + 2] = mic[2];
            rgba[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    rgba
}

fn main() {
    let mut dir = IconDir::new(ResourceType::Icon);
    for size in [16u32, 24, 32, 48, 64, 128, 256] {
        let img = IconImage::from_rgba_data(size, size, mic_rgba(size));
        dir.add_entry(IconDirEntry::encode(&img).expect("encode icon"));
    }
    std::fs::create_dir_all("assets").expect("create assets dir");
    let file = File::create("assets/voicewedge.ico").expect("create .ico");
    dir.write(file).expect("write .ico");
    println!("wrote assets/voicewedge.ico");
}
