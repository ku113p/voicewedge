# Voicewedge — Implementation Plan

> Working name: **voicewedge** (a push-to-talk voice "wedge" that drops transcribed
> speech into whatever text field has focus, and saves the raw audio for later).
> Rename freely before the first public commit.

A Windows **system-tray push-to-talk dictation tool**, written in Rust. Press a hotkey,
speak, release. It records the microphone, transcribes the audio with a cloud Whisper
model, **types the text into the currently focused field** (e.g. the Claude Code chat
box), optionally appends a fixed command string, and **keeps the raw audio file** so the
transcript can be regenerated later with a better model.

This is a **generic, personal-data-free utility**. It knows nothing about Postgres,
BIOGRAPH, or any biography content. Its only outputs are: (1) text injected into the
focused window, and (2) an audio file + a small JSON sidecar dropped into a configured
folder. A separate system (the BIOGRAPH plugin, driven by the live Claude session) picks
those up and decides what to archive. Keeping this boundary clean is what makes the app
safe to open-source.

---

## 1. Goals & non-goals

**Goals**
- One global hotkey → record → transcribe → inject text into the focused field.
- Always persist the raw audio (lossless-enough) so we can re-transcribe later.
- Quality first: record the **whole utterance, then send it in one request** (not
  real-time streaming). There is no special "batching" machinery — just one file, one POST.
- **Two+ usage profiles** with one shared hotkey; the **active profile** (switched in
  settings / the tray menu, not a second hotkey) decides the optional, configurable append
  string: a BIOGRAPH-capture profile that appends `/bio:voice <file>`, and a PLAIN dictation
  profile that just types the recognized text anywhere — same tool, one hotkey.
- Clear feedback: tray-icon state + a toast when done.
- Fully configurable via a TOML file (hotkey, model, append string, paths, inject method).
- Clean enough to publish on GitHub with **zero** personal data in the repo.

**Non-goals (for v1)**
- No real-time / streaming transcription (worse quality; not needed for push-to-talk).
- No Postgres / database writes. No BIOGRAPH coupling.
- No background/always-listening capture (push-to-talk only).
- No cross-platform polish. Windows-first; keep code portable where free, but do not
  block on macOS/Linux.

---

## 2. Why native Rust, not Docker

The user asked whether this could run via Docker to avoid "cluttering Windows". It cannot,
and here is the precise reason: this app's core actions are **host-level GUI / input
operations** —

- create a **system-tray icon** in the Windows shell,
- register a **global hotkey** with the Windows input system,
- capture the **host microphone**,
- read/write the **host clipboard**,
- **inject keystrokes** into the foreground window.

A Linux container has none of these (no Windows shell, no host input bus, no easy mic
passthrough). Docker is the right tool for the *headless* parts of this overall system
(the BIOGRAPH Postgres DB already runs in Docker), but a desktop input utility must be a
**native Windows binary**.

The "clutter" worry is unfounded here: the Rust toolchain is already installed
(`rustup` + `~/.cargo` + `~/.rustup`), it reuses the **existing Visual Studio 2022** MSVC
linker, and the only sizeable artifact is the per-project `target/` build directory, which
is `.gitignore`d and can be wiped any time with `cargo clean`. Keeping the repo **outside
OneDrive** prevents OneDrive from syncing that build churn.

---

## 3. Toolchain status & setup

Already present on this machine (verified):

| Tool | Location |
| --- | --- |
| `rustc` / `cargo` / `rustup` | `C:\Users\Ilia\.cargo\bin\` |
| MSVC linker | Visual Studio Community 2022 (17.8) |

**Verify the active toolchain is MSVC** (it should be, given VS is installed):

```powershell
rustup show          # expect: stable-x86_64-pc-windows-msvc (default)
rustup update        # keep stable current
```

If for any reason the GNU toolchain is the default, switch:

```powershell
rustup default stable-x86_64-pc-windows-msvc
```

No other install steps are required. (Note: `uv` is a Python tool and is unrelated to Rust;
it is not used here.)

---

## 4. Architecture

```
                ┌──────────────────────────────────────────────┐
                │                 voicewedge.exe                │
                │  (single native Windows binary, tray app)     │
                │                                                │
   hotkey ───▶  │  Hotkey ─▶ Recorder ─▶ STT client ─▶ Injector │  ──▶ types text into
                │     │          │            │           │      │      focused window
                │     │          │            │           │      │
                │   Tray     audio file   OpenRouter   Clipboard │
                │  + Toast   + sidecar    /audio/...   (restore) │
                └──────┼──────────┼────────────────────┼─────────┘
                       │          │                    │
                       ▼          ▼                    ▼
                  state icon   files/audio_inbox/   (chat receives:
                  + toast      <ts>.wav + <ts>.json   "<text> /bio:voice <ts>.wav")
