//! Re-run STT on a saved WAV with the app's own HTTP stack: `cargo run --example stt_probe -- <wav> [lang]`.

#[path = "../src/stt.rs"]
mod stt;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg: toml::Value = toml::from_str(&std::fs::read_to_string("config.toml").unwrap()).unwrap();
    let s = &cfg["stt"];
    let lang = args.get(2).map(String::as_str).unwrap_or("ru");
    let t = std::time::Instant::now();
    let r = stt::transcribe(
        std::path::Path::new(&args[1]),
        s["api_key"].as_str().unwrap(),
        s["model"].as_str().unwrap(),
        lang,
        s["endpoint"].as_str().unwrap(),
        s["timeout_secs"].as_integer().unwrap_or(60) as u64,
    );
    println!("{:.1}s {:?}", t.elapsed().as_secs_f32(), r.map(|t| t.chars().take(120).collect::<String>()));
}
