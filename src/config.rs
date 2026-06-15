//! Configuration: a TOML file (cross-platform location) holding the OpenRouter key,
//! model, hotkey, profiles, etc. The key lives ONLY here (gitignored) or in an env var.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkey: String,
    pub mode: String,
    pub active_profile: String,
    pub profiles: Vec<Profile>,
    pub stt: Stt,
    pub inject: Inject,
    pub audio: Audio,
    pub feedback: Feedback,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Inject template with placeholders {text} and {filename}.
    /// e.g. "{text}  /bio:voice {filename}" or just "{text}".
    pub template: String,
    pub press_enter: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Stt {
    pub provider: String,
    pub model: String,
    pub language: String,
    pub api_key_env: String,
    pub api_key: String,
    pub endpoint: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Inject {
    pub method: String,
    pub restore_clipboard: bool,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Audio {
    pub inbox_dir: String,
    pub sample_rate: u32,
    pub format: String,
    pub max_seconds: u64,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Feedback {
    pub overlay: bool,
    pub toast: bool,
    pub sound: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "CmdOrCtrl+Shift+Space".into(),
            mode: "enter_to_stop".into(),
            active_profile: "biograph".into(),
            profiles: vec![
                Profile {
                    name: "biograph".into(),
                    template: "{text}  /bio:voice {filename}".into(),
                    press_enter: true,
                },
                Profile { name: "plain".into(), template: "{text}".into(), press_enter: false },
            ],
            stt: Stt::default(),
            inject: Inject::default(),
            audio: Audio::default(),
            feedback: Feedback::default(),
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self { name: String::new(), template: "{text}".into(), press_enter: false }
    }
}

impl Default for Stt {
    fn default() -> Self {
        Self {
            provider: "openrouter".into(),
            model: "openai/gpt-4o-transcribe".into(),
            language: "ru".into(),
            api_key_env: "OPENROUTER_API_KEY".into(),
            api_key: String::new(),
            endpoint: "https://openrouter.ai/api/v1/audio/transcriptions".into(),
            timeout_secs: 60,
        }
    }
}

impl Default for Inject {
    fn default() -> Self {
        Self { method: "type".into(), restore_clipboard: true }
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self { inbox_dir: "recordings".into(), sample_rate: 16000, format: "wav".into(), max_seconds: 300 }
    }
}

impl Default for Feedback {
    fn default() -> Self {
        Self { overlay: true, toast: true, sound: true }
    }
}

impl Config {
    /// Key resolution: explicit `stt.api_key`, else the env var named by `stt.api_key_env`.
    pub fn resolve_api_key(&self) -> Option<String> {
        if !self.stt.api_key.is_empty() {
            return Some(self.stt.api_key.clone());
        }
        std::env::var(&self.stt.api_key_env).ok().filter(|s| !s.is_empty())
    }

    pub fn active_profile(&self) -> Profile {
        self.profiles
            .iter()
            .find(|p| p.name == self.active_profile)
            .cloned()
            .unwrap_or_default()
    }
}

/// Resolve the config file path (cross-platform):
///   1. $VOICEWEDGE_CONFIG if set
///   2. ./config.toml (handy for development)
///   3. the OS config dir (e.g. %APPDATA%\voicewedge\config.toml, ~/.config/voicewedge/...)
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("VOICEWEDGE_CONFIG") {
        return PathBuf::from(p);
    }
    let local = PathBuf::from("config.toml");
    if local.exists() {
        return local;
    }
    if let Some(dirs) = directories::ProjectDirs::from("", "", "voicewedge") {
        return dirs.config_dir().join("config.toml");
    }
    local
}

/// Load config, falling back to defaults (+ env key) if the file is missing or invalid.
pub fn load() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => match toml::from_str::<Config>(&s) {
            Ok(c) => {
                tracing::info!("loaded config from {}", path.display());
                c
            }
            Err(e) => {
                tracing::error!("config parse error ({}): {e}; using defaults", path.display());
                Config::default()
            }
        },
        Err(_) => {
            tracing::warn!(
                "no config at {} — using defaults; set the OpenRouter key in config.toml or ${}",
                path.display(),
                Config::default().stt.api_key_env
            );
            Config::default()
        }
    }
}