```

**Division of responsibility (keep this strict):**

- **voicewedge** = deterministic capture + transcription + injection. No judgement, no DB.
- **BIOGRAPH plugin + live Claude session** = reads the inbox on the `/bio:voice` trigger,
  decides disposition (archive / store-flagged / discard), writes to Postgres. All
  personal-data logic lives here, *not* in the app.

---

## 5. Capture flow (state machine)

```
Idle ──(hotkey)──▶ Recording ──(Enter, swallowed)──▶ Transcribing ──▶ Injecting ──▶ Done ──▶ Idle
  ▲                            │                                  │ error                          │
  │                    (Escape, swallowed)                        │                                │
  └── discard, Idle ◀──────────┘            Error toast ◀─────────┴────────────────────────────────┘
```

**Record mode: hotkey-to-start, Enter-to-stop, Escape-to-cancel.** Press a profile's
hotkey to begin; speak as long as you like; press **Enter** to finish and transcribe, or
**Escape** to cancel and discard. No hold, no second-press, no timed silence detection.

1. **Idle** — tray icon white/neutral. The single hotkey is registered.
2. **Recording** — on the hotkey press, open the input stream and buffer PCM (the **active
   profile** decides what happens at injection). Show the **on-screen recording indicator**
   (see below) and turn the tray icon red. Arm a low-level keyboard hook that watches for
   **Enter** (finish) and **Escape** (cancel).
   - **Critical:** the Enter/Escape that ends recording **must be swallowed** by the hook so
     it does NOT reach the focused window (an un-swallowed Enter would submit an
     empty/partial line in the chat box before the transcript is ready; an un-swallowed
     Escape could dismiss a dialog, etc.). On Windows this is a `WH_KEYBOARD_LL` low-level
     hook that returns non-zero for that keydown to suppress it.
   - On **Enter** keydown: suppress it, stop recording, unhook, hide the indicator →
     Transcribing.
   - On **Escape** keydown: suppress it, stop recording, **discard the buffer (no file
     written, no request)**, unhook, hide the indicator → Idle.
   - Cap max duration (config `max_seconds`, default 5 min) as a safety net.
3. **Transcribing** — write the buffered audio to `files/audio_inbox/<ts>.wav` (16 kHz
   mono), POST it to the STT endpoint, await text. Tray icon "busy". This is where the
   2–25 s latency lives; that is acceptable per requirements.
4. **Injecting** — put transcript on the clipboard, send `Ctrl+V` to the foreground window,
   then — per the **active profile** — type its `append` string (if non-empty, with
   `{filename}` substituted) and press `Enter` if its `press_enter` is set. Restore the
   previous clipboard contents afterward. (E.g. the *biograph* profile appends
   `/bio:voice <file>` + Enter; the *plain* profile appends nothing and just leaves the
   typed text.)
5. **Done** — write the JSON sidecar, show a success toast `✓ Transcribed & saved —
   <ts>.wav`, optional short sound, return to Idle.
6. **Error** at any step → error toast with the reason; the audio file is **kept** so
   nothing is lost and it can be retried/re-transcribed.

> **Note on the two Enters:** the *user's* Enter (step 2) is swallowed and only stops
> recording. The *app's* Enter (step 4, `press_enter`) is generated after injection to
> submit the finished line. They never collide because they happen at different phases.

### On-screen recording indicator (Win+H style)

While recording, show a small **always-on-top, borderless, click-through** overlay window
(like the Windows voice-typing widget) so it's unmistakable that mic capture is live —
e.g. a pulsing red dot + "🎤 Запись… (Enter — стоп)". It appears on Recording, disappears
on stop. The tray icon also turns red as a secondary cue, but the overlay is the primary
indicator the user asked for.

Implementation options (pick in Phase 2):
- **Lightweight:** a `tao` window with `decorations=false`, `always_on_top=true`,
  `skip_taskbar=true`, painted with `softbuffer` + `tiny-skia` (static icon or simple
  pulse). Smallest dependency footprint.
- **Nicer animation:** an `egui`/`eframe` always-on-top overlay for an easy pulsing
  animation, at the cost of a heavier dependency.

Default: start with the lightweight `tao` overlay; upgrade to `egui` only if the animation
matters.

---

## 6. Tech stack (crates) — 2026 best-of

Add with `cargo add <crate>` so Cargo pulls the latest and pins it in `Cargo.lock`.
Rationale included; these are the actively-maintained, de-facto-standard choices as of 2026.

| Concern | Crate | Notes |
| --- | --- | --- |
| Event loop / windowing | `tao` | Tauri's `winit` fork; the `tray-icon` and `global-hotkey` examples are built around its event loop. (Plain `winit` also works.) |
| System tray | `tray-icon` | Tauri ecosystem, the standard for Windows tray icons; supports menu + dynamic icon swaps for state. |
| Global hotkey | `global-hotkey` | Same ecosystem as `tray-icon`; registers OS-level hotkeys and delivers events to the `tao` loop. |
| Audio capture | `cpal` | The standard cross-platform audio I/O crate; enumerate input devices, capture PCM. |
| WAV writing | `hound` | Simple, reliable WAV encode (16-bit PCM). |
| Resampling | `rubato` | Resample device rate (44.1/48 kHz) → 16 kHz mono for Whisper (smaller upload, native rate). |
| HTTP client | `reqwest` (multipart, rustls) | POST the audio as multipart/form-data to OpenRouter. Use `rustls-tls` to avoid OpenSSL. |
| Async runtime | `tokio` | Drive `reqwest`; run the network call off the UI thread so the event loop never blocks. |
| Keystroke / text inject | `enigo` | Fast Unicode text entry independent of keyboard layout (handles Russian correctly); also used to send `Ctrl+V` / `Enter`. |
| Low-level key hook (Enter-to-stop) | `rdev` (with `grab`) **or** `windows` (`WH_KEYBOARD_LL`) | Intercept and **suppress** the Enter that stops recording so it never reaches the focused window. `rdev`'s `grab` is the clean cross-platform path; drop to the `windows` crate's `SetWindowsHookExW` for full control. Must run on a thread with a message pump. |
| On-screen overlay | `tao` window (+`softbuffer`+`tiny-skia`) or `egui`/`eframe` | Always-on-top, borderless, click-through recording indicator. |
| Clipboard | `arboard` | 1Password's cross-platform clipboard; read previous contents, set transcript, restore. |
| Toast notifications | `notify-rust` (delegates to `winrt-notification` on Windows) **or** `winrt-notification` directly | Use `notify-rust` for a simple cross-platform API; drop to `winrt-notification` (0.5.x) if you need fine Windows control (sound, hero image). |
| Config | `serde` + `toml` | Strongly-typed config struct ↔ `config.toml`. |
| Paths | `directories` | Locate `%APPDATA%` for the config file via `ProjectDirs`. |
| Logging | `tracing` + `tracing-subscriber` | Structured logs to a rotating file under `%APPDATA%`. |
| Errors | `anyhow` (app) + `thiserror` (typed module errors) | `anyhow` at the top level, `thiserror` for module error enums. |
| Raw Windows APIs (if needed) | `windows` | Only if a corner needs a direct Win32/WinRT call `tray-icon`/`enigo` don't cover. |

**Best practices baked in:**
- **Never block the event loop.** All recording/transcription runs on worker threads /
  `tokio` tasks; the UI thread only swaps the tray icon and shows toasts via channel
  messages.
- **No `unwrap()`/`panic!` in the event loop.** Bubble errors up to a single handler that
  shows an error toast; a failed transcription must never crash the tray.
- **Idempotent, timestamped filenames** (`YYYYMMDD-HHMMSS-mmm`) to avoid collisions on
  rapid presses.
- **Restore the clipboard** after paste so the tool is non-destructive to the user's
  clipboard.
- Pin everything via `Cargo.lock`; commit the lock file (it's an app, not a library).

---

## 7. Project layout

```
voicewedge/
├─ Cargo.toml
├─ Cargo.lock                 # committed (binary crate)
├─ .gitignore                 # ignores target/, config.toml, *.wav, *.log, secrets
├─ README.md
├─ IMPLEMENTATION_PLAN.md     # this file
├─ config.example.toml        # committed template, NO real key
├─ assets/
│  ├─ icon-idle.ico
│  ├─ icon-recording.ico
│  └─ icon-busy.ico
└─ src/
   ├─ main.rs                 # builds tray + hotkey, runs the tao event loop
   ├─ config.rs               # Config struct, load/merge, %APPDATA% path
   ├─ hotkey.rs               # register/parse the hotkey
   ├─ audio.rs                # cpal capture → resample → WAV
   ├─ stt.rs                  # OpenRouter /audio/transcriptions client
   ├─ inject.rs              # clipboard save/paste/restore + enigo append + Enter
   ├─ notify.rs               # tray icon state + toasts + optional sound
   └─ sidecar.rs              # write <ts>.json metadata
