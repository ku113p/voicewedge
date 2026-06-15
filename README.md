# voicewedge

A tiny Windows system-tray **push-to-talk dictation** tool. Press a hotkey, speak,
press Enter — your speech is transcribed and typed into whatever window has focus.
The raw audio is kept, so you can re-transcribe later with a better model.

> **Windows-first.** The stack (cpal / tao / tray-icon) is cross-platform, but the
> sound cues, keyboard-layout detection and single-instance guard are Windows-specific
> (cfg-gated). Only Windows is tested.

## How it works

1. Press the hotkey (default **Win+Alt+Space**) — a rising tone plays and the tray
   microphone icon gets a red dot (recording).
2. Speak.
3. Press **Enter** to finish, or **Escape** to cancel. A "ding" plays; the icon dot
   turns orange while transcribing.
4. The transcript is typed into the focused field using the active profile's template,
   and (optionally) Enter is pressed to submit.

Transcription uses the [OpenRouter](https://openrouter.ai) `/audio/transcriptions`
endpoint (default model `openai/gpt-4o-transcribe`).

## Build

Requires the Rust MSVC toolchain (`rustup`) and the Windows SDK.

```
cargo build --release
```

The binary is `target/release/voicewedge.exe` (a windowed app — no console). Pin it
to the taskbar, or make a Start-menu shortcut whose **Start in** is the folder that
holds your `config.toml`.

## Configure

Copy `config.example.toml` to `config.toml` and set your OpenRouter API key.
`config.toml` is searched next to the exe (its working directory), then in
`%APPDATA%\voicewedge\`. It is **gitignored** — your key never enters the repo.

Open it any time from the tray menu → **Settings (edit config)**.
`config.example.toml` documents every option (hotkey, model, language mode, inject
template, audio limit, feedback sounds).

## Hotkey

Default **Win+Alt+Space**. Change `hotkey` in the config — modifiers are `Super`
(the Windows key), `Ctrl`, `Alt`, `Shift`. To list which combos are free on your
machine:

```
cargo run --example probe_hotkeys
```

## Language

`stt.language` can be:

- `"layout"` (default) — follow the active keyboard layout (RU → `ru`, EN → `en`);
- `"auto"` — let the model detect the language;
- a fixed code like `"ru"` or `"en"`.

## Privacy

`config.toml` (your key), the `recordings/` folder (audio + transcripts) and logs are
all gitignored. There is no telemetry; the app talks only to your configured STT
endpoint.

## License

MIT — see [LICENSE](LICENSE).
