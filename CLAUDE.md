# vox — Claude Code Instructions

See [DESIGN.md](DESIGN.md) for the architectural overview and design decisions.

## Specs & Plan

Task contracts and implementation plan are in `specs/`:

- `specs/project.spec.md` — Project-level constraints, decisions, boundaries (inherited by all tasks)
- `specs/phase1-skeleton.spec.md` — Phase 1: Workspace 骨架 + 悬浮窗 UI + 菜单栏
- `specs/phase2-audio-hotkey.spec.md` — Phase 2: CGEvent tap 热键 + 音频录制 + 波形动画
- `specs/phase3-transcribe-inject.spec.md` — Phase 3: ominix-api 转录 + 剪贴板注入
- `specs/phase4-llm-settings.spec.md` — Phase 4: LLM refine + 设置窗口 + 完整菜单
- `specs/phase5-polish.spec.md` — Phase 5: 动画打磨 + .app bundle 打包
- `specs/v0.1-release.spec.md` — **v0.1 actual implementation baseline** (23 scenarios, matches code)
- `specs/PLAN.md` — Implementation plan with review checkpoints

When implementing a phase, read the corresponding spec first. The v0.1-release spec is the authoritative record of what the code actually does.

## Project Structure

Cargo workspace with two crates:

- `macos-sys/` — Standalone macOS FFI crate (zero Makepad dependency). Wraps CGEvent tap, NSStatusBar, NSPasteboard, TIS input source, CGEventPost behind pure Rust interfaces.
- `app/` — Makepad 2.0 application crate. Audio capture, UI (floating capsule + settings window), HTTP calls to ominix-api, text injection orchestration.

## Build & Run

```bash
# Run the app (requires ominix-api running separately)
cargo run -p vox

# Release build
cargo build --release -p vox

# Run ominix-api (separate terminal)
cargo run --release -p ominix-api -- --asr-model ~/.OminiX/models/qwen3-asr-1.7b --port 8080
```

## Required Skills

When working on this project, AI agents MUST load the following skills before writing code:

### Makepad 2.0 Skills (load via Skill tool)

| Skill | When to Load |
|-------|-------------|
| `makepad-2.0-app-structure` | App startup, `script_mod!`, `App::run`, `MatchEvent`, `AppMain` |
| `makepad-2.0-dsl` | Splash DSL syntax, `:=`, `+:`, `let` bindings, `mod.widgets` |
| `makepad-2.0-widgets` | Widget catalog, `Label`, `Button`, `TextInput`, `RoundedView`, `DropDown` |
| `makepad-2.0-layout` | `width`/`height`, `flow`, `padding`, `align`, `Filler`, `ScrollYView` |
| `makepad-2.0-shaders` | `draw_bg +: { pixel: fn() }`, SDF2D, `instance`, `Pal.premul` |
| `makepad-2.0-animation` | `Animator`, `AnimatorState`, `Forward`, `Loop`, `ease` functions |
| `makepad-2.0-events` | `handle_actions`, `handle_timer`, `handle_http_response`, `script_eval!` |
| `makepad-screenshot` | Run app and capture screenshot for visual debugging when UI doesn't look right |

### When to Use Screenshot Skill

- After creating or modifying UI layout, run the app and capture a screenshot to verify
- When a visual bug is reported or suspected (invisible text, wrong positioning, missing elements)
- Before marking a UI-related phase as complete

### agent-spec Skills

| Skill | When to Load |
|-------|-------------|
| `agent-spec-authoring` | Writing or editing `.spec.md` files |
| `agent-spec-tool-first` | Implementing code against a spec — verify with `agent-spec verify` |

## Key Conventions

### Makepad 2.0 Syntax (NOT 1.x)

This project uses **Makepad 2.0** with `script_mod!` and Splash DSL:

- Use `script_mod!{}` not `live_design!{}`
- Use `#[derive(Script, ScriptHook)]` not `#[derive(Live, LiveHook)]`
- Use `script_eval!` / `script_apply_eval!` not `apply_over` + `live!`
- Properties use colon syntax: `width: Fill` not `width = Fill`
- Theme access: `theme.color_bg_app` not `(THEME_COLOR_BG)`
- Named instances: `name := Widget{}` with `:=`
- Merge operator: `draw_bg +: { color: #f00 }` to extend, not replace

### Makepad Source Reference

Makepad source is at `/Users/zhangalex/Work/Projects/FW/robius/makepad/`. Key references:

- `examples/floating_panel/` — Floating panel window pattern
- `widgets/src/window_voice_input.rs` — Audio capture + voice transcription patterns
- `platform/src/window.rs` — `MacosWindowConfig` for window types
- `platform/src/audio.rs` — `AudioBuffer`, `AudioInfo` types
- `platform/src/cx_api.rs` — `show_in_dock()`, `copy_to_clipboard()`

### OminiX-MLX Reference

OminiX-MLX source is at `/Users/zhangalex/Work/Projects/FW/robius/OminiX-MLX/`. The app calls its REST API:

- `POST /v1/audio/transcriptions` — OpenAI Whisper-compatible ASR endpoint
- `POST /v1/chat/completions` — OpenAI-compatible LLM endpoint (for refine)

### Rust Edition & Style

- **Edition**: `edition = "2024"` in all Cargo.toml files
- Use `thiserror` for error types in `macos-sys`, `anyhow` in `app`
- Explicit error handling, no `.unwrap()` in production paths
- `#![warn(clippy::all)]` in lib.rs / main.rs
- **Logging**: Use `log!()` macro (from makepad_widgets) in the app crate, NOT `eprintln!()`. The macos-sys crate uses the standard `log` crate since it has no Makepad dependency.

### macos-sys Crate Rules

- **Zero Makepad dependency** — this crate must not import anything from makepad
- All ObjC/CoreFoundation details are hidden behind Rust-only public APIs
- All public functions use Rust types (String, Vec, closures), never raw ObjC pointers
- Platform guard: `#[cfg(target_os = "macos")]` on all modules
- Thread safety: callbacks must be `Send + 'static`; cross-thread communication via `crossbeam-channel`

### Audio Thread Safety

Audio callbacks (`cx.audio_input`) run on a real-time thread. Rules:

- No allocations, no locks, no blocking in audio callbacks
- Use `Arc<AtomicU64>` for RMS data (audio thread → UI thread)
- Use `Arc<Mutex<Vec<f32>>>` for PCM accumulation only with `try_lock` (never block audio thread)
- WAV encoding happens on the main thread after recording stops

### Cross-Thread Communication Pattern

```
macos-sys (CFRunLoop thread)  →  crossbeam channel  →  Makepad timer poll (main thread)
Audio callback (RT thread)    →  AtomicU64           →  NextFrame handler (main thread)
HTTP response                 →  MatchEvent handler   →  UI update via script_eval!
```

## Config

User config stored at `~/.config/vox/config.json`. See DESIGN.md Section 4 for schema.

## macOS Permissions Required

The following permissions must be granted to the terminal app running `cargo run`:

- **Accessibility** (for CGEvent tap / global hotkey): System Settings → Privacy & Security → Accessibility → add terminal app
- **Microphone** (for audio capture): auto-prompted on first run, or System Settings → Privacy & Security → Microphone
- **ominix-api**: must be running separately for Phase 3+ (`cargo run --release -p ominix-api -- --asr-model ~/.OminiX/models/qwen3-asr-1.7b --port 8080`)

## Pre-Commit

Before committing, ensure:

1. `cargo clippy --workspace` passes with no warnings
2. `cargo build --workspace` succeeds
3. Manual test: Option key triggers recording, capsule window appears, release triggers transcription flow