```

---

## 8. Configuration (`config.toml`)

Lives at `%APPDATA%\voicewedge\config.toml` (created from `config.example.toml` on first
run). **Never committed** — it holds the API key. The repo only ships
`config.example.toml` with placeholders.

```toml
# --- Record control ---
hotkey = "CmdOrCtrl+Shift+Space"   # ONE hotkey to START recording
mode   = "enter_to_stop"            # STOP on Enter, CANCEL on Escape (both swallowed)

# --- Profiles (the "two modes") ---
# One hotkey records; the ACTIVE profile decides whether/what to append.
# Switch the active profile in the tray menu (or here) — no second hotkey.
active_profile = "biograph"

[[profiles]]
name        = "biograph"
append      = "/bio:voice {filename}"   # free text; {filename} substituted; can differ per profile
press_enter = true

[[profiles]]
name        = "plain"
append      = ""                          # plain dictation: just type the text, append nothing
press_enter = false

# --- Speech-to-text (OpenRouter) ---
[stt]
provider = "openrouter"
model    = "openai/whisper-large-v3"  # default; alternatives: qwen/qwen3-asr-flash-2026-02-10, google/chirp-3
language = "ru"                        # hint; "" = auto-detect
api_key_env = "OPENROUTER_API_KEY"    # read key from this env var...
api_key     = ""                      # ...or paste here (gitignored file)
endpoint    = "https://openrouter.ai/api/v1/audio/transcriptions"
timeout_secs = 60

# --- Text injection (shared by all profiles; append/press_enter live per-profile above) ---
[inject]
method            = "paste"   # "paste" (clipboard + Ctrl+V, recommended) | "type" (enigo char-by-char)
restore_clipboard = true

# --- Audio storage ---
[audio]
inbox_dir   = 'C:\Users\Ilia\OneDrive\Documents\Claude Sessions\biograph\files\audio_inbox'
sample_rate = 16000         # Hz, mono — Whisper's native rate
format      = "wav"         # "wav" now; "opus" later for compression
max_seconds = 300

# --- Feedback ---
[feedback]
overlay = true   # on-screen "recording…" indicator (Win+H style)
toast   = true   # toast on success/error
sound   = true   # short sound on done
```

**Notes**
- Each profile's `append` (e.g. `"/bio:voice {filename}"`) is what makes a captured chat
  line self-describing: transcript, then the trigger command, then the exact audio
  filename, then `Enter`. The *plain* profile sets `append = ""` and `press_enter = false`,
  turning the same tool into a generic dictation utility for any window. Add more profiles
  (each its own hotkey + command) for other targets.
- Switching profiles is in **settings / the tray menu** (a radio list of profiles marking
  the active one), not a second hotkey. One hotkey always records; the active profile
  decides what is appended. No need to edit config at runtime — pick from the tray.
- API key resolution order: `api_key` (if non-empty) → env var named by `api_key_env`.
  Prefer the env var or the gitignored config; never hardcode in source.

---

## 9. STT integration (OpenRouter)

- Endpoint: `POST https://openrouter.ai/api/v1/audio/transcriptions`.
- Auth: `Authorization: Bearer <OPENROUTER_API_KEY>` (same key already used by BIOGRAPH
  for embeddings — but the app reads its **own** config/env, no shared file dependency).
- Body: `multipart/form-data` with `file` = the WAV bytes and `model` = configured model;
  include `language` when set.
- Response: JSON `{ "text": "...", "usage": {...} }` → extract `text`.
- **Model choice:** default `openai/whisper-large-v3` (strong Russian). Because raw audio
  is retained, models can be A/B-compared on real samples later and swapped via config with
  zero code change. **Do not** use `whisper-large-v3-turbo` (degrades on non-English).
- **Resilience:** one retry on network/5xx with backoff; on final failure, keep the WAV,
  show an error toast, and (optionally) still inject the audio filename so the user knows a
  capture exists to retry.

---

## 10. Audio + sidecar output

For each capture, write two files into `inbox_dir`:

- `<ts>.wav` — 16 kHz mono PCM (the durable "raw-raw" layer).
- `<ts>.json` — sidecar metadata:

```json
{
  "filename": "20260615-143022-187.wav",
  "captured_at": "2026-06-15T14:30:22.187+08:00",
  "duration_secs": 12.4,
  "sample_rate": 16000,
  "stt_model": "openai/whisper-large-v3",
  "language": "ru",
  "transcript": "…full recognized text…",
  "app_version": "0.1.0"
}
```

The sidecar lets the BIOGRAPH side ingest **without re-transcribing** (it already has the
text + the audio path + provenance), and records which model produced the text so a future
re-transcription is a clean operation.

---

## 11. Output line format (to the focused chat)

```
<transcript>  /bio:voice <ts>.wav
```
followed by `Enter` (if `press_enter`). Example:

```
Сегодня закрыл контракт по индексеру, довол, нашёл баг в обработке reorg.  /bio:voice 20260615-143022-187.wav
```

The live Claude session reads the human text as the user's message, and the
`/bio:voice <file>` trailer tells it exactly which inbox capture to consider archiving.

This trailer is the **biograph** profile only. The **plain** profile injects just
`<transcript>` with no trailer and no Enter — a generic dictation drop into any field.
The trailer string is per-profile config, so it can be anything (different command, no
command) without touching code.

---

## 12. Security & privacy (GitHub-ready)

- `.gitignore` excludes: `target/`, `config.toml`, `*.wav`, `*.json` captures, `*.log`,
  any `*.env`. The repo ships only source, `config.example.toml`, and icon assets.
- **No API key, no audio, no transcripts** ever enter git history.
- The app has no DB credentials and no knowledge of personal data by design.
- Before the first public push: scrub absolute user paths from committed files (use
  `config.example.toml` placeholders, not `C:\Users\Ilia\...`).

---

## 13. Packaging & autostart

- Build: `cargo build --release` → single `target\release\voicewedge.exe`.
- Subsystem: build as a **windowed** app (no console) for release
  (`#![windows_subsystem = "windows"]`) so no console flashes; keep console in debug.
- Autostart (optional, reversible): a `.lnk` in
  `shell:startup`, or an HKCU `Run` key — matching the lean-autostart preference on this
  machine. Decide later; not part of MVP.
- Distribution later (optional): MSI/NSIS via `cargo-wix` or `cargo-packager`.

---

## 14. Phased build

**Phase 0 — skeleton (first deliverable after this plan):**
- `cargo new`, add crates, tray icon + menu (Quit), register one hotkey, log "hotkey
  fired". Confirms tray + hotkey + event loop on this machine. Pick the real hotkey here.

**Phase 1 — capture + transcribe:**
- `cpal` record on hotkey, resample, write WAV. POST to OpenRouter, log the transcript.
  Validates audio + STT quality on real Russian speech. A/B the candidate models here.

**Phase 2 — inject + feedback:**
- Clipboard paste + append + Enter into the focused window; clipboard restore. Tray-icon
  state machine + success/error toasts + optional sound. Write the JSON sidecar.
- Tray menu with a **radio list of profiles** to switch the active profile at runtime
  (biograph ↔ plain), plus Quit.

**Phase 3 — polish:**
- Toggle mode, max-duration cap, config hot-reload, error retries, optional autostart,
  optional opus encoding for storage.

**Phase 4 (separate, BIOGRAPH side, not this repo):**
- `/bio:voice` skill: read `audio_inbox`, copy WAV into `files/audio/<hash>.wav`, create a
  `raw_entries` row (`source="voice"`, `file_path`, `content`=transcript, `meta`=sidecar)
  with the disposition decided in-session; dedup by content hash.

---

## 15. Testing

- Unit: config parse/merge, hotkey-string parsing, output-line formatting, sidecar JSON.
- Integration: a fake STT server (or a recorded WAV + live OpenRouter call behind a feature
  flag) to validate the multipart request and response parsing.
- Manual: latency and Russian accuracy on real dictation; clipboard restore correctness;
  injection into the Claude Code box specifically.

---

## 16. Open decisions to confirm before/while building

1. **Hotkey** — single `Ctrl+Shift+Space` (confirmed). The two modes (append command /
   plain) switch via settings + the tray menu, NOT a second hotkey.
2. **Record mode** — **CONFIRMED: hotkey-to-start, Enter-to-finish, Escape-to-cancel**
   (Enter and Escape both swallowed while recording).
3. **STT model** — start `openai/whisper-large-v3`; A/B vs `qwen/qwen3-asr-flash` and
   `google/chirp-3` in Phase 1.
4. **Inject method** — `paste` (recommended) vs `type`.
5. **Audio format at rest** — WAV now; opus later? (storage is trivial either way.)
6. **Overlay style** — lightweight `tao` indicator first; `egui` pulse only if wanted.
```
